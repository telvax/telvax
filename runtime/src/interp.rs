//! Tree-walking evaluator over the typed HIR.

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex, RwLock};

use serde_json::Value as Json;
use telvax_hir::Ty;
use telvax_hir::hir::{self, BinOp, ExprKind, Lit, LocalId, UnOp};

use crate::std_impl;

#[cfg(feature = "gpu")]
mod gpu_chain;
use crate::value::{
    Args, Closure, Db, Env, MapKey, MapVal, Native, RequestInfo, StructVal, TaskVal, Value,
    from_json,
};

/// Where `std.tensor` operations on `f32` tensors run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Device {
    #[default]
    Cpu,
    /// A GPU through `wgpu` (needs the `gpu` feature). Results agree with the CPU path to
    /// single-precision tolerance, not bit for bit.
    Gpu,
}

/// Runs `std.dist.map` somewhere other than the calling thread: other processes, other machines,
/// a Ray cluster. Arguments and results travel as JSON, converted with the function's static types.
pub trait Executor: Send + Sync {
    /// Calls the top-level function `function` once per item and returns the results in order.
    fn map(
        &self,
        function: &str,
        items: Vec<serde_json::Value>,
    ) -> Result<Vec<serde_json::Value>, String>;
}

/// Maximum nesting of function calls before evaluation aborts. The native stack grows on demand
/// (see `stacker`), so this is a language limit, not a limit of the thread's stack.
const MAX_DEPTH: usize = 2000;
/// Free native stack kept before each call, and the size of each extra stack segment.
const STACK_RED_ZONE: usize = 256 * 1024;
const STACK_SEGMENT: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeError {
    pub message: String,
}

impl RuntimeError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for RuntimeError {}

/// What a successful endpoint call answers: a status, headers (names in lower case) and a JSON body.
#[derive(Debug, Clone)]
pub struct EndpointReply {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Json,
    /// For a `Response.events` answer: the function that writes the events; see
    /// [`Interp::run_events`]. The body is `null`.
    pub events: Option<Value>,
}

/// What an endpoint call can fail with; maps directly onto an HTTP response.
#[derive(Debug, Clone, PartialEq)]
pub struct EndpointError {
    pub status: u16,
    pub message: String,
}

enum Ctrl {
    Return(Value),
    Break,
    Continue,
    Fail(RuntimeError),
}

impl From<RuntimeError> for Ctrl {
    fn from(e: RuntimeError) -> Self {
        Ctrl::Fail(e)
    }
}

type Eval = Result<Value, Ctrl>;

fn fail<T>(msg: impl Into<String>) -> Result<T, Ctrl> {
    Err(Ctrl::Fail(RuntimeError::new(msg)))
}

pub struct Interp {
    pub(crate) fns: crate::value::NameMap<hir::FnDef>,
    pub(crate) structs: HashMap<String, Vec<(String, Ty)>>,
    pub(crate) enums: HashMap<String, Vec<(String, Vec<Ty>)>>,
    pub(crate) globals: RwLock<HashMap<String, Value>>,
    /// The globals once they are all evaluated: read without a lock from then on, because a request
    /// on every core would otherwise queue on the lock to read a table that no longer changes.
    frozen_globals: std::sync::OnceLock<crate::value::NameMap<Value>>,
    dbs: Mutex<HashMap<String, Arc<Db>>>,
    out: Mutex<Box<dyn std::io::Write + Send>>,
    live: std::sync::atomic::AtomicBool,
    caps: std::sync::atomic::AtomicU8,
    /// Remaining evaluation steps; `u64::MAX` means unlimited.
    fuel: std::sync::atomic::AtomicU64,
    quantization: std::sync::atomic::AtomicU8,
    executor: RwLock<Option<Arc<dyn Executor>>>,
    device: std::sync::atomic::AtomicU8,
    /// Functions that tests replaced with `std.test.mock`, by name (`fetch`) or path (`std::fs::read`).
    mocks: Mutex<HashMap<String, Mock>>,
    mocked: std::sync::atomic::AtomicBool,
    /// Where the statements that ran start, while `start_coverage` is on.
    covered: Mutex<std::collections::HashSet<usize>>,
    covering: std::sync::atomic::AtomicBool,
    /// Called before each statement while a debugger is installed (see `crate::debug`).
    pub(crate) debugger: RwLock<Option<Arc<dyn crate::debug::Debugger>>>,
    pub(crate) debugging: std::sync::atomic::AtomicBool,
    pub(crate) local_decls: std::sync::OnceLock<HashMap<String, Vec<crate::debug::LocalDecl>>>,
    /// Told about every call, statement and `std` operation while a profiler is installed.
    pub(crate) profiler: RwLock<Option<Arc<crate::profile::Profiler>>>,
    pub(crate) profiling: std::sync::atomic::AtomicBool,
    /// This interpreter as an `Arc`, once [`Interp::into_shared`] was called: what a background task
    /// needs to outlive the call that started it.
    me: std::sync::OnceLock<std::sync::Weak<Interp>>,
    /// Background tasks running now.
    background: std::sync::atomic::AtomicUsize,
}

/// How many `std.task.spawn` tasks may run at once.
const MAX_BACKGROUND: usize = 256;

struct Mock {
    replacement: Value,
    calls: i64,
}

impl fmt::Debug for Interp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Interp")
            .field("fns", &self.fns.keys().collect::<Vec<_>>())
            .finish_non_exhaustive()
    }
}

struct Ctx {
    env: Env,
    depth: usize,
}

impl Interp {
    /// Prepares `program` for execution and evaluates its global `let`s in order.
    pub fn new(program: hir::Program) -> Result<Interp, RuntimeError> {
        Self::with_live(program, false)
    }

    /// Like [`Interp::new`], choosing real services (`live`) before the globals are evaluated.
    pub fn with_live(program: hir::Program, live: bool) -> Result<Interp, RuntimeError> {
        Self::with_caps(program, live, crate::Caps::ALL)
    }

    /// Like [`Interp::with_live`], also limiting what the program may do outside itself. The
    /// limit is in force while the globals are evaluated.
    pub fn with_caps(
        program: hir::Program,
        live: bool,
        caps: crate::Caps,
    ) -> Result<Interp, RuntimeError> {
        let interp = Interp {
            structs: program
                .structs
                .iter()
                .map(|s| (s.name.clone(), s.fields.clone()))
                .collect(),
            enums: program
                .enums
                .iter()
                .map(|e| (e.name.clone(), e.variants.clone()))
                .collect(),
            fns: program
                .fns
                .into_iter()
                .map(|f| (f.name.clone(), f))
                .collect(),
            globals: RwLock::new(HashMap::new()),
            frozen_globals: std::sync::OnceLock::new(),
            dbs: Mutex::new(HashMap::new()),
            out: Mutex::new(Box::new(std::io::stdout())),
            live: std::sync::atomic::AtomicBool::new(live),
            caps: std::sync::atomic::AtomicU8::new(caps.bits()),
            fuel: std::sync::atomic::AtomicU64::new(u64::MAX),
            quantization: std::sync::atomic::AtomicU8::new(0),
            executor: RwLock::new(None),
            device: std::sync::atomic::AtomicU8::new(0),
            mocks: Mutex::new(HashMap::new()),
            mocked: std::sync::atomic::AtomicBool::new(false),
            covered: Mutex::new(std::collections::HashSet::new()),
            covering: std::sync::atomic::AtomicBool::new(false),
            debugger: RwLock::new(None),
            debugging: std::sync::atomic::AtomicBool::new(false),
            local_decls: std::sync::OnceLock::new(),
            profiler: RwLock::new(None),
            profiling: std::sync::atomic::AtomicBool::new(false),
            me: std::sync::OnceLock::new(),
            background: std::sync::atomic::AtomicUsize::new(0),
        };
        for g in &program.globals {
            let mut ctx = Ctx {
                env: Env::new(),
                depth: 0,
            };
            let v = interp.eval(&g.init, &mut ctx).map_err(|c| match c {
                Ctrl::Fail(e) => RuntimeError::new(format!("while initialising `{}`: {e}", g.name)),
                Ctrl::Return(_) => RuntimeError::new("`return` outside a function"),
                Ctrl::Break | Ctrl::Continue => {
                    RuntimeError::new("`break` or `continue` outside a loop")
                }
            })?;
            interp
                .globals
                .write()
                .unwrap_or_else(|e| e.into_inner())
                .insert(g.name.clone(), v);
        }
        let all = std::mem::take(&mut *interp.globals.write().unwrap_or_else(|e| e.into_inner()));
        let _ = interp.frozen_globals.set(all.into_iter().collect());
        Ok(interp)
    }

    /// Redirects `std.io.print`/`println` (stdout by default).
    pub fn set_output(&self, writer: impl std::io::Write + Send + 'static) {
        *self.out.lock().unwrap_or_else(|e| e.into_inner()) = Box::new(writer);
    }

    pub(crate) fn write_out(&self, text: &str) -> Result<(), RuntimeError> {
        use std::io::Write;
        let mut out = self.out.lock().unwrap_or_else(|e| e.into_inner());
        out.write_all(text.as_bytes())
            .and_then(|()| out.flush())
            .map_err(|e| RuntimeError::new(format!("cannot write output: {e}")))
    }

    /// Switches the AI services between offline stand-ins (the default) and the real Ollama,
    /// OpenAI and PostgreSQL backends. Must be called before the program creates its clients.
    pub fn set_live(&self, live: bool) {
        self.live.store(live, std::sync::atomic::Ordering::SeqCst);
    }

    /// Embeds `text` the way `OllamaEmbedder.embed` would: with Ollama (`OLLAMA_HOST`, model
    /// `TELVAX_EMBED_MODEL`) when live, otherwise with the offline stand-in. Used to seed stores.
    pub fn embed(&self, text: &str) -> Result<Vec<f32>, RuntimeError> {
        #[cfg(feature = "live")]
        if self.is_live() {
            let host =
                std::env::var("OLLAMA_HOST").unwrap_or_else(|_| "http://localhost:11434".into());
            let model =
                std::env::var("TELVAX_EMBED_MODEL").unwrap_or_else(|_| "nomic-embed-text".into());
            return crate::services::ollama_embed(
                &host,
                &model,
                text,
                telvax_hir::stdlib::EMBEDDING_DIM as usize,
            );
        }
        Ok(crate::std_impl::embed_text(text))
    }

    /// Stops execution with an error after `steps` more evaluation steps (for untrusted code, e.g.
    /// a web playground). Unlimited by default.
    pub fn set_step_limit(&self, steps: u64) {
        self.fuel.store(steps, std::sync::atomic::Ordering::Relaxed);
    }

    /// Converts request-style JSON into a value of static type `ty`.
    pub fn value_from_json(&self, ty: &Ty, json: &Json) -> Result<Value, String> {
        from_json(ty, json, &self.structs, &self.enums)
    }

    /// Runs `f32` matrix multiplications at the given precision (see [`Quantization`]).
    pub fn set_quantization(&self, q: telvax_hir::Quantization) {
        let v = match q {
            telvax_hir::Quantization::None => 0,
            telvax_hir::Quantization::Int8 => 1,
            telvax_hir::Quantization::Int8Channel => 2,
            telvax_hir::Quantization::Int4Channel => 3,
        };
        self.quantization
            .store(v, std::sync::atomic::Ordering::Relaxed);
    }

    pub(crate) fn quantization(&self) -> telvax_hir::Quantization {
        match self.quantization.load(std::sync::atomic::Ordering::Relaxed) {
            1 => telvax_hir::Quantization::Int8,
            2 => telvax_hir::Quantization::Int8Channel,
            3 => telvax_hir::Quantization::Int4Channel,
            _ => telvax_hir::Quantization::None,
        }
    }

    /// Where `std.dist.map` runs its calls. Without one, calls run on this machine's threads.
    pub fn set_executor(&self, executor: Arc<dyn Executor>) {
        *self.executor.write().unwrap_or_else(|e| e.into_inner()) = Some(executor);
    }

    pub(crate) fn executor(&self) -> Option<Arc<dyn Executor>> {
        self.executor
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Calls `name` with JSON arguments (converted by the parameter types) and returns the result
    /// as JSON; an `Ok(v)` result becomes `v`. This is how workers run remote calls.
    pub fn call_json(&self, name: &str, args: &[Json]) -> Result<Json, String> {
        let (params, _) = self
            .signature(name)
            .ok_or_else(|| format!("no function `{name}`"))?;
        if params.len() != args.len() {
            return Err(format!(
                "`{name}` takes {} argument(s), got {}",
                params.len(),
                args.len()
            ));
        }
        let values = params
            .iter()
            .zip(args)
            .map(|((pname, ty), arg)| {
                self.value_from_json(ty, arg)
                    .map_err(|e| format!("argument `{pname}`: {e}"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        match self.call(name, values).map_err(|e| e.to_string())? {
            Value::Err(e) => Err(format!("Err({e})")),
            v => Ok(v.to_json()),
        }
    }

    /// Where tensor operations run (see [`Device`]).
    pub fn set_device(&self, device: Device) {
        let v = match device {
            Device::Cpu => 0,
            Device::Gpu => 1,
        };
        self.device.store(v, std::sync::atomic::Ordering::Relaxed);
    }

    pub(crate) fn device(&self) -> Device {
        match self.device.load(std::sync::atomic::Ordering::Relaxed) {
            1 => Device::Gpu,
            _ => Device::Cpu,
        }
    }

    /// Fails unless the program may call the standard library function `path`. Reaching the
    /// network only counts when the services are live; the offline stand-ins touch nothing.
    pub(crate) fn require(&self, path: &str) -> Result<(), RuntimeError> {
        let Some(need) = crate::Capability::of_function(path) else {
            return Ok(());
        };
        let caps = crate::Caps::from_bits(self.caps.load(std::sync::atomic::Ordering::Relaxed));
        if caps.allows(need) || (need == crate::Capability::Net && !self.is_live()) {
            return Ok(());
        }
        Err(RuntimeError::new(format!(
            "`{}` needs the `{}` capability; run with `--allow {}`",
            path.replace("::", "."),
            need.name(),
            need.name()
        )))
    }

    /// The key a test names a function by: a std path (`"std.fs.read"`) or a function value.
    pub(crate) fn mock_key(&self, target: &Value) -> Result<String, RuntimeError> {
        match target {
            Value::Func(name) if self.fns.contains_key(&**name) => Ok(name.to_string()),
            Value::Func(name) => Err(RuntimeError::new(format!("no function `{name}`"))),
            Value::Str(path) => {
                let key = path.replace('.', "::");
                if key.starts_with("std::test::") {
                    Err(RuntimeError::new("the test functions cannot be mocked"))
                } else if telvax_hir::stdlib::function_paths().any(|p| p == key) {
                    Ok(key)
                } else {
                    Err(RuntimeError::new(format!(
                        "`{path}` is not a std function; mock your own function by passing it without quotes: `std.test.mock(fetch, fake_fetch)`"
                    )))
                }
            }
            other => Err(RuntimeError::new(format!(
                "cannot mock {}: pass a function or the path of a std function",
                other.type_name()
            ))),
        }
    }

    pub(crate) fn set_mock(&self, key: String, replacement: Value) {
        self.mocks.lock().unwrap_or_else(|e| e.into_inner()).insert(
            key,
            Mock {
                replacement,
                calls: 0,
            },
        );
        self.mocked
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }

    pub(crate) fn mock_calls(&self, key: &str) -> Option<i64> {
        let mocks = self.mocks.lock().unwrap_or_else(|e| e.into_inner());
        mocks.get(key).map(|m| m.calls)
    }

    /// Starts recording which statements run (see `telvax_hir::coverage`).
    pub fn start_coverage(&self) {
        self.covering
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }

    /// The start offsets of the statements that ran since `start_coverage`.
    pub fn covered(&self) -> std::collections::HashSet<usize> {
        self.covered
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// A `std` function called while profiling: measured, with the size of what went in and out.
    fn profiled_call(
        &self,
        profiler: &Arc<crate::profile::Profiler>,
        path: &str,
        args: Args,
        result: &Ty,
    ) -> Result<Value, RuntimeError> {
        let name = path.replace("::", ".");
        let kept = args.clone();
        let _probe = crate::profile::Probe::enter(profiler.clone(), &name, true);
        let began = std::time::Instant::now();
        let out = std_impl::call_function(self, path, args, result);
        if let Ok(v) = &out {
            profiler.operation(&name, began.elapsed(), &kept, v);
        }
        out
    }

    /// A statement is about to run: record it for coverage and let a debugger look.
    fn point(&self, start: usize, ctx: &Ctx) {
        self.cover(start);
        if self.profiling.load(std::sync::atomic::Ordering::Relaxed) {
            crate::profile::statement(start);
        }
        if self.debugging.load(std::sync::atomic::Ordering::Relaxed) {
            let debugger = self
                .debugger
                .read()
                .unwrap_or_else(|e| e.into_inner())
                .clone();
            if let Some(d) = debugger {
                crate::debug::statement(self, &*d, start, &ctx.env);
            }
        }
    }

    fn cover(&self, start: usize) {
        if self.covering.load(std::sync::atomic::Ordering::Relaxed) {
            self.covered
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(start);
        }
    }

    /// Puts back every function a test replaced. `telvax test` does this after each test.
    pub fn reset_mocks(&self) {
        self.mocks.lock().unwrap_or_else(|e| e.into_inner()).clear();
        self.mocked
            .store(false, std::sync::atomic::Ordering::Relaxed);
    }

    /// The replacement for `key`, counting the call, when a test mocked it.
    fn mock_for(&self, key: &str) -> Option<Value> {
        if !self.mocked.load(std::sync::atomic::Ordering::Relaxed) {
            return None;
        }
        let mut mocks = self.mocks.lock().unwrap_or_else(|e| e.into_inner());
        mocks.get_mut(key).map(|m| {
            m.calls += 1;
            m.replacement.clone()
        })
    }

    /// Whether `key` is mocked, without counting a call.
    fn is_mocked(&self, key: &str) -> bool {
        self.mocked.load(std::sync::atomic::Ordering::Relaxed)
            && self
                .mocks
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .contains_key(key)
    }

    pub fn is_live(&self) -> bool {
        self.live.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// The database behind `url`, opened on first use: PostgreSQL when live, otherwise an
    /// in-memory store that embedders can seed.
    pub fn db_for(&self, url: &str) -> Result<Arc<Db>, RuntimeError> {
        let mut dbs = self.dbs.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(db) = dbs.get(url) {
            return Ok(db.clone());
        }
        #[cfg(feature = "live")]
        let pg = if self.is_live() && url.starts_with("postgres") {
            Some(crate::services::PgStore::connect(
                url,
                telvax_hir::stdlib::EMBEDDING_DIM as usize,
            )?)
        } else {
            None
        };
        let db = Arc::new(Db {
            url: url.to_string(),
            #[cfg(feature = "live")]
            pg,
            ..Db::default()
        });
        dbs.insert(url.to_string(), db.clone());
        Ok(db)
    }

    /// Takes over what a program keeps outside itself from `from`: the databases it opened and
    /// whether the services are real. A session that builds a new interpreter for each input uses
    /// this so that the data survives from one input to the next.
    pub fn inherit_state(&self, from: &Interp) {
        let from_dbs = from.dbs.lock().unwrap_or_else(|e| e.into_inner());
        let mut dbs = self.dbs.lock().unwrap_or_else(|e| e.into_inner());
        for (url, db) in from_dbs.iter() {
            dbs.insert(url.clone(), db.clone());
        }
        self.set_live(from.is_live());
    }

    /// Every database opened so far (one per distinct URL).
    pub fn dbs(&self) -> Vec<Arc<Db>> {
        self.dbs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .cloned()
            .collect()
    }

    /// The fields of struct `name`, in declaration order.
    pub fn struct_fields(&self, name: &str) -> Option<&[(String, Ty)]> {
        self.structs.get(name).map(Vec::as_slice)
    }

    /// Parameter names / types and the return type of function `name`.
    pub fn signature(&self, name: &str) -> Option<(Vec<(String, Ty)>, Ty)> {
        self.fns.get(name).map(|f| {
            (
                f.params
                    .iter()
                    .map(|p| (p.name.clone(), p.ty.clone()))
                    .collect(),
                f.ret.clone(),
            )
        })
    }

    pub fn function_names(&self) -> Vec<String> {
        let mut v: Vec<_> = self.fns.keys().cloned().collect();
        v.sort();
        v
    }

    /// Wraps an Arrow batch as a `Table<schema>`, matching columns to the struct's fields by name
    /// (casting where needed).
    pub fn table_from_batch(
        &self,
        schema: &str,
        batch: &arrow::record_batch::RecordBatch,
    ) -> Result<Value, RuntimeError> {
        let fields = self
            .structs
            .get(schema)
            .ok_or_else(|| RuntimeError::new(format!("unknown type `{schema}`")))?;
        let batch = crate::table::conform(batch, fields)?;
        Ok(Value::Table(Arc::new(crate::value::TableVal {
            schema: schema.to_string(),
            batch,
        })))
    }

    pub fn endpoint_names(&self) -> Vec<String> {
        let mut v: Vec<_> = self
            .fns
            .values()
            .filter(|f| f.endpoint.is_some())
            .map(|f| f.name.clone())
            .collect();
        v.sort();
        v
    }

    /// Calls a Telvax function by name. An `async fn` runs to the end, as if it were awaited.
    pub fn call(&self, name: &str, args: Vec<Value>) -> Result<Value, RuntimeError> {
        let f = self
            .fns
            .get(name)
            .ok_or_else(|| RuntimeError::new(format!("no function `{name}`")))?;
        let finish = |started: Value| match started {
            Value::Task(_) => self.run_task(&started, 0),
            done => Ok(done),
        };
        self.call_fn(f, args.into(), 0)
            .and_then(finish)
            .map_err(Self::plain_error)
    }

    fn plain_error(c: Ctrl) -> RuntimeError {
        match c {
            Ctrl::Fail(e) => e,
            Ctrl::Return(_) => RuntimeError::new("unexpected return"),
            Ctrl::Break | Ctrl::Continue => {
                RuntimeError::new("`break` or `continue` outside a loop")
            }
        }
    }

    /// Runs a task (what an `async fn` gave, or `std.task.sleep`, or `std.task.gather`).
    fn run_task(&self, task: &Value, depth: usize) -> Eval {
        let Value::Task(t) = task else {
            return fail(format!("`await` needs a task, found {}", task.type_name()));
        };
        match &**t {
            TaskVal::Call { name, args } => {
                let f = self.fns.get(name).ok_or_else(|| {
                    Ctrl::Fail(RuntimeError::new(format!("no function `{name}`")))
                })?;
                self.run_fn(f, args.clone().into(), depth)
            }
            TaskVal::Sleep(ms) => {
                std::thread::sleep(std::time::Duration::from_millis(*ms));
                Ok(Value::Unit)
            }
            TaskVal::Gather(tasks) => self.gather(tasks, depth),
        }
    }

    /// Runs the tasks at the same time, on at most 64 threads, and gives their values in order.
    /// When one fails, the first failure (by position) is the result.
    fn gather(&self, tasks: &[Value], depth: usize) -> Eval {
        let workers = tasks.len().min(64);
        let next = std::sync::atomic::AtomicUsize::new(0);
        let slots: Vec<Mutex<Option<Result<Value, RuntimeError>>>> =
            tasks.iter().map(|_| Mutex::new(None)).collect();
        std::thread::scope(|scope| {
            for _ in 0..workers {
                scope.spawn(|| {
                    loop {
                        let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let Some(task) = tasks.get(i) else { break };
                        let done = self.run_task(task, depth).map_err(Self::plain_error);
                        *slots[i].lock().unwrap_or_else(|e| e.into_inner()) = Some(done);
                    }
                });
            }
        });
        let mut values = Vec::with_capacity(tasks.len());
        for slot in slots {
            match slot.into_inner().unwrap_or_else(|e| e.into_inner()) {
                Some(Ok(v)) => values.push(v),
                Some(Err(e)) => return Err(Ctrl::Fail(e)),
                None => return fail("a task of `std.task.gather` did not run"),
            }
        }
        Ok(Value::vec(values))
    }

    /// `std.task.run(task)`: runs a task from code that is not async.
    /// Puts the interpreter in an `Arc`, so that `std.task.spawn` can run a task after the call that
    /// started it has returned. Without it `spawn` waits for the task.
    pub fn into_shared(self) -> Arc<Interp> {
        Arc::new_cyclic(|weak| {
            let _ = self.me.set(weak.clone());
            self
        })
    }

    /// Starts `task` on its own thread and returns. An error in it goes to the standard error.
    /// How many tasks started by `std.task.spawn` are still running.
    pub fn background_tasks(&self) -> usize {
        self.background.load(std::sync::atomic::Ordering::SeqCst)
    }

    pub(crate) fn spawn_task(
        &self,
        task: &Value,
        on_error: Option<&Value>,
    ) -> Result<Value, RuntimeError> {
        if !matches!(task, Value::Task(_)) {
            return Err(RuntimeError::new(format!(
                "`std.task.spawn` needs a task, found {}",
                task.type_name()
            )));
        }
        let Some(shared) = self.me.get().and_then(std::sync::Weak::upgrade) else {
            // Not behind an `Arc` (`telvax run`, a test): nothing outlives the call, so wait.
            return match (self.run_task_value(task), on_error) {
                (Err(e), Some(handler)) => self
                    .apply_at(handler, vec![Value::str(e.to_string())].into(), 0)
                    .map(|_| Value::Unit)
                    .map_err(Self::plain_error),
                (other, _) => other.map(|_| Value::Unit),
            };
        };
        use std::sync::atomic::Ordering::SeqCst;
        if self.background.fetch_add(1, SeqCst) >= MAX_BACKGROUND {
            self.background.fetch_sub(1, SeqCst);
            return Err(RuntimeError::new(format!(
                "too many background tasks ({MAX_BACKGROUND})"
            )));
        }
        let task = task.clone();
        let on_error = on_error.cloned();
        let spawned = std::thread::Builder::new()
            .name("telvax-task".into())
            .spawn(move || {
                if let Err(e) = shared.run_task_value(&task) {
                    match &on_error {
                        Some(handler) => {
                            let heard = shared
                                .apply_at(handler, vec![Value::str(e.to_string())].into(), 0)
                                .map_err(Interp::plain_error);
                            if let Err(h) = heard {
                                eprintln!(
                                    "background task failed: {e}; its on_error failed too: {h}"
                                );
                            }
                        }
                        None => eprintln!("background task failed: {e}"),
                    }
                }
                shared.background.fetch_sub(1, SeqCst);
            });
        match spawned {
            Ok(_) => Ok(Value::Unit),
            Err(e) => {
                self.background.fetch_sub(1, SeqCst);
                Err(RuntimeError::new(format!("cannot start a task: {e}")))
            }
        }
    }

    pub(crate) fn run_task_value(&self, task: &Value) -> Result<Value, RuntimeError> {
        self.run_task(task, 0).map_err(Self::plain_error)
    }

    /// Runs an `@endpoint` function on a JSON payload, returning the JSON response body.
    pub fn call_endpoint(&self, name: &str, payload: &Json) -> Result<Json, EndpointError> {
        self.call_endpoint_with(name, &std::collections::BTreeMap::new(), payload)
    }

    /// Like [`call_endpoint`](Self::call_endpoint), with the values of the `{name}` segments of the
    /// path: each is given to the parameter of that name, and the payload to the other one.
    pub fn call_endpoint_with(
        &self,
        name: &str,
        params: &std::collections::BTreeMap<String, Json>,
        payload: &Json,
    ) -> Result<Json, EndpointError> {
        self.call_endpoint_as(name, params, payload, &RequestInfo::default())
    }

    /// Like [`call_endpoint_with`](Self::call_endpoint_with), with the request: a parameter of type
    /// `Request` receives it.
    pub fn call_endpoint_as(
        &self,
        name: &str,
        params: &std::collections::BTreeMap<String, Json>,
        payload: &Json,
        request: &RequestInfo,
    ) -> Result<Json, EndpointError> {
        self.call_endpoint_reply(name, params, payload, request)
            .map(|r| r.body)
    }

    /// Like [`call_endpoint_as`](Self::call_endpoint_as), keeping the status and the headers of a
    /// `Response` that the function returned.
    pub fn call_endpoint_reply(
        &self,
        name: &str,
        params: &std::collections::BTreeMap<String, Json>,
        payload: &Json,
        request: &RequestInfo,
    ) -> Result<EndpointReply, EndpointError> {
        let internal = |m: String| EndpointError {
            status: 500,
            message: m,
        };
        let f = self
            .fns
            .get(name)
            .filter(|f| f.endpoint.is_some())
            .ok_or_else(|| internal(format!("no endpoint `{name}`")))?;
        let in_path = f.endpoint.as_ref().map(|e| &e.path_params);
        let mut args = Vec::with_capacity(f.params.len());
        for p in &f.params {
            if matches!(&p.ty, Ty::Named(n) if n == "Request") {
                args.push(Value::Native(Native::Request(Arc::new(request.clone()))));
                continue;
            }
            let source = if in_path.is_some_and(|names| names.contains(&p.name)) {
                params
                    .get(&p.name)
                    .ok_or_else(|| internal(format!("no value for path parameter `{}`", p.name)))?
            } else {
                payload
            };
            args.push(
                from_json(&p.ty, source, &self.structs, &self.enums).map_err(|e| {
                    EndpointError {
                        status: 400,
                        message: e,
                    }
                })?,
            );
        }
        let plain = |body: Json| EndpointReply {
            status: 200,
            headers: Vec::new(),
            body,
            events: None,
        };
        let reply = |v: Value| match v {
            Value::Native(Native::Response(r)) => match u16::try_from(r.status) {
                Ok(status) if (100..=599).contains(&status) => Ok(EndpointReply {
                    status,
                    headers: r.headers.clone(),
                    body: r.body.to_json(),
                    events: r.events.clone(),
                }),
                _ => Err(internal(format!(
                    "invalid status {}: it must be from 100 to 599",
                    r.status
                ))),
            },
            other => Ok(plain(other.to_json())),
        };
        match self.call(name, args) {
            Ok(Value::Ok(v)) => reply(*v),
            Ok(Value::Err(e)) => match *e {
                Value::Status(code, msg) => Err(EndpointError {
                    status: code,
                    message: msg.to_string(),
                }),
                other => Err(internal(other.to_string())),
            },
            Ok(other) => reply(other),
            Err(e) => Err(internal(format!("runtime error: {e}"))),
        }
    }

    /// Runs a `@middleware` function with the request and `next`, the rest of the chain, and gives the
    /// `Response` it returns. A failure of the function is a `500`.
    pub fn call_middleware(
        &self,
        name: &str,
        request: &RequestInfo,
        next: crate::value::NextFn,
    ) -> Result<crate::value::ResponseVal, EndpointError> {
        let internal = |m: String| EndpointError {
            status: 500,
            message: m,
        };
        let args = vec![
            Value::Native(Native::Request(Arc::new(request.clone()))),
            Value::Native(Native::Next(next)),
        ];
        match self.call(name, args) {
            Ok(Value::Native(Native::Response(r))) => match u16::try_from(r.status) {
                Ok(status) if (100..=599).contains(&status) => Ok((*r).clone()),
                _ => Err(internal(format!(
                    "invalid status {}: it must be from 100 to 599",
                    r.status
                ))),
            },
            Ok(other) => Err(internal(format!(
                "the middleware `{name}` gave {} instead of a Response",
                other.type_name()
            ))),
            Err(e) => Err(internal(format!("runtime error: {e}"))),
        }
    }

    /// Runs the producer of a `Response.events` answer. Every event goes to `send`, which says
    /// whether the client is still there; the error is the reason the stream ended early.
    pub fn run_events(
        &self,
        producer: &Value,
        send: crate::value::EventSink,
    ) -> Result<(), RuntimeError> {
        let sink = Value::Native(Native::Sink(crate::value::SinkVal(send)));
        self.apply(producer, vec![sink]).map(|_| ())
    }

    /// Applies a closure or function value (used by std higher-order functions).
    pub(crate) fn apply(&self, f: &Value, args: Vec<Value>) -> Result<Value, RuntimeError> {
        self.apply_at(f, args.into(), 0).map_err(|c| match c {
            Ctrl::Fail(e) => e,
            Ctrl::Return(v) => RuntimeError::new(format!("stray return of {v}")),
            Ctrl::Break | Ctrl::Continue => {
                RuntimeError::new("`break` or `continue` outside a loop")
            }
        })
    }

    fn apply_at(&self, f: &Value, args: Args, depth: usize) -> Eval {
        if let Value::Func(name) = f {
            let func = self
                .fns
                .get(&**name)
                .ok_or_else(|| Ctrl::Fail(RuntimeError::new(format!("no function `{name}`"))))?;
            return self.call_fn(func, args, depth);
        }
        if let Value::Native(Native::Next(next)) = f {
            let [Value::Native(Native::Request(request))] = args.as_slice() else {
                return fail("`next` takes the request");
            };
            return match (next.0)(request.clone()) {
                Ok(answer) => Ok(Value::Native(Native::Response(Arc::new(answer)))),
                Err(message) => fail(message),
            };
        }
        let Value::Closure(c) = f else {
            return fail(format!("{} is not callable", f.type_name()));
        };
        if c.params.len() != args.len() {
            return fail(format!(
                "closure takes {} argument(s), got {}",
                c.params.len(),
                args.len()
            ));
        }
        if depth >= MAX_DEPTH {
            return fail("stack overflow: call depth limit reached");
        }
        let mut ctx = Ctx {
            env: c.env.clone(),
            depth: depth + 1,
        };
        ctx.env.extend(c.params.iter().copied().zip(args));
        stacker::maybe_grow(STACK_RED_ZONE, STACK_SEGMENT, || {
            self.eval(&c.body, &mut ctx)
        })
    }

    /// A call. An `async fn` does not run: the call gives a task, and `await` runs it.
    fn call_fn(&self, f: &hir::FnDef, args: Args, depth: usize) -> Eval {
        if f.is_async {
            if args.len() != f.params.len() {
                return fail(format!(
                    "`{}` takes {} argument(s), got {}",
                    f.name,
                    f.params.len(),
                    args.len()
                ));
            }
            return Ok(Value::Task(Arc::new(TaskVal::Call {
                name: f.name.clone(),
                args: args.into_vec(),
            })));
        }
        self.run_fn(f, args, depth)
    }

    /// Runs the body of a function.
    fn run_fn(&self, f: &hir::FnDef, args: Args, depth: usize) -> Eval {
        if args.len() != f.params.len() {
            return fail(format!(
                "`{}` takes {} argument(s), got {}",
                f.name,
                f.params.len(),
                args.len()
            ));
        }
        let mut env = Env::with_capacity(f.params.len() + 8);
        env.extend(f.params.iter().map(|p| p.id).zip(args));
        self.run_body(f, env, depth)
    }

    /// Runs the body of a function whose parameters are already in `env`.
    fn run_body(&self, f: &hir::FnDef, env: Env, depth: usize) -> Eval {
        if depth >= MAX_DEPTH {
            return fail(format!(
                "stack overflow: call depth limit reached in `{}`",
                f.name
            ));
        }
        let mut ctx = Ctx {
            env,
            depth: depth + 1,
        };
        let _probe = self
            .profiling
            .load(std::sync::atomic::Ordering::Relaxed)
            .then(|| self.current_profiler())
            .flatten()
            .map(|p| crate::profile::Probe::enter(p, &f.name, false));
        let _call = self
            .debugging
            .load(std::sync::atomic::Ordering::Relaxed)
            .then(|| crate::debug::CallGuard::enter(&f.name));
        let result = stacker::maybe_grow(STACK_RED_ZONE, STACK_SEGMENT, || {
            self.block(&f.body, &mut ctx)
        });
        match result {
            Err(Ctrl::Return(v)) => Ok(v),
            other => other,
        }
    }

    /// Runs one loop iteration; `Ok(false)` means the body executed `break`.
    fn loop_body(&self, body: &hir::Block, ctx: &mut Ctx) -> Result<bool, Ctrl> {
        match self.block(body, ctx) {
            Ok(_) | Err(Ctrl::Continue) => Ok(true),
            Err(Ctrl::Break) => Ok(false),
            Err(other) => Err(other),
        }
    }

    fn block(&self, b: &hir::Block, ctx: &mut Ctx) -> Eval {
        for s in &b.stmts {
            self.point(telvax_hir::coverage::stmt_start(s), ctx);
            match s {
                hir::Stmt::Let { local, init } => {
                    let v = self.eval(init, ctx)?;
                    ctx.env.insert(local.id, v);
                }
                hir::Stmt::Expr(e) => {
                    self.eval(e, ctx)?;
                }
            }
        }
        match &b.tail {
            Some(t) => {
                self.point(t.span.start, ctx);
                self.eval(t, ctx)
            }
            None => Ok(Value::Unit),
        }
    }

    /// Assignment, loops and arrays. Kept out of `eval` so its stack frame stays small: the
    /// evaluator recurses once per Telvax call, and debug-build frames add up quickly.
    #[inline(never)]
    fn eval_mutation_and_loops(&self, e: &hir::Expr, ctx: &mut Ctx) -> Eval {
        match &e.kind {
            ExprKind::Assign { place, value } => {
                // Order: the value, then the place's index expressions left to right.
                let v = self.eval(value, ctx)?;
                let mut steps = Vec::with_capacity(place.projs.len());
                for pr in &place.projs {
                    steps.push(match pr {
                        hir::Proj::Field { name, .. } => Step::Field(name.as_str()),
                        hir::Proj::Index(i) => match self.eval(i, ctx)? {
                            Value::Int(i) => Step::Index(i),
                            other => Step::Key(
                                MapKey::from_value(&other)
                                    .map_err(|e| Ctrl::Fail(RuntimeError::new(e)))?,
                            ),
                        },
                    });
                }
                let root = ctx.env.get(&place.local).cloned().ok_or_else(|| {
                    Ctrl::Fail(RuntimeError::new(format!(
                        "unbound variable `{}`",
                        place.name
                    )))
                })?;
                let updated = update_place(root, &steps, v)?;
                ctx.env.insert(place.local, updated);
                Ok(Value::Unit)
            }
            ExprKind::While { cond, body } => {
                loop {
                    match self.eval(cond, ctx)? {
                        Value::Bool(true) => {
                            if !self.loop_body(body, ctx)? {
                                break;
                            }
                        }
                        Value::Bool(false) => break,
                        other => {
                            return fail(format!(
                                "`while` condition is {}, not bool",
                                other.type_name()
                            ));
                        }
                    }
                }
                Ok(Value::Unit)
            }
            ExprKind::For { var, iter, body } => {
                match iter {
                    hir::ForIter::Range {
                        start,
                        end,
                        inclusive,
                    } => {
                        let (Value::Int(a), Value::Int(b)) =
                            (self.eval(start, ctx)?, self.eval(end, ctx)?)
                        else {
                            return fail("range bounds must be integers");
                        };
                        // i128 so that `..=i64::MAX` cannot overflow the counter.
                        let (mut i, last) = (i128::from(a), i128::from(b));
                        while i < last || (*inclusive && i == last) {
                            ctx.env.insert(var.id, Value::Int(i as i64));
                            if !self.loop_body(body, ctx)? {
                                break;
                            }
                            i += 1;
                        }
                    }
                    hir::ForIter::Vec(v) => {
                        let items = match self.eval(v, ctx)? {
                            Value::Vec(items) => items,
                            Value::Table(t) => {
                                for i in 0..t.rows() {
                                    ctx.env.insert(var.id, t.row(i)?);
                                    if !self.loop_body(body, ctx)? {
                                        break;
                                    }
                                }
                                return Ok(Value::Unit);
                            }
                            _ => return fail("`for` needs a Vec or a Table"),
                        };
                        // Live view: element assignments inside the loop are visible to later
                        // iterations. The length is fixed when the loop starts.
                        for i in 0..items.len() {
                            let Some(item) = items.get(i) else { break };
                            ctx.env.insert(var.id, item);
                            if !self.loop_body(body, ctx)? {
                                break;
                            }
                        }
                    }
                }
                Ok(Value::Unit)
            }
            ExprKind::Index { base, index } => {
                // A name or a field of one is read in place: cloning a shared list or map to look
                // at one item makes every thread that reads it write to the same reference count.
                let owned;
                let (base, i) = if Self::is_place(base) && self.peek(base, ctx).is_some() {
                    let i = self.eval(index, ctx)?;
                    (
                        self.peek(base, ctx)
                            .ok_or_else(|| Ctrl::Fail(RuntimeError::new("unbound local")))?,
                        i,
                    )
                } else {
                    let b = self.eval(base, ctx)?;
                    let i = self.eval(index, ctx)?;
                    owned = b;
                    (&owned, i)
                };
                if let Value::Map(m) = base {
                    let key =
                        MapKey::from_value(&i).map_err(|e| Ctrl::Fail(RuntimeError::new(e)))?;
                    return m.get(&key).ok_or_else(|| {
                        Ctrl::Fail(RuntimeError::new(format!("key not found: {i}")))
                    });
                }
                let Value::Int(i) = i else {
                    return fail("indexing needs an integer index");
                };
                let out_of_bounds = |len: usize| {
                    Ctrl::Fail(RuntimeError::new(format!(
                        "index out of bounds: the length is {len} but the index is {i}"
                    )))
                };
                // A negative index counts from the end: `xs[-1]` is the last item.
                let from_end = |len: usize| {
                    if i < 0 {
                        usize::try_from(len as i64 + i).ok()
                    } else {
                        usize::try_from(i).ok()
                    }
                };
                match base {
                    Value::Vec(items) => from_end(items.len())
                        .and_then(|i| items.get(i))
                        .ok_or_else(|| out_of_bounds(items.len())),
                    Value::Str(s) => {
                        let len = s.chars().count();
                        from_end(len)
                            .and_then(|i| s.chars().nth(i))
                            .map(|c| Value::str(c.to_string()))
                            .ok_or_else(|| out_of_bounds(len))
                    }
                    Value::Table(t) => match usize::try_from(i) {
                        Ok(i) if i < t.rows() => Ok(t.row(i)?),
                        _ => Err(out_of_bounds(t.rows())),
                    },
                    Value::Tensor(t) => match (usize::try_from(i), t.shape.split_first()) {
                        (Ok(i), Some((&n, rest))) if i < n => Ok(t.element(i, rest)?),
                        (_, Some((&n, _))) => Err(out_of_bounds(n)),
                        _ => fail("cannot index a scalar tensor"),
                    },
                    _ => fail("indexing needs a List, a Text, a Map or a Table"),
                }
            }
            ExprKind::Array(items) => {
                let mut out = Vec::with_capacity(items.len());
                for it in items {
                    out.push(self.eval(it, ctx)?);
                }
                Ok(Value::vec(out))
            }

            _ => unreachable!("only called for mutation, loops and arrays"),
        }
    }

    /// `v` as text, knowing its type `ty`: an `f32` shows its own digits.
    pub(crate) fn show(&self, v: &Value, ty: &Ty, quoted: bool) -> String {
        let env = crate::show::TypeEnv {
            structs: Some(&self.structs),
            enums: Some(&self.enums),
        };
        crate::show::show(v, Some(ty), &env, quoted)
    }

    fn eval(&self, e: &hir::Expr, ctx: &mut Ctx) -> Eval {
        use std::sync::atomic::Ordering::Relaxed;
        let fuel = self.fuel.load(Relaxed);
        if fuel != u64::MAX {
            if fuel == 0 {
                return fail("execution step limit exceeded");
            }
            self.fuel.store(fuel - 1, Relaxed);
        }
        match &e.kind {
            ExprKind::Error => fail("cannot run code that failed to type-check"),
            ExprKind::Assign { .. }
            | ExprKind::While { .. }
            | ExprKind::For { .. }
            | ExprKind::Index { .. }
            | ExprKind::Array(_) => self.eval_mutation_and_loops(e, ctx),
            ExprKind::Lit(l) => Ok(match l {
                Lit::Int(n) => match &e.ty {
                    Ty::Prim(p) if p.is_float() => round_to(&e.ty, Value::Float(*n as f64)),
                    _ => Value::Int(
                        i64::try_from(*n)
                            .map_err(|_| RuntimeError::new("integer literal too large"))?,
                    ),
                },
                Lit::Float(x) => round_to(&e.ty, Value::Float(*x)),
                Lit::Str(s) => Value::str(s.as_str()),
                Lit::Bool(b) => Value::Bool(*b),
            }),
            ExprKind::Local(id) => ctx
                .env
                .get(id)
                .cloned()
                .ok_or_else(|| Ctrl::Fail(RuntimeError::new("unbound local"))),
            ExprKind::Global(name) => match self.frozen_globals.get() {
                Some(all) => all.get(name).cloned(),
                None => self
                    .globals
                    .read()
                    .unwrap_or_else(|e| e.into_inner())
                    .get(name)
                    .cloned(),
            }
            .ok_or_else(|| {
                Ctrl::Fail(RuntimeError::new(format!(
                    "global `{name}` is not initialised yet"
                )))
            }),
            ExprKind::Fn(name) => Ok(Value::Func(name.as_str().into())),
            ExprKind::Extern(path) => {
                fail(format!("`{}` cannot be used as a value", path.join("::")))
            }
            ExprKind::FString(parts) => {
                let mut out = String::new();
                for p in parts {
                    match p {
                        hir::FPart::Text(t) => out.push_str(t),
                        hir::FPart::Expr(x) => {
                            let v = self.eval(x, ctx)?;
                            out.push_str(&self.show(&v, &x.ty, false));
                        }
                        hir::FPart::Fmt { expr, repr, spec } => {
                            let v = self.eval(expr, ctx)?;
                            let shown = self.show(&v, &expr.ty, *repr);
                            match spec {
                                None => out.push_str(&shown),
                                Some(spec) => {
                                    let spec = self.eval(spec, ctx)?;
                                    let Value::Str(spec) = spec else {
                                        return fail("a format is a Text");
                                    };
                                    out.push_str(&std_impl::format_value(&v, &shown, &spec)?);
                                }
                            }
                        }
                    }
                }
                Ok(Value::str(out))
            }
            ExprKind::Field { base, name }
                if Self::is_place(base) && self.peek(base, ctx).is_some() =>
            {
                match self.peek(base, ctx) {
                    Some(Value::Struct(s)) => s
                        .fields
                        .iter()
                        .find(|(n, _)| n == name)
                        .map(|(_, v)| v.clone())
                        .ok_or_else(|| {
                            Ctrl::Fail(RuntimeError::new(format!(
                                "{} has no field `{name}`",
                                s.name
                            )))
                        }),
                    Some(Value::Table(t)) => Ok(t.column(name)?),
                    Some(Value::Tuple(items)) => name
                        .parse::<usize>()
                        .ok()
                        .and_then(|i| items.get(i).cloned())
                        .ok_or_else(|| {
                            Ctrl::Fail(RuntimeError::new(format!("tuple has no part `{name}`")))
                        }),
                    Some(other) => fail(format!("{} has no field `{name}`", other.type_name())),
                    None => Err(Ctrl::Fail(RuntimeError::new("unbound local"))),
                }
            }
            ExprKind::Field { base, name } => match self.eval(base, ctx)? {
                Value::Table(t) => Ok(t.column(name)?),
                Value::Struct(s) => s
                    .fields
                    .iter()
                    .find(|(n, _)| n == name)
                    .map(|(_, v)| v.clone())
                    .ok_or_else(|| {
                        Ctrl::Fail(RuntimeError::new(format!(
                            "{} has no field `{name}`",
                            s.name
                        )))
                    }),
                Value::Tuple(items) => name
                    .parse::<usize>()
                    .ok()
                    .and_then(|i| items.get(i).cloned())
                    .ok_or_else(|| {
                        Ctrl::Fail(RuntimeError::new(format!("tuple has no part `{name}`")))
                    }),
                other => fail(format!("{} has no field `{name}`", other.type_name())),
            },
            ExprKind::DynCall {
                trait_name,
                method,
                args,
            } => {
                let mut values = Vec::with_capacity(args.len());
                for a in args {
                    values.push(self.eval(a, ctx)?);
                }
                let type_name = match values.first() {
                    Some(Value::Struct(s)) => s.name.clone(),
                    Some(Value::Enum(e)) => e.name.to_string(),
                    Some(other) => {
                        return fail(format!(
                            "{}.{method}(): a {} does not implement `{trait_name}`",
                            other.type_name(),
                            other.type_name()
                        ));
                    }
                    None => return fail("a trait method needs a receiver"),
                };
                // The type's own method, else the default one the trait provides.
                let f = self
                    .fns
                    .get(&format!("{type_name}::{method}"))
                    .or_else(|| self.fns.get(&format!("{trait_name}::{method}")))
                    .ok_or_else(|| {
                        Ctrl::Fail(RuntimeError::new(format!(
                            "`{type_name}` has no method `{method}` for the trait `{trait_name}`"
                        )))
                    })?;
                self.call_fn(f, values.into(), ctx.depth)
            }
            ExprKind::Tuple(items) => {
                let mut values = Vec::with_capacity(items.len());
                for item in items {
                    values.push(self.eval(item, ctx)?);
                }
                Ok(Value::Tuple(Arc::new(values)))
            }
            ExprKind::Call { callee, args } => self.call_expr(callee, args, &e.ty, ctx),
            ExprKind::Unary { op, expr } => match (op, self.eval(expr, ctx)?) {
                (UnOp::Neg, Value::Int(i)) => i.checked_neg().map(Value::Int).ok_or_else(overflow),
                (UnOp::Neg, Value::Float(x)) => Ok(Value::Float(-x)),
                (UnOp::Not, Value::Bool(b)) => Ok(Value::Bool(!b)),
                (op, v) => fail(format!("cannot apply {op:?} to {}", v.type_name())),
            },
            ExprKind::Binary { op, lhs, rhs } => self.binary(*op, lhs, rhs, &e.ty, ctx),
            ExprKind::Closure { params, body } => Ok(Value::Closure(Arc::new(Closure {
                params: params.iter().map(|p| p.id).collect::<Vec<LocalId>>(),
                body: (**body).clone(),
                env: ctx.env.clone(),
            }))),
            ExprKind::StructLit { name, fields } => {
                let decl = self.structs.get(name).ok_or_else(|| {
                    Ctrl::Fail(RuntimeError::new(format!("unknown type `{name}`")))
                })?;
                let mut evaluated: HashMap<&str, Value> = HashMap::new();
                for (fname, fe) in fields {
                    evaluated.insert(fname.as_str(), self.eval(fe, ctx)?);
                }
                // Keep declaration order so JSON output is stable.
                let ordered = decl
                    .iter()
                    .map(|(n, _)| {
                        evaluated
                            .remove(n.as_str())
                            .map(|v| (n.clone(), v))
                            .ok_or_else(|| {
                                Ctrl::Fail(RuntimeError::new(format!("missing field `{n}`")))
                            })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Value::Struct(Arc::new(StructVal {
                    name: name.clone(),
                    fields: ordered,
                })))
            }
            ExprKind::If {
                cond,
                then,
                otherwise,
            } => match self.eval(cond, ctx)? {
                Value::Bool(true) => self.block(then, ctx),
                Value::Bool(false) => match otherwise {
                    Some(o) => self.eval(o, ctx),
                    None => Ok(Value::Unit),
                },
                other => fail(format!("`if` condition is {}, not bool", other.type_name())),
            },
            ExprKind::Block(b) => self.block(b, ctx),
            ExprKind::Return(v) => {
                let v = match v {
                    Some(v) => self.eval(v, ctx)?,
                    None => Value::Unit,
                };
                Err(Ctrl::Return(v))
            }
            ExprKind::EnumCtor {
                enum_name,
                variant,
                args,
            } => {
                let mut fields = Vec::with_capacity(args.len());
                for a in args {
                    fields.push(self.eval(a, ctx)?);
                }
                let index = self
                    .enums
                    .get(enum_name)
                    .and_then(|vs| vs.iter().position(|(v, _)| v == variant))
                    .unwrap_or(0);
                Ok(Value::Enum(Arc::new(crate::value::EnumVal {
                    name: enum_name.clone().into(),
                    variant: variant.clone().into(),
                    index: index as u32,
                    fields: fields.into(),
                })))
            }
            ExprKind::Match { scrutinee, arms } => {
                let value = self.eval(scrutinee, ctx)?;
                for arm in arms {
                    if !match_pattern(&arm.pat, &value, &mut ctx.env) {
                        continue;
                    }
                    if let Some(guard) = &arm.guard {
                        match self.eval(guard, ctx)? {
                            Value::Bool(true) => {}
                            Value::Bool(false) => continue,
                            other => {
                                return fail(format!(
                                    "the `if` of a `case` gave {}, not a Bool",
                                    other.type_name()
                                ));
                            }
                        }
                    }
                    self.point(arm.body.span.start, ctx);
                    return self.eval(&arm.body, ctx);
                }
                fail("no `match` arm matched")
            }
            ExprKind::Await(inner) => {
                let task = self.eval(inner, ctx)?;
                self.run_task(&task, ctx.depth)
            }
            ExprKind::Try(inner) => match self.eval(inner, ctx)? {
                Value::Ok(v) => Ok(*v),
                Value::Err(e) => Err(Ctrl::Return(Value::Err(e))),
                Value::Enum(e) if e.name == "Option" => match e.fields.first() {
                    Some(v) => Ok(v.clone()),
                    None => Err(Ctrl::Return(Value::none())),
                },
                other => fail(format!(
                    "`?` needs a Result or an Option, found {}",
                    other.type_name()
                )),
            },
            ExprKind::Break => Err(Ctrl::Break),
            ExprKind::Continue => Err(Ctrl::Continue),
        }
    }

    /// Whether `e` is a name, or a field of one: something `peek` can look at without running anything.
    fn is_place(e: &hir::Expr) -> bool {
        match &e.kind {
            ExprKind::Local(_) | ExprKind::Global(_) => true,
            ExprKind::Field { base, .. } => Self::is_place(base),
            _ => false,
        }
    }

    /// The value a place holds, without copying it.
    fn peek<'a>(&'a self, e: &hir::Expr, ctx: &'a Ctx) -> Option<&'a Value> {
        match &e.kind {
            ExprKind::Local(id) => ctx.env.get(id),
            ExprKind::Global(name) => match self.frozen_globals.get() {
                Some(all) => all.get(name),
                None => None,
            },
            ExprKind::Field { base, name } => match self.peek(base, ctx)? {
                Value::Struct(s) => s.fields.iter().find(|(n, _)| n == name).map(|(_, v)| v),
                _ => None,
            },
            _ => None,
        }
    }

    fn call_expr(
        &self,
        callee: &hir::Expr,
        args: &[hir::Expr],
        result: &Ty,
        ctx: &mut Ctx,
    ) -> Eval {
        // Evaluate arguments left to right, before dispatch.
        let eval_args = |this: &Self, ctx: &mut Ctx| {
            // Sized up front: collecting through `Result` starts empty and grows.
            let mut values = crate::value::Args::new();
            for a in args {
                values.push(this.eval(a, ctx)?);
            }
            Ok::<_, Ctrl>(values)
        };

        match &callee.kind {
            ExprKind::Fn(name) => {
                // A plain call: the arguments go straight into the locals of the callee, with no list
                // in between.
                if let Some(f) = self.fns.get(name)
                    && !f.is_async
                    && f.params.len() == args.len()
                    && !self.is_mocked(name)
                {
                    let mut env = Env::with_capacity(f.params.len() + 8);
                    for (param, arg) in f.params.iter().zip(args) {
                        env.push(param.id, self.eval(arg, ctx)?);
                    }
                    return self.run_body(f, env, ctx.depth);
                }
                let args = eval_args(self, ctx)?;
                if let Some(mock) = self.mock_for(name) {
                    return self.apply_at(&mock, args, ctx.depth);
                }
                let f = self.fns.get(name).ok_or_else(|| {
                    Ctrl::Fail(RuntimeError::new(format!("no function `{name}`")))
                })?;
                self.call_fn(f, args, ctx.depth)
            }
            ExprKind::Extern(path) => {
                #[cfg(feature = "gpu")]
                if let Some(v) = self.gpu_chain(path, args, result, ctx)? {
                    return Ok(v);
                }
                let mut args = eval_args(self, ctx)?;
                match path.as_slice() {
                    [n] if n == "Ok" || n == "Err" => {
                        let v = Box::new(args.remove(0));
                        Ok(if n == "Ok" {
                            Value::Ok(v)
                        } else {
                            Value::Err(v)
                        })
                    }
                    _ => {
                        let path = path.join("::");
                        if let Some(mock) = self.mock_for(&path) {
                            return self.apply_at(&mock, args, ctx.depth);
                        }
                        match self
                            .profiling
                            .load(std::sync::atomic::Ordering::Relaxed)
                            .then(|| self.current_profiler())
                            .flatten()
                        {
                            Some(profiler) => {
                                Ok(self.profiled_call(&profiler, &path, args, result)?)
                            }
                            None => Ok(std_impl::call_function(self, &path, args, result)?),
                        }
                    }
                }
            }
            ExprKind::Field { base, name }
                if Self::is_place(base) && self.peek(base, ctx).is_some() =>
            {
                // The receiver is a name or a field of one: it is read in place, after the
                // arguments, which cannot change what a name stands for.
                let args = eval_args(self, ctx)?;
                let Some(receiver) = self.peek(base, ctx) else {
                    return fail("unbound local");
                };
                if let Value::Struct(s) = receiver
                    && let Some((_, f)) = s.fields.iter().find(|(n, _)| n == name)
                {
                    let f = f.clone();
                    return self.apply_at(&f, args, ctx.depth);
                }
                if let Value::Native(Native::Postgres(pg)) = receiver {
                    let pg = pg.clone();
                    return Ok(std_impl::postgres_method(self, &pg, name, &args, result)?);
                }
                Ok(std_impl::call_method(receiver, name, args)?)
            }
            ExprKind::Field { base, name } => {
                let receiver = self.eval(base, ctx)?;
                let args = eval_args(self, ctx)?;
                // A field that holds a function: `handler.run(5)`.
                if let Value::Struct(s) = &receiver
                    && let Some((_, f)) = s.fields.iter().find(|(n, _)| n == name)
                {
                    return self.apply_at(f, args, ctx.depth);
                }
                if let Value::Native(Native::Postgres(pg)) = &receiver {
                    return Ok(std_impl::postgres_method(self, pg, name, &args, result)?);
                }
                Ok(std_impl::call_method(&receiver, name, args)?)
            }
            _ => {
                let f = self.eval(callee, ctx)?;
                let args = eval_args(self, ctx)?;
                self.apply_at(&f, args, ctx.depth)
            }
        }
    }

    #[inline(never)]
    fn binary(&self, op: BinOp, lhs: &hir::Expr, rhs: &hir::Expr, ty: &Ty, ctx: &mut Ctx) -> Eval {
        self.binary_exact(op, lhs, rhs, ctx)
            .map(|v| round_to(ty, v))
    }

    fn binary_exact(&self, op: BinOp, lhs: &hir::Expr, rhs: &hir::Expr, ctx: &mut Ctx) -> Eval {
        let l = self.eval(lhs, ctx)?;
        // `&&` and `||` short-circuit.
        match (op, &l) {
            (BinOp::And, Value::Bool(false)) => return Ok(Value::Bool(false)),
            (BinOp::Or, Value::Bool(true)) => return Ok(Value::Bool(true)),
            _ => {}
        }
        let r = self.eval(rhs, ctx)?;
        use BinOp::*;
        match (op, &l, &r) {
            (And | Or, Value::Bool(_), Value::Bool(b)) => Ok(Value::Bool(*b)),
            (Eq, a, b) => Ok(Value::Bool(a.equals(b))),
            (NotEq, a, b) => Ok(Value::Bool(!a.equals(b))),
            (Add, Value::Str(a), Value::Str(b)) => Ok(Value::concat(a, b)),
            (_, Value::Int(a), Value::Int(b)) => int_op(op, *a, *b),
            (_, Value::Float(a), Value::Float(b)) => float_op(op, *a, *b),
            (_, Value::Int(a), Value::Float(b)) => float_op(op, *a as f64, *b),
            (_, Value::Float(a), Value::Int(b)) => float_op(op, *a, *b as f64),
            (Lt | LtEq | Gt | GtEq, Value::Str(a), Value::Str(b)) => Ok(Value::Bool(match op {
                Lt => a < b,
                LtEq => a <= b,
                Gt => a > b,
                _ => a >= b,
            })),
            // Variants without values are ordered as the enum declares them.
            (Lt | LtEq | Gt | GtEq, Value::Enum(a), Value::Enum(b)) if a.name == b.name => {
                let (x, y) = (a.index, b.index);
                Ok(Value::Bool(match op {
                    Lt => x < y,
                    LtEq => x <= y,
                    Gt => x > y,
                    _ => x >= y,
                }))
            }
            _ => fail(format!(
                "cannot apply {op:?} to {} and {}",
                l.type_name(),
                r.type_name()
            )),
        }
    }
}

/// One resolved step of an assignment target.
enum Step<'a> {
    Field(&'a str),
    Index(i64),
    /// A key of a `Map` that is not an integer (an integer index also reaches a `Map` that is
    /// keyed by integers: see `update_place`).
    Key(MapKey),
}

/// Returns `current` with the place described by `steps` replaced by `new`. Structs are values
/// (rebuilt on the way back up); vectors are shared, so their elements are updated in place.
fn update_place(current: Value, steps: &[Step], new: Value) -> Result<Value, Ctrl> {
    let Some((step, rest)) = steps.split_first() else {
        return Ok(new);
    };
    match (step, current) {
        (Step::Field(name), Value::Struct(s)) => {
            let mut fields = s.fields.clone();
            let slot = fields.iter_mut().find(|(n, _)| n == name).ok_or_else(|| {
                Ctrl::Fail(RuntimeError::new(format!(
                    "{} has no field `{name}`",
                    s.name
                )))
            })?;
            let old = std::mem::replace(&mut slot.1, Value::Unit);
            slot.1 = update_place(old, rest, new)?;
            Ok(Value::Struct(Arc::new(StructVal {
                name: s.name.clone(),
                fields,
            })))
        }
        (Step::Index(i), Value::Vec(items)) => {
            let out_of_bounds = || {
                Ctrl::Fail(RuntimeError::new(format!(
                    "index out of bounds: the length is {} but the index is {i}",
                    items.len()
                )))
            };
            let idx = if *i < 0 {
                usize::try_from(items.len() as i64 + *i)
            } else {
                usize::try_from(*i)
            }
            .map_err(|_| out_of_bounds())?;
            let old = items.get(idx).ok_or_else(out_of_bounds)?;
            let updated = update_place(old, rest, new)?;
            items.set(idx, updated);
            Ok(Value::Vec(items))
        }
        (Step::Index(i), Value::Map(m)) => update_entry(m, MapKey::Int(*i), rest, new),
        (Step::Key(k), Value::Map(m)) => update_entry(m, k.clone(), rest, new),
        (_, other) => fail(format!("cannot assign through a {}", other.type_name())),
    }
}

/// `m[key] = new`, or `m[key].field = new`: a `Map` is shared, so the entry is updated in place.
fn update_entry(m: Arc<MapVal>, key: MapKey, rest: &[Step], new: Value) -> Result<Value, Ctrl> {
    let updated = if rest.is_empty() {
        new
    } else {
        let old = m.get(&key).ok_or_else(|| {
            Ctrl::Fail(RuntimeError::new(format!(
                "key not found: {}",
                key.to_value()
            )))
        })?;
        update_place(old, rest, new)?
    };
    m.insert(key, updated);
    Ok(Value::Map(m))
}

/// `f32` values are stored as `f64`; rounding after every operation keeps `f32` arithmetic exactly
/// what native code computes.
fn round_to(ty: &Ty, v: Value) -> Value {
    match (ty, &v) {
        (Ty::Prim(telvax_hir::Prim::F32), Value::Float(x)) => Value::Float(f64::from(*x as f32)),
        _ => v,
    }
}

fn overflow() -> Ctrl {
    Ctrl::Fail(RuntimeError::new("integer overflow"))
}

fn int_op(op: BinOp, a: i64, b: i64) -> Eval {
    use BinOp::*;
    Ok(match op {
        Add => Value::Int(a.checked_add(b).ok_or_else(overflow)?),
        Sub => Value::Int(a.checked_sub(b).ok_or_else(overflow)?),
        Mul => Value::Int(a.checked_mul(b).ok_or_else(overflow)?),
        Div | Rem if b == 0 => return fail("division by zero"),
        Div => Value::Int(a.checked_div(b).ok_or_else(overflow)?),
        Rem => Value::Int(a.checked_rem(b).ok_or_else(overflow)?),
        Lt => Value::Bool(a < b),
        LtEq => Value::Bool(a <= b),
        Gt => Value::Bool(a > b),
        GtEq => Value::Bool(a >= b),
        _ => return fail("invalid integer operation"),
    })
}

fn float_op(op: BinOp, a: f64, b: f64) -> Eval {
    use BinOp::*;
    Ok(match op {
        Add => Value::Float(a + b),
        Sub => Value::Float(a - b),
        Mul => Value::Float(a * b),
        Div => Value::Float(a / b),
        Rem => Value::Float(a % b),
        Lt => Value::Bool(a < b),
        LtEq => Value::Bool(a <= b),
        Gt => Value::Bool(a > b),
        GtEq => Value::Bool(a >= b),
        _ => return fail("invalid float operation"),
    })
}

/// Matches `value` against `pat`, binding names in `env`. Bindings made before a later part of
/// the pattern fails are left behind; the arm that follows rebinds what it uses.
fn match_pattern(pat: &hir::Pat, value: &Value, env: &mut Env) -> bool {
    match (pat, value) {
        (hir::Pat::Wild, _) => true,
        (hir::Pat::Bind(local), v) => {
            env.insert(local.id, v.clone());
            true
        }
        (hir::Pat::Tuple(parts), Value::Tuple(items)) => {
            parts.len() == items.len()
                && parts
                    .iter()
                    .zip(items.iter())
                    .all(|(p, v)| match_pattern(p, v, env))
        }
        (hir::Pat::NegInt(a), Value::Int(b)) => -i128::from(*a) == i128::from(*b),
        (hir::Pat::NegInt(a), Value::Float(b)) => -(*a as f64) == *b,
        (hir::Pat::Or(alts), v) => alts.iter().any(|alt| match_pattern(alt, v, env)),
        (
            hir::Pat::List {
                before,
                rest,
                after,
            },
            Value::Vec(items),
        ) => {
            let items = items.snapshot();
            let fixed = before.len() + after.len();
            if items.len() < fixed || (rest.is_none() && items.len() != fixed) {
                return false;
            }
            let (head, tail) = (&items[..before.len()], &items[items.len() - after.len()..]);
            let middle = &items[before.len()..items.len() - after.len()];
            before
                .iter()
                .zip(head)
                .all(|(p, v)| match_pattern(p, v, env))
                && after
                    .iter()
                    .zip(tail)
                    .all(|(p, v)| match_pattern(p, v, env))
                && rest
                    .as_deref()
                    .is_none_or(|p| match_pattern(p, &Value::vec(middle.to_vec()), env))
        }
        (hir::Pat::Struct { name, fields }, Value::Struct(s)) => {
            s.name == *name
                && fields.iter().all(|(field, p)| {
                    s.fields
                        .iter()
                        .find(|(n, _)| n == field)
                        .is_some_and(|(_, v)| match_pattern(p, v, env))
                })
        }
        (hir::Pat::Lit(l), v) => match (l, v) {
            (Lit::Int(a), Value::Int(b)) => i128::from(*b) == i128::from(*a),
            (Lit::Float(a), Value::Float(b)) => a == b,
            (Lit::Int(a), Value::Float(b)) => *a as f64 == *b,
            (Lit::Str(a), Value::Str(b)) => a.as_str() == &**b,
            (Lit::Bool(a), Value::Bool(b)) => a == b,
            _ => false,
        },
        (
            hir::Pat::Variant {
                enum_name: None,
                variant,
                args,
            },
            Value::Ok(v),
        ) if variant == "Ok" => args.first().is_some_and(|p| match_pattern(p, v, env)),
        (
            hir::Pat::Variant {
                enum_name: None,
                variant,
                args,
            },
            Value::Err(v),
        ) if variant == "Err" => args.first().is_some_and(|p| match_pattern(p, v, env)),
        (
            hir::Pat::Variant {
                enum_name: Some(en),
                variant,
                args,
            },
            Value::Enum(e),
        ) => {
            e.name == *en
                && e.variant == *variant
                && args.len() == e.fields.len()
                && args
                    .iter()
                    .zip(&e.fields)
                    .all(|(p, v)| match_pattern(p, v, env))
        }
        _ => false,
    }
}
