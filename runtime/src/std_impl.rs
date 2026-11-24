//! Runtime implementations of the std items declared in `telvax_hir::stdlib`.
//!
//! External services are replaced by deterministic in-process stand-ins (hashed bag-of-words
//! embeddings, an in-memory vector store, a canned LLM) so programs run without network access.

use std::sync::Arc;

use crate::interp::{Interp, RuntimeError};
use crate::value::{Db, MapKey, MapVal, Native, Query, StructVal, TensorVal, Value};
use telvax_hir::Ty;
use telvax_hir::stdlib::EMBEDDING_DIM;

type Res = Result<Value, RuntimeError>;

thread_local! {
    /// The patterns compiled so far on this thread, by source text: an endpoint compiles the same few
    /// on every request. One list per thread, so requests on every core do not queue on a lock.
    static PATTERNS: std::cell::RefCell<Vec<(String, std::rc::Rc<regex::Regex>)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// How many patterns are kept per thread; the oldest goes first.
const PATTERN_CACHE: usize = 64;

fn pattern(source: &str) -> Result<std::rc::Rc<regex::Regex>, RuntimeError> {
    PATTERNS.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some((_, re)) = cache.iter().find(|(s, _)| s == source) {
            return Ok(re.clone());
        }
        // Kept behind an `Rc`: cloning a `Regex` makes a new pool of search caches, and building
        // a cache costs more than the search it serves.
        let re = std::rc::Rc::new(
            regex::RegexBuilder::new(source)
                .size_limit(1 << 20)
                .build()
                .map_err(|e| RuntimeError::new(format!("invalid pattern `{source}`: {e}")))?,
        );
        if cache.len() >= PATTERN_CACHE {
            cache.remove(0);
        }
        cache.push((source.to_string(), re.clone()));
        Ok(re)
    })
}

/// The counters a program makes with `std.metrics.count`, in several tables: a thread uses the one
/// its id picks, so threads that count at the same time rarely meet on the same lock.
const COUNTER_SHARDS: usize = 16;

static COUNTERS: [std::sync::Mutex<std::collections::BTreeMap<String, u64>>; COUNTER_SHARDS] =
    [const { std::sync::Mutex::new(std::collections::BTreeMap::new()) }; COUNTER_SHARDS];

thread_local! {
    static COUNTER_SHARD: usize = {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        std::thread::current().id().hash(&mut h);
        h.finish() as usize % COUNTER_SHARDS
    };
}

/// How many different label sets one counter name may have: a label made from user input would
/// otherwise grow the memory without end.
const LABEL_SETS: usize = 1000;

static LABEL_SET_COUNT: std::sync::Mutex<
    std::collections::BTreeMap<String, std::collections::HashSet<String>>,
> = std::sync::Mutex::new(std::collections::BTreeMap::new());

/// Called the first time a thread meets a label set.
fn note_label_set(function: &str, name: &str, key: &str) -> Result<(), RuntimeError> {
    let mut sets = LABEL_SET_COUNT.lock().unwrap_or_else(|e| e.into_inner());
    let set = sets.entry(name.to_string()).or_default();
    if !set.contains(key) && set.len() >= LABEL_SETS {
        return err(format!(
            "std.metrics.{function}: `{name}` has {LABEL_SETS} label sets already; a label should not come from user input"
        ));
    }
    set.insert(key.to_string());
    Ok(())
}

/// What a metric name was first used for. A name is a counter or a gauge, not both, because
/// `/metrics` gives each name one `# TYPE`.
static METRIC_KINDS: std::sync::Mutex<std::collections::BTreeMap<String, bool>> =
    std::sync::Mutex::new(std::collections::BTreeMap::new());

/// The gauges a program sets with `std.metrics.set`, by the name with its labels. A gauge holds the
/// last value set, so there is one table and not one per thread.
static GAUGES: std::sync::Mutex<std::collections::BTreeMap<String, f64>> =
    std::sync::Mutex::new(std::collections::BTreeMap::new());

/// Takes `name` for a gauge (`gauge`) or a counter. Called the first time a thread meets a name.
fn claim_metric(function: &str, name: &str, gauge: bool) -> Result<(), RuntimeError> {
    let mut kinds = METRIC_KINDS.lock().unwrap_or_else(|e| e.into_inner());
    match kinds.get(name) {
        Some(&is_gauge) if is_gauge != gauge => err(format!(
            "std.metrics.{function}: `{name}` is a {}: use `std.metrics.{}`",
            if is_gauge { "gauge" } else { "counter" },
            if is_gauge { "set" } else { "count" },
        )),
        _ => {
            kinds.insert(name.to_string(), gauge);
            Ok(())
        }
    }
}

fn metric_name(s: &str, colon: bool) -> bool {
    let mut chars = s.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || (colon && c == ':'))
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || (colon && c == ':'))
}

/// `name{a="x",b="y"}`, the labels in key order and the values escaped; `name` when there are none.
/// The name is what `/metrics` lists, so it must be one that Prometheus reads.
fn metric_key(
    function: &str,
    noun: &str,
    name: &str,
    labels: &[(MapKey, Value)],
) -> Result<String, RuntimeError> {
    if !metric_name(name, true) {
        return err(format!(
            "std.metrics.{function}: `{name}` is not a {noun} name: use letters, digits, `_` and `:`, and do not start with a digit"
        ));
    }
    let mut parts = Vec::new();
    for (k, v) in labels {
        let (MapKey::Str(k), Value::Str(v)) = (k, v) else {
            return err(format!(
                "std.metrics.{function}: the labels are a `Map[Text, Text]`"
            ));
        };
        if !metric_name(k, false) {
            return err(format!(
                "std.metrics.{function}: `{k}` is not a label name: use letters, digits and `_`, and do not start with a digit"
            ));
        }
        let value = v
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n");
        parts.push(format!("{k}=\"{value}\""));
    }
    if parts.is_empty() {
        return Ok(name.to_string());
    }
    Ok(format!("{name}{{{}}}", parts.join(",")))
}

/// `(name, value)` of every counter made with `std.metrics.count`, by name.
pub fn counters() -> Vec<(String, u64)> {
    let mut total: std::collections::BTreeMap<String, u64> = std::collections::BTreeMap::new();
    for shard in &COUNTERS {
        for (k, v) in shard.lock().unwrap_or_else(|e| e.into_inner()).iter() {
            *total.entry(k.clone()).or_default() += v;
        }
    }
    total.into_iter().collect()
}

/// `(name, value)` of every gauge set with `std.metrics.set`, by name.
pub fn gauges() -> Vec<(String, f64)> {
    GAUGES
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .map(|(k, v)| (k.clone(), *v))
        .collect()
}

/// How `std.log` writes, set by `telvax serve` from the options of the server: JSON or text, and the
/// lowest level that is written (0 debug, 1 info, 2 warn, 3 error).
static LOG_JSON: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static LOG_LEVEL: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(1);

const LOG_LEVELS: [&str; 4] = ["debug", "info", "warn", "error"];

/// Sets the format and the lowest level (`0` debug to `3` error) of the lines of `std.log`.
pub fn set_log(json: bool, level: u8) {
    use std::sync::atomic::Ordering::Relaxed;
    LOG_JSON.store(json, Relaxed);
    LOG_LEVEL.store(level.min(3), Relaxed);
}

/// One line of `std.log`: `{"ts": .., "level": .., "message": .., fields..}` or `level message key=value`.
/// A field called `ts`, `level` or `message` is left out.
fn log_line(
    json: bool,
    level: &str,
    message: &str,
    fields: &[(String, String)],
    ts: &str,
) -> String {
    let fields = fields
        .iter()
        .filter(|(k, _)| !matches!(k.as_str(), "ts" | "level" | "message"));
    if json {
        let mut line = serde_json::Map::new();
        line.insert("ts".into(), ts.into());
        line.insert("level".into(), level.into());
        line.insert("message".into(), message.into());
        for (k, v) in fields {
            line.insert(k.clone(), v.clone().into());
        }
        return serde_json::Value::Object(line).to_string();
    }
    let mut line = format!("{level} {message}");
    for (k, v) in fields {
        line.push_str(&format!(" {k}={v}"));
    }
    line
}

/// The files named by `std.fs.embed`, read when the program was built.
static EMBEDDED: std::sync::RwLock<Vec<(String, Arc<str>)>> = std::sync::RwLock::new(Vec::new());

/// Sets the files `std.fs.embed` returns: `(path as written in the program, text)`.
pub fn set_embedded_files(files: Vec<(String, String)>) {
    let files = files.into_iter().map(|(p, t)| (p, Arc::from(t))).collect();
    *EMBEDDED.write().unwrap_or_else(|e| e.into_inner()) = files;
}

static PROGRAM_ARGS: std::sync::RwLock<Vec<String>> = std::sync::RwLock::new(Vec::new());

/// Sets what `std.env.args()` returns: the words after the function name in `telvax run`.
pub fn set_program_args(args: Vec<String>) {
    *PROGRAM_ARGS.write().unwrap_or_else(|e| e.into_inner()) = args;
}

/// The data of an event: a `Text` as it is, any other value as JSON.
fn event_text(v: &Value) -> String {
    match v {
        Value::Str(s) => s.to_string(),
        other => other.to_json().to_string(),
    }
}

/// One server-sent event: an optional `event:` line and one `data:` line per line of text.
fn sse_event(name: Option<&str>, data: &str) -> String {
    let mut out = String::new();
    if let Some(name) = name {
        out.push_str(&format!("event: {}\n", name.replace(['\r', '\n'], " ")));
    }
    for line in data.split('\n') {
        out.push_str(&format!("data: {}\n", line.trim_end_matches('\r')));
    }
    out.push('\n');
    out
}

fn sse_comment(text: &str) -> String {
    let mut out = String::new();
    for line in text.split('\n') {
        out.push_str(&format!(": {}\n", line.trim_end_matches('\r')));
    }
    out.push('\n');
    out
}

fn err<T>(msg: impl Into<String>) -> Result<T, RuntimeError> {
    Err(RuntimeError::new(msg))
}

fn want_str(v: &Value) -> Result<Arc<str>, RuntimeError> {
    match v {
        Value::Str(s) => Ok(s.clone()),
        other => err(format!("expected a Text, found {}", other.type_name())),
    }
}

fn want_vec(v: &Value) -> Result<Vec<Value>, RuntimeError> {
    match v {
        Value::Vec(items) => Ok(items.snapshot()),
        other => err(format!("expected a List, found {}", other.type_name())),
    }
}

/// A `List` of numbers as `f64`.
fn want_f64_list(v: &Value) -> Result<Vec<f64>, RuntimeError> {
    want_vec(v)?.iter().map(want_f64).collect()
}

fn want_usize(v: &Value) -> Result<usize, RuntimeError> {
    match v {
        Value::Int(i) if *i >= 0 => Ok(*i as usize),
        other => err(format!("expected a non-negative integer, found {other}")),
    }
}

/// Deterministic embedding: hashed bag of lowercase words, L2-normalised. Texts that share words
/// get a higher cosine similarity, which is enough to exercise retrieval pipelines offline.
pub fn embed_text(text: &str) -> Vec<f32> {
    let dim = EMBEDDING_DIM as usize;
    let mut v = vec![0f32; dim];
    for word in text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
    {
        let mut h: u64 = 0xcbf29ce484222325;
        for b in word.to_lowercase().bytes() {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x100000001b3);
        }
        let sign = if (h >> 63) & 1 == 0 { 1.0 } else { -1.0 };
        v[(h % dim as u64) as usize] += sign;
    }
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        v.iter_mut().for_each(|x| *x /= norm);
    }
    v
}

fn tensor(data: Vec<f32>) -> Value {
    Value::Tensor(Arc::new(TensorVal::from_f32(vec![data.len()], data)))
}

fn status(code: u16, args: &[Value]) -> Res {
    Ok(Value::Status(code, want_str(&args[0])?))
}

pub(crate) fn call_function(
    interp: &Interp,
    path: &str,
    args: crate::value::Args,
    result: &Ty,
) -> Res {
    interp.require(path)?;
    if path.starts_with("std::tensor::") {
        return crate::tensor::call(path, &args, result, interp.quantization(), interp.device());
    }
    if path.starts_with("std::table::") {
        return crate::table::call(interp, path, &args, result);
    }
    match path {
        "std::test::assert" => match &args[0] {
            Value::Bool(true) => Ok(Value::Unit),
            _ => err("assertion failed"),
        },
        "std::test::assert_msg" => match &args[0] {
            Value::Bool(true) => Ok(Value::Unit),
            _ => err(want_str(&args[1])?.to_string()),
        },
        "std::test::mock" => {
            let key = interp.mock_key(&args[0])?;
            interp.set_mock(key, args[1].clone());
            Ok(Value::Unit)
        }
        "std::test::calls" => {
            let key = interp.mock_key(&args[0])?;
            match interp.mock_calls(&key) {
                Some(n) => Ok(Value::Int(n)),
                None => err(format!(
                    "`{}` is not mocked: call `std.test.mock` first",
                    key.replace("::", ".")
                )),
            }
        }
        "std::test::reset_mocks" => {
            interp.reset_mocks();
            Ok(Value::Unit)
        }
        "std::test::fail" => err(want_str(&args[0])?.to_string()),
        "std::debug::dbg" => {
            eprintln!("{} = {}", want_str(&args[1])?, args[0]);
            Ok(args[0].clone())
        }
        "std::test::assert_eq" => {
            if args[0].equals(&args[1]) {
                Ok(Value::Unit)
            } else {
                err(format!(
                    "assertion failed: left is `{}`, right is `{}`",
                    args[0], args[1]
                ))
            }
        }
        "std::env::args" => Ok(Value::vec(
            PROGRAM_ARGS
                .read()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .map(|a| Value::str(a.clone()))
                .collect(),
        )),
        "std::process::exit" => {
            let code = want_i64(&args[0])?;
            if !(0..=255).contains(&code) {
                return err(format!(
                    "std.process.exit: the code is {code}; it has to be from 0 to 255"
                ));
            }
            use std::io::Write;
            let _ = std::io::stdout().flush();
            let _ = std::io::stderr().flush();
            std::process::exit(code as i32)
        }
        "std::env::get" => Ok(match std::env::var(&*want_str(&args[0])?) {
            Ok(v) => Value::some(Value::str(v)),
            Err(_) => Value::none(),
        }),
        "std::fs::read" => {
            let path = want_str(&args[0])?;
            Ok(match std::fs::read_to_string(&*path) {
                Ok(text) => Value::Ok(Box::new(Value::str(text))),
                Err(e) => Value::Err(Box::new(Value::str(format!("cannot read {path}: {e}")))),
            })
        }
        "std::fs::embed" => {
            let path = want_str(&args[0])?;
            let files = EMBEDDED.read().unwrap_or_else(|e| e.into_inner());
            match files.iter().find(|(p, _)| p.as_str() == &*path) {
                Some((_, text)) => Ok(Value::Str(text.clone())),
                None => err(format!(
                    "std.fs.embed: `{path}` was not included; run the program with the `telvax` command so it is read at build time"
                )),
            }
        }
        "std::fs::write" => {
            let path = want_str(&args[0])?;
            let text = want_str(&args[1])?;
            Ok(match std::fs::write(&*path, &*text) {
                Ok(()) => Value::Ok(Box::new(Value::Unit)),
                Err(e) => Value::Err(Box::new(Value::str(format!("cannot write {path}: {e}")))),
            })
        }
        "std::plot::bar" => {
            let labels = want_vec(&args[0])?
                .iter()
                .map(|v| want_str(v).map(|s| s.to_string()))
                .collect::<Result<Vec<_>, _>>()?;
            let values = want_f64_list(&args[1])?;
            crate::plot::bar(&labels, &values, &want_str(&args[2])?)
                .map(Value::str)
                .or_else(err)
        }
        "std::plot::line" => {
            let (xs, ys) = (want_f64_list(&args[0])?, want_f64_list(&args[1])?);
            crate::plot::line(&xs, &ys, &want_str(&args[2])?)
                .map(Value::str)
                .or_else(err)
        }
        "std::plot::histogram" => {
            let values = want_f64_list(&args[0])?;
            crate::plot::histogram(&values, want_i64(&args[1])?, &want_str(&args[2])?)
                .map(Value::str)
                .or_else(err)
        }

        "std::path::join" => {
            let (a, b) = (want_str(&args[0])?, want_str(&args[1])?);
            Ok(Value::str(
                std::path::Path::new(&*a)
                    .join(&*b)
                    .to_string_lossy()
                    .into_owned(),
            ))
        }
        "std::path::parent" => {
            let path = want_str(&args[0])?;
            Ok(optional_text(
                std::path::Path::new(&*path)
                    .parent()
                    .map(|p| p.to_string_lossy().into_owned()),
            ))
        }
        "std::path::file_name" => {
            let path = want_str(&args[0])?;
            Ok(optional_text(
                std::path::Path::new(&*path)
                    .file_name()
                    .map(|p| p.to_string_lossy().into_owned()),
            ))
        }
        "std::path::extension" => {
            let path = want_str(&args[0])?;
            Ok(optional_text(
                std::path::Path::new(&*path)
                    .extension()
                    .map(|p| p.to_string_lossy().into_owned()),
            ))
        }
        "std::path::relative" => {
            let (path, base) = (want_str(&args[0])?, want_str(&args[1])?);
            Ok(optional_text(
                std::path::Path::new(&*path)
                    .strip_prefix(&*base)
                    .ok()
                    .map(|p| p.to_string_lossy().into_owned()),
            ))
        }
        "std::fs::create_dir" => {
            let path = want_str(&args[0])?;
            Ok(match std::fs::create_dir_all(&*path) {
                Ok(()) => Value::Ok(Box::new(Value::Unit)),
                Err(e) => Value::Err(Box::new(Value::str(format!("cannot create {path}: {e}")))),
            })
        }
        "std::fs::remove" => {
            let path = want_str(&args[0])?;
            let target = std::path::Path::new(&*path);
            let removed = match std::fs::symlink_metadata(target) {
                Ok(meta) if meta.is_dir() => std::fs::remove_dir(target),
                Ok(_) => std::fs::remove_file(target),
                Err(e) => Err(e),
            };
            Ok(match removed {
                Ok(()) => Value::Ok(Box::new(Value::Unit)),
                Err(e) => Value::Err(Box::new(Value::str(format!("cannot remove {path}: {e}")))),
            })
        }
        "std::fs::copy" => {
            let from = want_str(&args[0])?;
            let to = want_str(&args[1])?;
            Ok(match std::fs::copy(&*from, &*to) {
                Ok(_) => Value::Ok(Box::new(Value::Unit)),
                Err(e) => Value::Err(Box::new(Value::str(format!(
                    "cannot copy {from} to {to}: {e}"
                )))),
            })
        }
        "std::fs::list_dir" => {
            let path = want_str(&args[0])?;
            Ok(match list_dir(&path) {
                Ok(names) => Value::Ok(Box::new(Value::vec(
                    names.into_iter().map(Value::str).collect(),
                ))),
                Err(e) => Value::Err(Box::new(Value::str(format!("cannot read {path}: {e}")))),
            })
        }
        "std::fs::walk" => {
            let path = want_str(&args[0])?;
            let mut files = Vec::new();
            Ok(
                match walk_dir(std::path::Path::new(&*path), &path, &mut files) {
                    Ok(()) => {
                        files.sort();
                        Value::Ok(Box::new(Value::vec(
                            files.into_iter().map(Value::str).collect(),
                        )))
                    }
                    Err(e) => Value::Err(Box::new(Value::str(format!("cannot read {path}: {e}")))),
                },
            )
        }
        "std::log::debug" | "std::log::info" | "std::log::warn" | "std::log::error" => {
            use std::sync::atomic::Ordering::Relaxed;
            let name = path.trim_start_matches("std::log::");
            let level = LOG_LEVELS.iter().position(|l| *l == name).unwrap_or(1);
            let message = want_str(&args[0])?;
            let mut fields = Vec::new();
            if let Some(Value::Map(m)) = args.get(1) {
                for (k, v) in m.entries() {
                    let (MapKey::Str(k), Value::Str(v)) = (k, v) else {
                        return err("std.log: the fields are a `Map[Text, Text]`");
                    };
                    fields.push((k.to_string(), v.to_string()));
                }
            }
            if level as u8 >= LOG_LEVEL.load(Relaxed) {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default();
                let ts = chrono::DateTime::from_timestamp(now.as_secs() as i64, now.subsec_nanos())
                    .map(|t| t.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
                    .unwrap_or_default();
                eprintln!(
                    "{}",
                    log_line(LOG_JSON.load(Relaxed), name, &message, &fields, &ts)
                );
            }
            Ok(Value::Unit)
        }
        "std::metrics::count" => {
            let name = want_str(&args[0])?;
            let (labels, by) = match (args.get(1), args.get(2)) {
                (Some(Value::Map(m)), by) => (Some(m), by),
                (by, None) => (None, by),
                _ => return err("std.metrics.count: expected labels, then `by`"),
            };
            let by = match by {
                Some(v) => u64::try_from(want_i64(v)?)
                    .or_else(|_| err("std.metrics.count: `by` cannot be negative"))?,
                None => 1,
            };
            let key = match labels {
                Some(m) => metric_key("count", "counter", &name, &m.entries())?,
                None => metric_key("count", "counter", &name, &[])?,
            };
            let shard = COUNTER_SHARD.with(|s| *s);
            let mut counters = COUNTERS[shard].lock().unwrap_or_else(|e| e.into_inner());
            match counters.get_mut(&key) {
                Some(n) => *n = n.saturating_add(by),
                None => {
                    claim_metric("count", &name, false)?;
                    if key.contains('{') {
                        note_label_set("count", &name, &key)?;
                    }
                    counters.insert(key, by);
                }
            }
            Ok(Value::Unit)
        }
        "std::metrics::set" => {
            let name = want_str(&args[0])?;
            let value = want_f64(&args[1])?;
            let key = match args.get(2) {
                Some(Value::Map(m)) => metric_key("set", "gauge", &name, &m.entries())?,
                Some(_) => return err("std.metrics.set: the labels are a `Map[Text, Text]`"),
                None => metric_key("set", "gauge", &name, &[])?,
            };
            let mut gauges = GAUGES.lock().unwrap_or_else(|e| e.into_inner());
            if !gauges.contains_key(&key) {
                claim_metric("set", &name, true)?;
                if key.contains('{') {
                    note_label_set("set", &name, &key)?;
                }
            }
            gauges.insert(key, value);
            Ok(Value::Unit)
        }
        "std::fs::exists" => Ok(Value::Bool(
            std::path::Path::new(&*want_str(&args[0])?).exists(),
        )),
        "std::json::stringify" => {
            let json = args[0].to_json();
            let indent = match args.get(1) {
                Some(n) => want_i64(n)?,
                None => 0,
            };
            if indent <= 0 {
                return Ok(Value::str(json.to_string()));
            }
            // serde_json indents by two spaces a level and escapes every new line inside a string, so the
            // spaces that begin a line are all structure: scale them to the width asked for (at most 16).
            let width = indent.min(16) as usize;
            let pretty = serde_json::to_string_pretty(&json)
                .map_err(|e| RuntimeError::new(format!("std.json.stringify: {e}")))?;
            let lines: Vec<String> = pretty
                .lines()
                .map(|line| {
                    let rest = line.trim_start_matches(' ');
                    let levels = (line.len() - rest.len()) / 2;
                    format!("{}{rest}", " ".repeat(levels * width))
                })
                .collect();
            Ok(Value::str(lines.join("\n")))
        }
        "std::json::parse" => {
            let Ty::Result(target, _) = result else {
                return err("`std.json.parse` needs its result type");
            };
            let text = want_str(&args[0])?;
            let parsed = serde_json::from_str::<serde_json::Value>(&text)
                .map_err(|e| format!("not valid JSON: {e}"))
                .and_then(|json| interp.value_from_json(target, &json));
            Ok(match parsed {
                Ok(v) => Value::Ok(Box::new(v)),
                Err(e) => Value::Err(Box::new(Value::str(e))),
            })
        }
        "std::text::clean_whitespace" => Ok(Value::str(
            want_str(&args[0])?
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" "),
        )),
        "std::text::trim" => Ok(Value::str(want_str(&args[0])?.trim())),
        "std::text::lowercase" => Ok(Value::str(want_str(&args[0])?.to_lowercase())),
        "std::text::uppercase" => Ok(Value::str(want_str(&args[0])?.to_uppercase())),
        "std::text::truncate" => {
            let n = want_usize(&args[1])?;
            Ok(Value::str(
                want_str(&args[0])?.chars().take(n).collect::<String>(),
            ))
        }
        "std::text::normalize" => {
            use unicode_normalization::UnicodeNormalization;
            let text = want_str(&args[0])?;
            let form = want_str(&args[1])?;
            Ok(Value::str(match &*form {
                "NFC" => text.nfc().collect::<String>(),
                "NFD" => text.nfd().collect::<String>(),
                "NFKC" => text.nfkc().collect::<String>(),
                "NFKD" => text.nfkd().collect::<String>(),
                other => {
                    return err(format!(
                        "unknown normalization form `{other}`: write \"NFC\", \"NFD\", \"NFKC\" or \"NFKD\""
                    ));
                }
            }))
        }
        "std::text::strip_accents" => {
            use unicode_normalization::{UnicodeNormalization, char::is_combining_mark};
            Ok(Value::str(
                want_str(&args[0])?
                    .nfd()
                    .filter(|c| !is_combining_mark(*c))
                    .collect::<String>(),
            ))
        }
        "std::text::matches" => {
            let re = pattern(&want_str(&args[1])?)?;
            Ok(Value::Bool(re.is_match(&want_str(&args[0])?)))
        }
        "std::text::find" => {
            let re = pattern(&want_str(&args[1])?)?;
            let text = want_str(&args[0])?;
            Ok(re
                .find(&text)
                .map_or_else(Value::none, |m| Value::some(Value::str(m.as_str()))))
        }
        "std::text::find_all" => {
            let re = pattern(&want_str(&args[1])?)?;
            let text = want_str(&args[0])?;
            Ok(Value::vec(
                re.find_iter(&text)
                    .map(|m| Value::str(m.as_str()))
                    .collect(),
            ))
        }
        "std::text::replace_all" => {
            let re = pattern(&want_str(&args[1])?)?;
            let text = want_str(&args[0])?;
            Ok(Value::str(
                re.replace_all(&text, &*want_str(&args[2])?).into_owned(),
            ))
        }
        "std::text::remove_chars" => {
            let text = want_str(&args[0])?;
            let drop = want_str(&args[1])?;
            Ok(Value::str(
                text.chars()
                    .filter(|c| !drop.contains(*c))
                    .collect::<String>(),
            ))
        }
        "std::text::split" => {
            let s = want_str(&args[0])?;
            let sep = want_str(&args[1])?;
            if sep.is_empty() {
                return err("`split` separator must not be empty");
            }
            Ok(Value::vec(s.split(&*sep).map(Value::str).collect()))
        }
        "std::string::join" => {
            let items = want_vec(&args[0])?;
            let sep = want_str(&args[1])?;
            Ok(Value::str(
                items
                    .iter()
                    .map(|v| v.to_string())
                    .collect::<Vec<_>>()
                    .join(&sep),
            ))
        }
        "std::iter::map" => {
            let items = want_vec(&args[0])?;
            let out = items
                .iter()
                .map(|v| interp.apply(&args[1], vec![v.clone()]))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Value::vec(out))
        }
        "std::dist::map" => dist_map(interp, &args),
        "std::iter::filter" => {
            let items = want_vec(&args[0])?;
            let mut out = Vec::new();
            for v in items.iter() {
                match interp.apply(&args[1], vec![v.clone()])? {
                    Value::Bool(true) => out.push(v.clone()),
                    Value::Bool(false) => {}
                    other => {
                        return err(format!("filter predicate returned {}", other.type_name()));
                    }
                }
            }
            Ok(Value::vec(out))
        }
        "std::iter::take" => {
            let n = want_usize(&args[1])?;
            Ok(Value::vec(
                want_vec(&args[0])?.iter().take(n).cloned().collect(),
            ))
        }
        "std::task::gather" => {
            let tasks = want_vec(&args[0])?;
            if let Some(bad) = tasks.iter().find(|t| !matches!(t, Value::Task(_))) {
                return err(format!(
                    "`std.task.gather` needs tasks, found {}",
                    bad.type_name()
                ));
            }
            Ok(Value::Task(Arc::new(crate::value::TaskVal::Gather(tasks))))
        }
        "std::task::sleep" => Ok(Value::Task(Arc::new(crate::value::TaskVal::Sleep(
            want_usize(&args[0])? as u64,
        )))),
        "std::task::run" => interp.run_task_value(&args[0]),
        "std::task::spawn" => interp.spawn_task(&args[0], args.get(1)),
        "std::time::sleep" => {
            std::thread::sleep(std::time::Duration::from_millis(
                want_usize(&args[0])? as u64
            ));
            Ok(Value::Unit)
        }
        "std::random::uuid" => {
            let mut b = [0u8; 16];
            random_fill(&mut b)?;
            b[6] = (b[6] & 0x0f) | 0x40;
            b[8] = (b[8] & 0x3f) | 0x80;
            Ok(Value::str(
                b.iter().map(|x| format!("{x:02x}")).collect::<String>(),
            ))
        }
        "std::random::int" => {
            let (low, high) = (want_i64(&args[0])?, want_i64(&args[1])?);
            if low > high {
                return err(format!(
                    "std.random.int: low ({low}) is above high ({high})"
                ));
            }
            let span = (i128::from(high) - i128::from(low) + 1) as u128;
            // Draws that fall in the incomplete last block are thrown away, so every number is as likely.
            let limit = u128::from(u64::MAX) + 1;
            let zone = limit - limit % span;
            loop {
                let mut b = [0u8; 8];
                random_fill(&mut b)?;
                let x = u128::from(u64::from_le_bytes(b));
                if x < zone || span > limit {
                    return Ok(Value::Int((i128::from(low) + (x % span) as i128) as i64));
                }
            }
        }
        "std::cache::new" => {
            let max = want_i64(&args[0])?;
            let ttl = match &args[1] {
                Value::Int(n) => *n as f64,
                Value::Float(x) => *x,
                other => {
                    return err(format!(
                        "expected the ttl in seconds, found {}",
                        other.type_name()
                    ));
                }
            };
            if max <= 0 || ttl <= 0.0 || !ttl.is_finite() {
                return err("std.cache.new: `max` and `ttl_seconds` must be above 0");
            }
            Ok(Value::Native(Native::Cache(Arc::new(
                crate::value::CacheVal::new(
                    usize::try_from(max).unwrap_or(usize::MAX),
                    std::time::Duration::from_secs_f64(ttl.min(31_536_000_000.0)),
                ),
            ))))
        }
        "std::time::format" => {
            let (t, pattern) = (want_f64(&args[0])?, want_str(&args[1])?);
            time_format(t, &pattern).map(Value::str).or_else(err)
        }
        "std::time::parse" => {
            let (text, pattern) = (want_str(&args[0])?, want_str(&args[1])?);
            Ok(match time_parse(&text, &pattern) {
                Ok(t) => Value::Ok(Box::new(Value::Float(t))),
                Err(e) => Value::Err(Box::new(Value::str(e))),
            })
        }
        "std::time::now" => Ok(Value::Float(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0.0, |d| d.as_secs_f64()),
        )),
        "std::map::from_entries" => {
            let mut entries = Vec::new();
            for item in want_vec(&args[0])? {
                let Value::Tuple(pair) = &item else {
                    return err("a Map is built from (key, value) pairs");
                };
                let [k, v] = pair.as_slice() else {
                    return err("a Map is built from (key, value) pairs");
                };
                entries.push((MapKey::from_value(k).map_err(RuntimeError::new)?, v.clone()));
            }
            Ok(Value::map(entries))
        }
        "std::iter::slice" => {
            let items = want_vec(&args[0])?;
            let (from, to) = slice_bounds(items.len(), &args[1], &args[2])?;
            Ok(Value::vec(items[from..to].to_vec()))
        }
        "std::iter::concat" => {
            let mut items = want_vec(&args[0])?;
            items.extend(want_vec(&args[1])?);
            Ok(Value::vec(items))
        }
        "std::iter::range" => {
            let (start, end) = match args.as_slice() {
                [end] => (0, want_i64(end)?),
                [start, end] => (want_i64(start)?, want_i64(end)?),
                _ => return err("`range` takes one or two integers"),
            };
            Ok(Value::vec((start..end).map(Value::Int).collect()))
        }
        "std::string::slice" => {
            let chars: Vec<char> = want_str(&args[0])?.chars().collect();
            let (from, to) = slice_bounds(chars.len(), &args[1], &args[2])?;
            Ok(Value::str(chars[from..to].iter().collect::<String>()))
        }
        "std::string::repeat" => {
            let n = want_i64(&args[1])?;
            if n < 0 {
                return err("a Text is repeated a number of times that is not negative");
            }
            Ok(Value::str(want_str(&args[0])?.repeat(n as usize)))
        }
        "std::text::to_text" => Ok(Value::str(args[0].to_string())),
        "std::text::format" => Ok(Value::str(format_value(
            &args[0],
            &args[0].to_string(),
            &want_str(&args[1])?,
        )?)),
        "std::math::pow" => math_pow(&args[0], &args[1]),
        "std::math::floor_div" => math_floor_div(&args[0], &args[1]),
        "std::math::abs" => match &args[0] {
            Value::Int(i) => i
                .checked_abs()
                .map(Value::Int)
                .ok_or_else(|| RuntimeError::new("integer overflow")),
            Value::Float(x) => Ok(Value::Float(x.abs())),
            other => err(format!("`abs` needs a number, found {}", other.type_name())),
        },
        "std::math::min" | "std::math::max" => {
            let want_max = path.ends_with("max");
            let ordering = match (&args[0], &args[1]) {
                (Value::Int(a), Value::Int(b)) => a.cmp(b),
                (Value::Str(a), Value::Str(b)) => a.cmp(b),
                (a, b) => want_f64(a)?
                    .partial_cmp(&want_f64(b)?)
                    .unwrap_or(std::cmp::Ordering::Equal),
            };
            let first = (ordering == std::cmp::Ordering::Less) != want_max;
            let pick = if ordering == std::cmp::Ordering::Equal || first {
                &args[0]
            } else {
                &args[1]
            };
            // Two numbers of different kinds compare as floats and give a float.
            Ok(match (&args[0], &args[1], pick) {
                (Value::Int(_), Value::Float(_), Value::Int(i)) => Value::Float(*i as f64),
                (Value::Float(_), Value::Int(_), Value::Int(i)) => Value::Float(*i as f64),
                _ => pick.clone(),
            })
        }
        "std::math::sqrt" => Ok(Value::Float(want_f64(&args[0])?.sqrt())),
        "std::math::ln" => Ok(Value::Float(want_f64(&args[0])?.ln())),
        "std::math::log2" => Ok(Value::Float(want_f64(&args[0])?.log2())),
        "std::math::log10" => Ok(Value::Float(want_f64(&args[0])?.log10())),
        "std::math::exp" => Ok(Value::Float(want_f64(&args[0])?.exp())),
        "std::math::ln_gamma" => Ok(Value::Float(ln_gamma(want_f64(&args[0])?))),
        "std::math::digamma" => Ok(Value::Float(digamma(want_f64(&args[0])?))),
        "std::math::floor" => Ok(Value::Float(want_f64(&args[0])?.floor())),
        "std::math::ceil" => Ok(Value::Float(want_f64(&args[0])?.ceil())),
        "std::math::round" => Ok(Value::Float(want_f64(&args[0])?.round())),
        "std::math::to_float" => Ok(Value::Float(want_f64(&args[0])?)),
        "std::math::to_int" => match &args[0] {
            Value::Int(i) => Ok(Value::Int(*i)),
            Value::Float(x) if x.is_finite() => Ok(Value::Int(*x as i64)),
            other => err(format!("cannot turn {other} into an Int")),
        },
        "std::map::new" => Ok(Value::map(Vec::new())),
        "std::iter::any" | "std::iter::all" | "std::iter::find" => {
            let items = want_vec(&args[0])?;
            let which = path.rsplit("::").next().unwrap_or("");
            for v in items.iter() {
                let hit = match interp.apply(&args[1], vec![v.clone()])? {
                    Value::Bool(b) => b,
                    other => {
                        return err(format!("{which} predicate returned {}", other.type_name()));
                    }
                };
                match (which, hit) {
                    ("any", true) => return Ok(Value::Bool(true)),
                    ("all", false) => return Ok(Value::Bool(false)),
                    ("find", true) => return Ok(Value::some(v.clone())),
                    _ => {}
                }
            }
            Ok(match which {
                "any" => Value::Bool(false),
                "all" => Value::Bool(true),
                _ => Value::none(),
            })
        }
        "std::iter::for_each" => {
            for v in want_vec(&args[0])?.iter() {
                interp.apply(&args[1], vec![v.clone()])?;
            }
            Ok(Value::Unit)
        }
        "std::iter::fold" => {
            let mut acc = args[1].clone();
            for v in want_vec(&args[0])?.iter() {
                acc = interp.apply(&args[2], vec![acc, v.clone()])?;
            }
            Ok(acc)
        }
        "std::option::map" | "std::option::and_then" | "std::option::filter" => {
            let which = path.rsplit("::").next().unwrap_or("");
            let Some(inner) = option_value(&args[0]) else {
                return Ok(args[0].clone());
            };
            let out = interp.apply(&args[1], vec![inner.clone()])?;
            Ok(match which {
                "map" => Value::some(out),
                "and_then" => out,
                _ => match out {
                    Value::Bool(true) => args[0].clone(),
                    Value::Bool(false) => Value::none(),
                    other => {
                        return err(format!("filter predicate returned {}", other.type_name()));
                    }
                },
            })
        }
        "std::option::ok_or" => Ok(match option_value(&args[0]) {
            Some(v) => Value::Ok(Box::new(v.clone())),
            None => Value::Err(Box::new(args[1].clone())),
        }),
        "std::option::unwrap_or_else" => match option_value(&args[0]) {
            Some(v) => Ok(v.clone()),
            None => interp.apply(&args[1], vec![]),
        },
        "std::option::expect" => match option_value(&args[0]) {
            Some(v) => Ok(v.clone()),
            None => err(want_str(&args[1])?.to_string()),
        },
        "std::result::is_ok" => Ok(Value::Bool(matches!(args[0], Value::Ok(_)))),
        "std::result::is_err" => Ok(Value::Bool(matches!(args[0], Value::Err(_)))),
        "std::result::unwrap" => match &args[0] {
            Value::Ok(v) => Ok((**v).clone()),
            Value::Err(e) => err(format!("called `unwrap` on `Err`: {e}")),
            other => err(format!("expected a Result, found {}", other.type_name())),
        },
        "std::result::expect" => match &args[0] {
            Value::Ok(v) => Ok((**v).clone()),
            Value::Err(e) => err(format!("{}: {e}", want_str(&args[1])?)),
            other => err(format!("expected a Result, found {}", other.type_name())),
        },
        "std::result::unwrap_or" => match &args[0] {
            Value::Ok(v) => Ok((**v).clone()),
            _ => Ok(args[1].clone()),
        },
        "std::result::unwrap_or_else" => match &args[0] {
            Value::Ok(v) => Ok((**v).clone()),
            Value::Err(e) => interp.apply(&args[1], vec![(**e).clone()]),
            other => err(format!("expected a Result, found {}", other.type_name())),
        },
        "std::result::map" => match &args[0] {
            Value::Ok(v) => Ok(Value::Ok(Box::new(
                interp.apply(&args[1], vec![(**v).clone()])?,
            ))),
            other => Ok(other.clone()),
        },
        "std::result::map_err" => match &args[0] {
            Value::Err(e) => Ok(Value::Err(Box::new(
                interp.apply(&args[1], vec![(**e).clone()])?,
            ))),
            other => Ok(other.clone()),
        },
        "std::result::and_then" => match &args[0] {
            Value::Ok(v) => interp.apply(&args[1], vec![(**v).clone()]),
            other => Ok(other.clone()),
        },
        "std::result::ok" => Ok(match &args[0] {
            Value::Ok(v) => Value::some((**v).clone()),
            _ => Value::none(),
        }),
        "std::result::err" => Ok(match &args[0] {
            Value::Err(e) => Value::some((**e).clone()),
            _ => Value::none(),
        }),
        "std::iter::enumerate" => Ok(Value::vec(
            want_vec(&args[0])?
                .into_iter()
                .enumerate()
                .map(|(i, v)| Value::Tuple(Arc::new(vec![Value::Int(i as i64), v])))
                .collect(),
        )),
        "std::iter::zip" => Ok(Value::vec(
            want_vec(&args[0])?
                .into_iter()
                .zip(want_vec(&args[1])?)
                .map(|(a, b)| Value::Tuple(Arc::new(vec![a, b])))
                .collect(),
        )),
        "std::iter::sum" => {
            let items = want_vec(&args[0])?;
            if items.iter().any(|v| matches!(v, Value::Float(_))) {
                let mut total = 0.0;
                for v in items.iter() {
                    match v {
                        Value::Float(x) => total += x,
                        Value::Int(i) => total += *i as f64,
                        other => return err(format!("cannot sum {}", other.type_name())),
                    }
                }
                Ok(Value::Float(total))
            } else {
                let mut total: i64 = 0;
                for v in items.iter() {
                    match v {
                        Value::Int(i) => {
                            total = total
                                .checked_add(*i)
                                .ok_or_else(|| RuntimeError::new("integer overflow in `sum`"))?;
                        }
                        other => return err(format!("cannot sum {}", other.type_name())),
                    }
                }
                Ok(Value::Int(total))
            }
        }
        "std::iter::min" | "std::iter::max" => {
            let items = want_vec(&args[0])?;
            let want_max = path.ends_with("max");
            let mut best: Option<Value> = None;
            for v in items {
                best = Some(match best {
                    None => v,
                    Some(b) => {
                        let less = crate::value::compare(&v, &b).map_err(RuntimeError::new)?
                            == std::cmp::Ordering::Less;
                        if less != want_max { v } else { b }
                    }
                });
            }
            Ok(Value::option(best))
        }
        "std::iter::skip" => {
            let n = match &args[1] {
                Value::Int(n) => usize::try_from(*n).unwrap_or(0),
                other => {
                    return err(format!(
                        "`skip` needs an integer, found {}",
                        other.type_name()
                    ));
                }
            };
            Ok(Value::vec(
                want_vec(&args[0])?.into_iter().skip(n).collect(),
            ))
        }
        "std::iter::count" => Ok(Value::Int(want_vec(&args[0])?.len() as i64)),
        "std::collect" => Ok(args[0].clone()),
        "std::io::print" => interp.write_out(&want_str(&args[0])?).map(|()| Value::Unit),
        "std::io::println" => interp
            .write_out(&format!("{}\n", want_str(&args[0])?))
            .map(|()| Value::Unit),
        "std::http::Response::new" => Ok(Value::Native(Native::Response(Arc::new(
            crate::value::ResponseVal {
                status: 200,
                headers: Vec::new(),
                body: args[0].clone(),
                text: None,
                events: None,
                forward: None,
            },
        )))),
        "std::http::Response::events" => Ok(Value::Native(Native::Response(Arc::new(
            crate::value::ResponseVal {
                status: 200,
                headers: Vec::new(),
                body: Value::Unit,
                text: None,
                events: Some(args[0].clone()),
                forward: None,
            },
        )))),
        "std::http::Response::text" | "std::http::Response::html" => {
            let kind = if path.ends_with("::html") {
                "text/html"
            } else {
                "text/plain"
            };
            Ok(Value::Native(Native::Response(Arc::new(
                crate::value::ResponseVal {
                    status: 200,
                    headers: vec![("content-type".into(), format!("{kind}; charset=utf-8"))],
                    body: Value::Unit,
                    text: Some(want_str(&args[0])?.to_string()),
                    events: None,
                    forward: None,
                },
            ))))
        }
        "std::http::Status::BadRequest" => status(400, &args),
        "std::http::Status::Unauthorized" => status(401, &args),
        "std::http::Status::Forbidden" => status(403, &args),
        "std::http::Status::NotFound" => status(404, &args),
        "std::http::Status::Unprocessable" => status(422, &args),
        "std::http::Status::Internal" => status(500, &args),
        "std::ai::embeddings::OllamaEmbedder::new" => Ok(Value::Native(Native::Embedder(
            Arc::new(crate::value::EmbedderVal {
                model: want_str(&args[0])?,
                endpoint: if interp.is_live() {
                    Some(env_or("OLLAMA_HOST", "http://localhost:11434").into())
                } else {
                    None
                },
            }),
        ))),
        "std::ai::llm::OpenAIClient::new" => {
            let endpoint = if interp.is_live() {
                let key = std::env::var("OPENAI_API_KEY").map_err(|_| {
                    RuntimeError::new("live mode needs the OPENAI_API_KEY environment variable")
                })?;
                Some(Arc::new((
                    env_or("OPENAI_BASE_URL", "https://api.openai.com/v1"),
                    key,
                )))
            } else {
                None
            };
            Ok(Value::Native(Native::Llm(Arc::new(crate::value::LlmVal {
                model: want_str(&args[0])?,
                endpoint,
            }))))
        }
        "std::db::postgres::PostgresClient::connect" => Ok(Value::Native(Native::Postgres(
            Arc::new(crate::value::PgClient::new(&want_str(&args[0])?)),
        ))),
        "std::db::pgvector::PgVectorClient::connect" => Ok(Value::Native(Native::Db(
            interp.db_for(&want_str(&args[0])?)?,
        ))),
        other => err(format!(
            "std function `{other}` is not implemented by the runtime"
        )),
    }
}

/// `execute` and `query` of a `PostgresClient`. `result` is the type of the call, which says what a row is.
pub(crate) fn postgres_method(
    interp: &Interp,
    pg: &crate::value::PgClient,
    name: &str,
    args: &[Value],
    result: &Ty,
) -> Res {
    if !matches!(name, "execute" | "query") {
        return err(format!("PostgresClient has no method `{name}`"));
    }
    interp.require(&format!("std::db::postgres::PostgresClient::{name}"))?;
    if !interp.is_live() {
        return err(
            "`std.db.postgres` needs `--live`: there is no offline stand-in for SQL, so run the program with `telvax serve --live`",
        );
    }
    let sql = want_str(
        args.first()
            .ok_or_else(|| RuntimeError::new("missing the SQL"))?,
    )?;
    let params = postgres_params(args.get(1))?;
    #[cfg(feature = "live")]
    {
        let fail = |e: String| Value::Err(Box::new(Value::str(e)));
        if name == "execute" {
            return Ok(match pg.execute(&sql, &params) {
                Ok(n) => Value::Ok(Box::new(Value::Int(n))),
                Err(e) => fail(e),
            });
        }
        let Ty::Result(rows, _) = result else {
            return err(
                "`query` needs the type of its rows: `let rows: List[Job] = db.query(sql, params)?`",
            );
        };
        Ok(match pg.query(&sql, &params) {
            Ok(rows_json) => {
                match interp.value_from_json(rows, &serde_json::Value::Array(rows_json)) {
                    Ok(v) => Value::Ok(Box::new(v)),
                    Err(e) => fail(e),
                }
            }
            Err(e) => fail(e),
        })
    }
    #[cfg(not(feature = "live"))]
    {
        let _ = (pg, params, sql, result);
        err("this build of Telvax has no PostgreSQL support; build it with the `live` feature")
    }
}

/// The values of a list or a tuple, as the parameters of a statement.
fn postgres_params(v: Option<&Value>) -> Result<Vec<crate::value::PgParam>, RuntimeError> {
    use crate::value::PgParam;
    fn one(v: &Value) -> Result<PgParam, RuntimeError> {
        Ok(match v {
            Value::Str(s) => PgParam::Text(s.to_string()),
            Value::Int(i) => PgParam::Int(*i),
            Value::Float(x) => PgParam::Float(*x),
            Value::Bool(b) => PgParam::Bool(*b),
            Value::Enum(e) if e.name == "Option" && e.variant == "None" => PgParam::Null,
            Value::Enum(e) if e.name == "Option" => match e.fields.first() {
                Some(inner) => one(inner)?,
                None => PgParam::Null,
            },
            other => {
                return err(format!(
                    "a statement parameter must be a Text, Int, Float, Bool or an optional of those, found {}",
                    other.type_name()
                ));
            }
        })
    }
    match v {
        None => Ok(Vec::new()),
        Some(Value::Vec(items)) => items.snapshot().iter().map(one).collect(),
        Some(Value::Tuple(items)) => items.iter().map(one).collect(),
        Some(other) => err(format!(
            "the parameters must be a list or a tuple, found {}",
            other.type_name()
        )),
    }
}

fn want_i64(v: &Value) -> Result<i64, RuntimeError> {
    match v {
        Value::Int(i) => Ok(*i),
        other => err(format!("expected an Int, found {}", other.type_name())),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn random_fill(buf: &mut [u8]) -> Result<(), RuntimeError> {
    getrandom::fill(buf).or_else(|e| err(format!("the operating system gave no random bytes: {e}")))
}

#[cfg(target_arch = "wasm32")]
fn random_fill(_: &mut [u8]) -> Result<(), RuntimeError> {
    err("std.random is not available in the browser")
}

fn want_f64(v: &Value) -> Result<f64, RuntimeError> {
    match v {
        Value::Int(i) => Ok(*i as f64),
        Value::Float(x) => Ok(*x),
        other => err(format!("expected a number, found {}", other.type_name())),
    }
}

/// The part of a sequence of `len` items that `xs[from:to]` selects. A negative bound counts from
/// the end, and a bound past either end stops at it.
pub(crate) fn slice_bounds(
    len: usize,
    from: &Value,
    to: &Value,
) -> Result<(usize, usize), RuntimeError> {
    let at = |i: i64| -> usize {
        if i < 0 {
            (len as i64).saturating_add(i).max(0) as usize
        } else {
            (i as usize).min(len)
        }
    };
    let (from, to) = (at(want_i64(from)?), at(want_i64(to)?));
    Ok((from.min(to), to))
}

fn math_pow(base: &Value, exp: &Value) -> Res {
    match (base, exp) {
        (Value::Int(b), Value::Int(e)) => {
            let e = u32::try_from(*e).map_err(|_| {
                RuntimeError::new("a negative exponent needs a Float: write `2.0 ** -1`")
            })?;
            b.checked_pow(e)
                .map(Value::Int)
                .ok_or_else(|| RuntimeError::new("integer overflow"))
        }
        (b, e) => Ok(Value::Float(want_f64(b)?.powf(want_f64(e)?))),
    }
}

fn math_floor_div(a: &Value, b: &Value) -> Res {
    match (a, b) {
        (Value::Int(_), Value::Int(0)) => err("division by zero"),
        (Value::Int(a), Value::Int(b)) => {
            let q = a
                .checked_div(*b)
                .ok_or_else(|| RuntimeError::new("integer overflow"))?;
            Ok(Value::Int(if a % b != 0 && ((*a < 0) != (*b < 0)) {
                q - 1
            } else {
                q
            }))
        }
        (a, b) => {
            let (a, b) = (want_f64(a)?, want_f64(b)?);
            if b == 0.0 {
                return err("division by zero");
            }
            Ok(Value::Float((a / b).floor()))
        }
    }
}

/// `{value:spec}` in an f-string: `[[fill]align][sign][#][0][width][grouping][.precision][type]`.
/// The alignments are `<`, `>`, `^` and `=` (padding between the sign and the digits), the signs
/// `+`, `-` and a space, the groupings `,` and `_`, and the types `s`, `d`, `f`, `F`, `e`, `E`,
/// `g`, `G`, `x`, `X`, `o`, `b`, `c` and `%`. `shown` is how the value reads without a type.
pub(crate) fn format_value(v: &Value, shown: &str, spec: &str) -> Result<String, RuntimeError> {
    let bad = || {
        RuntimeError::new(format!(
            "`{spec}` is not a format; write for example `.2f`, `>8`, `05d`, `+.1e` or `,`"
        ))
    };
    let chars: Vec<char> = spec.chars().collect();
    let mut i = 0;
    let (mut fill, mut align) = (' ', None);
    if chars.len() >= 2 && matches!(chars[1], '<' | '>' | '^' | '=') {
        (fill, align) = (chars[0], Some(chars[1]));
        i = 2;
    } else if chars
        .first()
        .is_some_and(|c| matches!(c, '<' | '>' | '^' | '='))
    {
        align = Some(chars[0]);
        i = 1;
    }
    let sign = match chars.get(i) {
        Some(&c @ ('+' | '-' | ' ')) => {
            i += 1;
            c
        }
        _ => '-',
    };
    let alternate = chars.get(i) == Some(&'#');
    if alternate {
        i += 1;
    }
    let zero = chars.get(i) == Some(&'0');
    if zero {
        i += 1;
    }
    let number = |i: &mut usize| -> Option<usize> {
        let start = *i;
        while chars.get(*i).is_some_and(char::is_ascii_digit) {
            *i += 1;
        }
        chars[start..*i].iter().collect::<String>().parse().ok()
    };
    let width = number(&mut i).unwrap_or(0);
    let group = match chars.get(i) {
        Some(&c @ (',' | '_')) => {
            i += 1;
            Some(c)
        }
        _ => None,
    };
    let precision = if chars.get(i) == Some(&'.') {
        i += 1;
        Some(number(&mut i).ok_or_else(bad)?)
    } else {
        None
    };
    let kind = chars.get(i).copied();
    if i + usize::from(kind.is_some()) != chars.len() {
        return Err(bad());
    }
    let needs_int = |what: &str| RuntimeError::new(format!("`{spec}` needs an Int, found {what}",));
    // The digits of the number (without its sign), and whether it is negative.
    let (mut digits, negative, numeric) = match (kind, v) {
        (Some('x' | 'X' | 'o' | 'b' | 'd' | 'c'), Value::Int(n)) => {
            let magnitude = n.unsigned_abs();
            let text = match kind {
                Some('x') => format!("{magnitude:x}"),
                Some('X') => format!("{magnitude:X}"),
                Some('o') => format!("{magnitude:o}"),
                Some('b') => format!("{magnitude:b}"),
                Some('c') => {
                    let c = u32::try_from(*n)
                        .ok()
                        .and_then(char::from_u32)
                        .ok_or_else(|| RuntimeError::new(format!("{n} is not a character")))?;
                    return Ok(pad(&c.to_string(), width, fill, align.unwrap_or('<')));
                }
                _ => magnitude.to_string(),
            };
            (text, *n < 0, true)
        }
        (Some('x' | 'X' | 'o' | 'b' | 'd' | 'c'), other) => {
            return Err(needs_int(other.type_name()));
        }
        (Some(t @ ('f' | 'F' | 'e' | 'E' | 'g' | 'G' | '%')), v) => {
            let x = want_f64(v)?;
            let x = if t == '%' { x * 100.0 } else { x };
            let text = if !x.is_finite() {
                if x.is_nan() { "nan" } else { "inf" }.to_string()
            } else {
                match t {
                    'f' | 'F' | '%' => format!("{:.*}", precision.unwrap_or(6), x.abs()),
                    'e' | 'E' => exponent_form(x.abs(), precision.unwrap_or(6)),
                    _ => general_form(x.abs(), precision.unwrap_or(6), false),
                }
            };
            let text = match t {
                'E'..='G' => text.to_uppercase(),
                '%' => format!("{text}%"),
                _ => text,
            };
            (text, x.is_sign_negative() && !x.is_nan(), true)
        }
        (None | Some('s'), Value::Float(x)) if precision.is_some() => {
            let mut text = general_form(x.abs(), precision.unwrap_or(6).max(1), true);
            if !x.is_finite() {
                text = if x.is_nan() { "nan" } else { "inf" }.to_string();
            }
            (text, x.is_sign_negative() && !x.is_nan(), true)
        }
        (None | Some('s'), Value::Str(s)) if precision.is_some() => {
            let text: String = s.chars().take(precision.unwrap_or(0)).collect();
            return Ok(pad(&text, width, fill, align.unwrap_or('<')));
        }
        (None | Some('s'), Value::Int(n)) => (n.unsigned_abs().to_string(), *n < 0, true),
        (None | Some('s'), Value::Float(x)) => {
            let text = shown.strip_prefix('-').unwrap_or(shown).to_string();
            (text, x.is_sign_negative() && !x.is_nan(), true)
        }
        (None | Some('s'), _) => {
            return Ok(pad(shown, width, fill, align.unwrap_or('<')));
        }
        _ => return Err(bad()),
    };
    if numeric && let Some(sep) = group {
        digits = group_digits(&digits, sep, matches!(kind, Some('x' | 'X' | 'o' | 'b')));
    }
    let prefix = match (alternate, kind) {
        (true, Some('x')) => "0x",
        (true, Some('X')) => "0X",
        (true, Some('o')) => "0o",
        (true, Some('b')) => "0b",
        _ => "",
    };
    let sign_text = if negative {
        "-"
    } else {
        match sign {
            '+' => "+",
            ' ' => " ",
            _ => "",
        }
    };
    let head = format!("{sign_text}{prefix}");
    let len = head.chars().count() + digits.chars().count();
    if len >= width {
        return Ok(format!("{head}{digits}"));
    }
    let gap = width - len;
    // `0` pads with zeros between the sign and the digits, unless an alignment was asked for.
    let (fill, align) = match (zero, align) {
        (true, None) => ('0', '='),
        (_, Some(a)) => (fill, a),
        (_, None) => (fill, '>'),
    };
    let make = |n: usize| fill.to_string().repeat(n);
    Ok(match align {
        '<' => format!("{head}{digits}{}", make(gap)),
        '^' => format!("{}{head}{digits}{}", make(gap / 2), make(gap - gap / 2)),
        '=' => format!("{head}{}{digits}", make(gap)),
        _ => format!("{}{head}{digits}", make(gap)),
    })
}

fn pad(text: &str, width: usize, fill: char, align: char) -> String {
    let len = text.chars().count();
    if len >= width {
        return text.to_string();
    }
    let gap = width - len;
    let make = |n: usize| fill.to_string().repeat(n);
    match align {
        '>' | '=' => format!("{}{text}", make(gap)),
        '^' => format!("{}{text}{}", make(gap / 2), make(gap - gap / 2)),
        _ => format!("{text}{}", make(gap)),
    }
}

/// `1234567.5` with `,` or `_` between the groups of three digits of its whole part (four for
/// hexadecimal, octal and binary).
fn group_digits(text: &str, sep: char, wide: bool) -> String {
    let size = if wide { 4 } else { 3 };
    if wide && sep == ',' {
        return text.to_string();
    }
    let split = text.find(['.', 'e', 'E', '%']).unwrap_or(text.len());
    let (whole, rest) = text.split_at(split);
    let mut out = String::new();
    for (n, c) in whole.chars().enumerate() {
        if n > 0 && (whole.len() - n) % size == 0 {
            out.push(sep);
        }
        out.push(c);
    }
    format!("{out}{rest}")
}

/// `1.5e+03`: the form of `{x:e}`, with the exponent signed and at least two digits.
fn exponent_form(x: f64, precision: usize) -> String {
    let text = format!("{x:.precision$e}");
    let (mantissa, exp) = text.split_once('e').unwrap_or((&text, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    format!(
        "{mantissa}e{}{:02}",
        if exp < 0 { '-' } else { '+' },
        exp.abs()
    )
}

/// The form of `{x:g}`: `precision` significant digits, in the exponent form when the number is
/// very large or very small, and without trailing zeros. With `point` a whole number keeps a
/// `.0` (what a float shows when no type was given).
fn general_form(x: f64, precision: usize, point: bool) -> String {
    let precision = precision.max(1);
    if x == 0.0 {
        return if point { "0.0" } else { "0" }.to_string();
    }
    let sci = format!("{:.*e}", precision - 1, x);
    let exp: i32 = sci
        .split_once('e')
        .and_then(|(_, e)| e.parse().ok())
        .unwrap_or(0);
    let strip = |text: String| {
        if text.contains('.') {
            text.trim_end_matches('0').trim_end_matches('.').to_string()
        } else {
            text
        }
    };
    if exp < -4 || exp >= precision as i32 {
        let (mantissa, _) = sci.split_once('e').unwrap_or((&sci, "0"));
        return format!(
            "{}e{}{:02}",
            strip(mantissa.to_string()),
            if exp < 0 { '-' } else { '+' },
            exp.abs()
        );
    }
    let decimals = (precision as i32 - 1 - exp).max(0) as usize;
    let text = strip(format!("{x:.decimals$}"));
    if point && !text.contains('.') {
        format!("{text}.0")
    } else {
        text
    }
}

fn arg(args: &[Value], i: usize) -> Result<&Value, RuntimeError> {
    args.get(i)
        .ok_or_else(|| RuntimeError::new(format!("missing argument {}", i + 1)))
}

/// `receiver.name(args)`. For vector-store query builders the piped query is `args[0]`.
pub(crate) fn call_method(receiver: &Value, name: &str, args: crate::value::Args) -> Res {
    match receiver {
        Value::Str(s) => match name {
            "is_empty" => Ok(Value::Bool(s.is_empty())),
            "len" => Ok(Value::Int(s.len() as i64)),
            "trim" => Ok(Value::str(s.trim())),
            // An ASCII text is changed byte by byte, and one that has nothing to change is given back
            // as it is, with no copy.
            "to_lowercase" if s.is_ascii() => Ok(if s.bytes().any(|b| b.is_ascii_uppercase()) {
                Value::ascii_case(s, false)
            } else {
                receiver.clone()
            }),
            "to_uppercase" if s.is_ascii() => Ok(if s.bytes().any(|b| b.is_ascii_lowercase()) {
                Value::ascii_case(s, true)
            } else {
                receiver.clone()
            }),
            "to_lowercase" => Ok(Value::str(s.to_lowercase())),
            "to_uppercase" => Ok(Value::str(s.to_uppercase())),
            "contains" => Ok(Value::Bool(s.contains(&*want_str(arg(&args, 0)?)?))),
            "starts_with" => Ok(Value::Bool(s.starts_with(&*want_str(arg(&args, 0)?)?))),
            "ends_with" => Ok(Value::Bool(s.ends_with(&*want_str(arg(&args, 0)?)?))),
            "replace" => Ok(Value::str(
                s.replace(&*want_str(arg(&args, 0)?)?, &want_str(arg(&args, 1)?)?),
            )),
            "find" => {
                let needle = want_str(arg(&args, 0)?)?;
                Ok(Value::option(
                    s.find(&*needle).map(|i| Value::Int(i as i64)),
                ))
            }
            "to_int" => Ok(Value::option(s.trim().parse::<i64>().ok().map(Value::Int))),
            "to_float" => Ok(Value::option(
                s.trim().parse::<f64>().ok().map(Value::Float),
            )),
            "chars" => Ok(Value::vec(
                s.chars().map(|c| Value::str(c.to_string())).collect(),
            )),
            "lines" => Ok(Value::vec(s.lines().map(Value::str).collect())),
            "split" => {
                let sep = want_str(arg(&args, 0)?)?;
                if sep.is_empty() {
                    return err("`split` needs a separator that is not empty");
                }
                Ok(Value::vec(s.split(&*sep).map(Value::str).collect()))
            }
            _ => err(format!("Text has no method `{name}`")),
        },
        Value::Table(t) => match name {
            "len" => Ok(Value::Int(t.rows() as i64)),
            "is_empty" => Ok(Value::Bool(t.rows() == 0)),
            _ => err(format!("Table has no method `{name}`")),
        },
        Value::Vec(v) => match name {
            "is_empty" => Ok(Value::Bool(v.is_empty())),
            "len" => Ok(Value::Int(v.len() as i64)),
            "push" => {
                v.push(arg(&args, 0)?.clone());
                Ok(Value::Unit)
            }
            "contains" => {
                let needle = arg(&args, 0)?;
                Ok(Value::Bool(v.contains(needle)))
            }
            "first" => Ok(Value::option(v.get(0))),
            "last" => Ok(Value::option(v.len().checked_sub(1).and_then(|i| v.get(i)))),
            "pop" => Ok(Value::option(v.pop())),
            "get" => match arg(&args, 0)? {
                Value::Int(i) => Ok(Value::option(
                    usize::try_from(*i).ok().and_then(|i| v.get(i)),
                )),
                other => err(format!(
                    "`get` needs an integer index, found {}",
                    other.type_name()
                )),
            },
            "sort" => v.sort().map(|()| Value::Unit).map_err(RuntimeError::new),
            "reverse" => {
                v.reverse();
                Ok(Value::Unit)
            }
            _ => err(format!("List has no method `{name}`")),
        },
        Value::Enum(e) if e.name == "Option" => match name {
            "is_some" => Ok(Value::Bool(e.variant == "Some")),
            "is_none" => Ok(Value::Bool(e.variant == "None")),
            "unwrap" => e
                .fields
                .first()
                .cloned()
                .ok_or_else(|| RuntimeError::new("called `unwrap` on `None`")),
            "unwrap_or" => Ok(e.fields.first().cloned().unwrap_or(arg(&args, 0)?.clone())),
            _ => err(format!("Option has no method `{name}`")),
        },
        Value::Map(m) => map_method(m, name, &args),
        Value::Native(Native::Response(r)) => {
            let mut next = (**r).clone();
            match name {
                "status" => next.status = want_i64(&args[0])?,
                "header" => {
                    let key = want_str(&args[0])?.to_ascii_lowercase();
                    next.headers.retain(|(k, _)| *k != key);
                    next.headers.push((key, want_str(&args[1])?.to_string()));
                }
                _ => return err(format!("Response has no method `{name}`")),
            }
            Ok(Value::Native(Native::Response(Arc::new(next))))
        }
        Value::Native(Native::Cache(cache)) => match name {
            "get" => Ok(Value::option(cache.get(&want_str(&args[0])?))),
            "set" => {
                cache.set(&want_str(&args[0])?, args[1].clone());
                Ok(Value::Unit)
            }
            "remove" => Ok(Value::option(cache.remove(&want_str(&args[0])?))),
            "clear" => Ok(Value::Int(cache.clear() as i64)),
            "len" => Ok(Value::Int(cache.len() as i64)),
            _ => err(format!("Cache has no method `{name}`")),
        },
        Value::Native(Native::Sink(sink)) => {
            let text = match name {
                "send" => sse_event(None, &event_text(&args[0])),
                "event" => sse_event(Some(&want_str(&args[0])?), &event_text(&args[1])),
                "comment" => sse_comment(&want_str(&args[0])?),
                _ => return err(format!("Sink has no method `{name}`")),
            };
            if (sink.0)(text) {
                Ok(Value::Unit)
            } else {
                err("the client closed the connection")
            }
        }
        Value::Native(Native::Request(r)) => match name {
            "header" => {
                let wanted = want_str(&args[0])?.to_ascii_lowercase();
                Ok(r.headers
                    .iter()
                    .find(|(k, _)| *k == wanted)
                    .map_or_else(Value::none, |(_, v)| Value::some(Value::str(v.as_str()))))
            }
            "method" => Ok(Value::str(r.method.as_str())),
            "path" => Ok(Value::str(r.path.as_str())),
            _ => err(format!("Request has no method `{name}`")),
        },
        Value::Native(Native::Embedder(client)) if name == "embed" => {
            let crate::value::EmbedderVal { model, endpoint } = &**client;
            let text = want_str(&args[0])?;
            match endpoint {
                #[cfg(feature = "live")]
                Some(host) => Ok(tensor(crate::services::ollama_embed(
                    host,
                    model,
                    &text,
                    EMBEDDING_DIM as usize,
                )?)),
                #[cfg(not(feature = "live"))]
                Some(_) => err("live mode is not available in this build"),
                None => {
                    let _ = model;
                    Ok(tensor(embed_text(&text)))
                }
            }
        }
        Value::Native(Native::Llm(client)) if name == "complete" => {
            let crate::value::LlmVal { model, endpoint } = &**client;
            let prompt = want_str(&args[0])?;
            #[cfg(feature = "live")]
            if let Some(ep) = endpoint {
                return Ok(Value::str(crate::services::openai_complete(
                    &ep.0, &ep.1, model, &prompt,
                )?));
            }
            #[cfg(not(feature = "live"))]
            if endpoint.is_some() {
                return err("live mode is not available in this build");
            }
            let question = prompt
                .lines()
                .rev()
                .find_map(|l| l.strip_prefix("User Question: "))
                .unwrap_or_else(|| prompt.lines().last().unwrap_or(""));
            let context = prompt.lines().filter(|l| !l.is_empty()).count();
            Ok(Value::str(format!(
                "[{model} offline stand-in] answer to \"{question}\" from {context} prompt line(s)"
            )))
        }
        Value::Native(Native::Db(db)) => db_method(db, name, args.into_vec()),
        other => err(format!("{} has no method `{name}`", other.type_name())),
    }
}

fn map_method(m: &Arc<MapVal>, name: &str, args: &[Value]) -> Res {
    let key = |i: usize| MapKey::from_value(arg(args, i)?).map_err(RuntimeError::new);
    match name {
        "insert" => {
            m.insert(key(0)?, arg(args, 1)?.clone());
            Ok(Value::Unit)
        }
        "get" => Ok(Value::option(m.get(&key(0)?))),
        "remove" => Ok(Value::option(m.remove(&key(0)?))),
        "contains_key" => Ok(Value::Bool(m.contains(&key(0)?))),
        "len" => Ok(Value::Int(m.len() as i64)),
        "is_empty" => Ok(Value::Bool(m.is_empty())),
        "keys" => Ok(Value::vec(
            m.entries().iter().map(|(k, _)| k.to_value()).collect(),
        )),
        "values" => Ok(Value::vec(
            m.entries().into_iter().map(|(_, v)| v).collect(),
        )),
        "entries" => Ok(Value::vec(
            m.entries()
                .into_iter()
                .map(|(k, v)| Value::Tuple(Arc::new(vec![k.to_value(), v])))
                .collect(),
        )),
        _ => err(format!("Map has no method `{name}`")),
    }
}

fn db_method(db: &Arc<Db>, name: &str, args: Vec<Value>) -> Res {
    match name {
        "search_collection" => Ok(Value::Native(Native::Query(Arc::new(Query {
            db: db.clone(),
            collection: want_str(&args[0])?.to_string(),
            vector: None,
            limit: 10,
        })))),
        "with_vector" => {
            let mut q = want_query(&args[0])?;
            match &args[1] {
                Value::Tensor(t) => q.vector = Some(t.to_f32_vec()),
                other => return err(format!("expected a Tensor, found {}", other.type_name())),
            }
            Ok(Value::Native(Native::Query(Arc::new(q))))
        }
        "limit" => {
            let mut q = want_query(&args[0])?;
            q.limit = want_usize(&args[1])?;
            Ok(Value::Native(Native::Query(Arc::new(q))))
        }
        "execute" => execute(&want_query(&args[0])?),
        "insert" => {
            let Value::Tensor(t) = &args[3] else {
                return err("insert expects a Tensor vector");
            };
            db.insert(
                &want_str(&args[0])?,
                &want_str(&args[1])?,
                &want_str(&args[2])?,
                t.to_f32_vec(),
            )?;
            Ok(Value::Unit)
        }
        _ => err(format!("PgVectorClient has no method `{name}`")),
    }
}

fn want_query(v: &Value) -> Result<Query, RuntimeError> {
    match v {
        Value::Native(Native::Query(q)) => Ok((**q).clone()),
        other => err(format!(
            "expected a VectorQuery, found {}",
            other.type_name()
        )),
    }
}

fn execute(q: &Query) -> Res {
    let Some(vector) = &q.vector else {
        return err("vector query has no `with_vector(..)`");
    };
    #[cfg(feature = "live")]
    if let Some(pg) = &q.db.pg {
        let rows = pg.search(&q.collection, vector, q.limit)?;
        return Ok(Value::vec(
            rows.into_iter()
                .map(|(id, text, score)| row(&id, &text, score))
                .collect(),
        ));
    }
    let cols = q.db.collections.lock().unwrap_or_else(|e| e.into_inner());
    let mut scored: Vec<(f64, &crate::value::Doc)> = cols
        .get(&q.collection)
        .map(|docs| {
            docs.iter()
                .map(|d| {
                    let dot: f32 = d.vector.iter().zip(vector).map(|(a, b)| a * b).sum();
                    (f64::from(dot), d)
                })
                .collect()
        })
        .unwrap_or_default();
    scored.sort_by(|a, b| b.0.total_cmp(&a.0).then_with(|| a.1.id.cmp(&b.1.id)));
    let rows = scored
        .into_iter()
        .take(q.limit)
        .map(|(score, d)| row(&d.id, &d.text, score))
        .collect();
    Ok(Value::vec(rows))
}

/// A search hit: an object with `id`, `text` and `score`.
fn row(id: &str, text: &str, score: f64) -> Value {
    Value::Struct(Arc::new(StructVal {
        name: "Row".into(),
        fields: vec![
            ("id".into(), Value::str(id)),
            ("text".into(), Value::str(text)),
            ("score".into(), Value::Float(score)),
        ],
    }))
}

fn env_or(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| default.to_string())
}

/// `std::dist::map(items, f)`: `f(item)` for every item, in parallel, wherever the interpreter's
/// executor puts it. Results come back in input order.
fn dist_map(interp: &Interp, args: &[Value]) -> Res {
    let items = want_vec(&args[0])?;
    let Value::Func(name) = &args[1] else {
        return err("`std.dist.map` needs the name of a top-level function, not a closure");
    };
    if let Some(exec) = interp.executor() {
        let (_, ret) = interp
            .signature(name)
            .ok_or_else(|| RuntimeError::new(format!("no function `{name}`")))?;
        let payload: Vec<serde_json::Value> = items.iter().map(Value::to_json).collect();
        let (params, _) = interp.signature(name).expect("checked above");
        if params.len() != 1 {
            return err(format!(
                "`{name}` must take exactly one argument to be mapped"
            ));
        }
        let results = exec.map(name, payload).map_err(RuntimeError::new)?;
        if results.len() != items.len() {
            return err(format!(
                "the executor returned {} results for {} items",
                results.len(),
                items.len()
            ));
        }
        let ret = match ret {
            // A function returning `Result<T, E>` yields `T` per item; errors fail the whole map.
            Ty::Result(ok, _) => *ok,
            other => other,
        };
        let out = results
            .iter()
            .map(|j| {
                interp
                    .value_from_json(&ret, j)
                    .map_err(|e| RuntimeError::new(format!("result of `{name}`: {e}")))
            })
            .collect::<Result<Vec<_>, _>>()?;
        return Ok(Value::vec(out));
    }

    // No executor: spread the calls over this machine's cores.
    let n = items.len();
    let workers = std::thread::available_parallelism()
        .map_or(1, |p| p.get())
        .min(n.max(1));
    let next = std::sync::atomic::AtomicUsize::new(0);
    let slots: Vec<std::sync::Mutex<Option<Result<Value, RuntimeError>>>> =
        (0..n).map(|_| std::sync::Mutex::new(None)).collect();
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    if i >= n {
                        break;
                    }
                    let r = interp.apply(&args[1], vec![items[i].clone()]);
                    *slots[i].lock().unwrap_or_else(|e| e.into_inner()) = Some(r);
                }
            });
        }
    });
    slots
        .into_iter()
        .map(|s| {
            s.into_inner()
                .unwrap_or_else(|e| e.into_inner())
                .unwrap_or_else(|| err("a worker thread stopped early"))
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Value::vec)
}

/// The value inside a `Some`, or `None` for `None`.
fn option_value(v: &Value) -> Option<&Value> {
    match v {
        Value::Enum(e) if e.name == "Option" => e.fields.first(),
        _ => None,
    }
}

/// `t` seconds since the epoch written in UTC with a strftime pattern.
fn time_format(t: f64, pattern: &str) -> Result<String, String> {
    use chrono::format::{Item, StrftimeItems};
    if !t.is_finite() {
        return Err(format!("std.time.format: {t} is not a time"));
    }
    let secs = t.floor();
    let nanos = ((t - secs) * 1e9).round().min(999_999_999.0) as u32;
    let moment = chrono::DateTime::from_timestamp(secs as i64, nanos)
        .ok_or_else(|| format!("std.time.format: {t} is outside the times that can be written"))?;
    let items: Vec<Item> = StrftimeItems::new(pattern).collect();
    if items.iter().any(|item| matches!(item, Item::Error)) {
        return Err(format!(
            "std.time.format: `{pattern}` is not a valid pattern (see `strftime`: `%Y`, `%m`, `%d`, `%H`, `%M`, `%S`)"
        ));
    }
    Ok(moment.format_with_items(items.iter()).to_string())
}

/// The seconds since the epoch of a time written with a strftime pattern: with an offset, a date and time,
/// or a date alone (midnight), the last two in UTC.
fn time_parse(text: &str, pattern: &str) -> Result<f64, String> {
    let seconds = |secs: i64, nanos: u32| secs as f64 + f64::from(nanos) / 1e9;
    if let Ok(t) = chrono::DateTime::parse_from_str(text, pattern) {
        return Ok(seconds(t.timestamp(), t.timestamp_subsec_nanos()));
    }
    if let Ok(t) = chrono::NaiveDateTime::parse_from_str(text, pattern) {
        let t = t.and_utc();
        return Ok(seconds(t.timestamp(), t.timestamp_subsec_nanos()));
    }
    if let Ok(d) = chrono::NaiveDate::parse_from_str(text, pattern) {
        return Ok(seconds(
            d.and_hms_opt(0, 0, 0)
                .map_or(0, |t| t.and_utc().timestamp()),
            0,
        ));
    }
    Err(format!("cannot read `{text}` as `{pattern}`"))
}

/// `Some(text)` or `None`, as a value.
fn optional_text(text: Option<String>) -> Value {
    match text {
        Some(t) => Value::some(Value::str(t)),
        None => Value::none(),
    }
}

/// The natural logarithm of |gamma(x)|: the Lanczos approximation (g = 7, 9 terms), good to about 1e-14 for
/// `x` of 0.5 or more, and the reflection formula below that. The poles (0 and the negative whole numbers) are `inf`.
fn ln_gamma(x: f64) -> f64 {
    const G: f64 = 7.0;
    const C: [f64; 9] = [
        0.999_999_999_999_809_9,
        676.520_368_121_885_1,
        -1_259.139_216_722_402_8,
        771.323_428_777_653_1,
        -176.615_029_162_140_6,
        12.507_343_278_686_905,
        -0.138_571_095_265_720_12,
        9.984_369_578_019_572e-6,
        1.505_632_735_149_311_6e-7,
    ];
    if x.is_nan() {
        return f64::NAN;
    }
    if x <= 0.0 && x == x.floor() {
        return f64::INFINITY;
    }
    if x < 0.5 {
        // gamma(x) * gamma(1 - x) = pi / sin(pi x)
        return (std::f64::consts::PI / (std::f64::consts::PI * x).sin().abs()).ln()
            - ln_gamma(1.0 - x);
    }
    let x = x - 1.0;
    let mut a = C[0];
    for (i, c) in C.iter().enumerate().skip(1) {
        a += c / (x + i as f64);
    }
    let t = x + G + 0.5;
    0.5 * (2.0 * std::f64::consts::PI).ln() + (x + 0.5) * t.ln() - t + a.ln()
}

/// The digamma function: the reflection formula for a negative `x`, the recurrence
/// `digamma(x) = digamma(x + 1) - 1 / x` up to 10, and the asymptotic series above. The poles are `NaN`.
fn digamma(x: f64) -> f64 {
    if x.is_nan() || (x <= 0.0 && x == x.floor()) {
        return f64::NAN;
    }
    if x < 0.0 {
        // digamma(1 - x) - digamma(x) = pi / tan(pi x)
        return digamma(1.0 - x) - std::f64::consts::PI / (std::f64::consts::PI * x).tan();
    }
    let (mut x, mut shift) = (x, 0.0);
    while x < 10.0 {
        shift -= 1.0 / x;
        x += 1.0;
    }
    let inv = 1.0 / x;
    let inv2 = inv * inv;
    let series = inv2
        * (1.0 / 12.0
            - inv2
                * (1.0 / 120.0
                    - inv2 * (1.0 / 252.0 - inv2 * (1.0 / 240.0 - inv2 * (1.0 / 132.0)))));
    shift + x.ln() - 0.5 * inv - series
}

/// The names in a directory, sorted.
fn list_dir(path: &str) -> std::io::Result<Vec<String>> {
    let mut names = Vec::new();
    for entry in std::fs::read_dir(path)? {
        names.push(entry?.file_name().to_string_lossy().into_owned());
    }
    names.sort();
    Ok(names)
}

/// The files under `dir`, as `prefix/name/...`. A link to a directory is listed as a file, not followed,
/// so a loop of links cannot make it run for ever.
fn walk_dir(dir: &std::path::Path, prefix: &str, out: &mut Vec<String>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let shown = format!("{}/{name}", prefix.trim_end_matches('/'));
        if entry.file_type()?.is_dir() {
            walk_dir(&entry.path(), &shown, out)?;
        } else {
            out.push(shown);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_log_line_is_json_or_text_and_keeps_its_own_keys() {
        let fields = vec![
            ("entries".to_string(), "12".to_string()),
            ("level".to_string(), "fake".to_string()),
        ];
        let json: serde_json::Value = serde_json::from_str(&super::log_line(
            true,
            "info",
            "cache cleared",
            &fields,
            "2026-10-19T00:00:00.000Z",
        ))
        .unwrap();
        assert_eq!(json["level"], "info");
        assert_eq!(json["message"], "cache cleared");
        assert_eq!(json["entries"], "12");
        assert_eq!(json["ts"], "2026-10-19T00:00:00.000Z");
        assert_eq!(
            super::log_line(false, "warn", "cache cleared", &fields, ""),
            "warn cache cleared entries=12"
        );
    }

    use super::*;
    use std::panic::{AssertUnwindSafe, catch_unwind};

    /// The checker's table of std functions and the runtime's implementations are written apart
    /// and must agree: a function the checker accepts has to exist when the program runs.
    #[test]
    fn every_std_function_the_checker_accepts_is_implemented() {
        let interp = Interp::new(telvax_hir::hir::Program::default()).expect("an empty program");
        // Called with no arguments, a function that exists complains about them or panics on the
        // missing one; only a function the runtime does not know says it is not implemented.
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let mut missing = Vec::new();
        let mut checked = 0;
        for path in telvax_hir::stdlib::function_paths() {
            checked += 1;
            let outcome = catch_unwind(AssertUnwindSafe(|| {
                call_function(&interp, path, Default::default(), &Ty::Unknown)
            }));
            if let Ok(Err(e)) = outcome
                && e.to_string().contains("is not implemented by the runtime")
            {
                missing.push(path);
            }
        }
        std::panic::set_hook(previous);
        assert!(checked > 50, "only {checked} std functions were listed");
        assert!(
            missing.is_empty(),
            "the checker accepts these std functions but the runtime has no code for them: {missing:#?}"
        );
    }

    #[test]
    fn a_function_the_checker_does_not_know_is_reported_as_missing() {
        // The test above relies on this message; keep it honest.
        let interp = Interp::new(telvax_hir::hir::Program::default()).unwrap();
        let e = call_function(
            &interp,
            "std::nowhere::nothing",
            Default::default(),
            &Ty::Unknown,
        )
        .expect_err("no such function");
        assert!(e.to_string().contains("is not implemented by the runtime"));
        let e = call_function(
            &interp,
            "std::tensor::nothing",
            Default::default(),
            &Ty::Unknown,
        );
        assert!(e.is_err());
    }

    #[test]
    fn gamma_and_digamma_match_known_values() {
        let close = |got: f64, want: f64| {
            assert!(
                (got - want).abs() <= 1e-12 * want.abs().max(1.0),
                "{got} vs {want}"
            );
        };
        // ln_gamma: ln((n - 1)!) for a whole number, and gamma(1/2) = sqrt(pi).
        close(ln_gamma(1.0), 0.0);
        close(ln_gamma(2.0), 0.0);
        close(ln_gamma(5.0), 24f64.ln());
        close(ln_gamma(10.0), 362_880f64.ln());
        close(ln_gamma(0.5), std::f64::consts::PI.sqrt().ln());
        // The reflection: gamma(-1/2) = -2 sqrt(pi), and the logarithm is of the absolute value.
        close(ln_gamma(-0.5), (2.0 * std::f64::consts::PI.sqrt()).ln());
        // Large arguments do not overflow, as gamma(171) already would: ln(170!) = 706.5730622...
        close(ln_gamma(171.0), 706.573_062_245_787_5);
        close(ln_gamma(1e6), 12_815_504.569_147_611);
        assert_eq!(ln_gamma(0.0), f64::INFINITY);
        assert_eq!(ln_gamma(-3.0), f64::INFINITY);
        assert!(ln_gamma(f64::NAN).is_nan());
        // digamma(1) = -euler_gamma, digamma(n + 1) = digamma(n) + 1/n, digamma(1/2) = -euler_gamma - 2 ln 2.
        let euler = 0.577_215_664_901_532_9;
        close(digamma(1.0), -euler);
        close(digamma(2.0), 1.0 - euler);
        close(digamma(3.0), 1.5 - euler);
        close(digamma(0.5), -euler - 2.0 * 2f64.ln());
        close(digamma(100.0), 4.600_161_852_738_087);
        // The reflection for a negative argument: digamma(-1/2) = digamma(1/2) + pi * tan(pi/2)... = 0.03648997...
        close(digamma(-0.5), 0.036_489_973_978_576_52);
        assert!(digamma(0.0).is_nan() && digamma(-2.0).is_nan() && digamma(f64::NAN).is_nan());
    }

    #[test]
    fn io_needs_its_capability() {
        use crate::{Capability, Caps};
        let none =
            Interp::with_caps(telvax_hir::hir::Program::default(), false, Caps::NONE).unwrap();
        let path = Value::str("/tmp/telvax-caps-test");
        for function in ["std::fs::exists", "std::fs::read", "std::table::read_csv"] {
            let e = call_function(&none, function, vec![path.clone()].into(), &Ty::Unknown)
                .expect_err(function);
            assert!(e.to_string().contains("`fs.read` capability"), "{e}");
        }
        let e = call_function(
            &none,
            "std::env::get",
            vec![Value::str("HOME")].into(),
            &Ty::Unknown,
        )
        .expect_err("env");
        assert!(e.to_string().contains("--allow env"), "{e}");
        // What touches nothing outside the program is never refused, and neither is a client of
        // the offline stand-ins.
        assert!(
            call_function(
                &none,
                "std::math::abs",
                vec![Value::Int(-1)].into(),
                &Ty::Unknown
            )
            .is_ok()
        );
        assert!(none.require("std::ai::llm::OpenAIClient::new").is_ok());
        let only_read = Caps::parse("fs.read").unwrap();
        let some =
            Interp::with_caps(telvax_hir::hir::Program::default(), false, only_read).unwrap();
        assert!(some.require("std::fs::read").is_ok());
        let e = some.require("std::fs::write").unwrap_err();
        assert!(e.to_string().contains("`fs.write`"), "{e}");
        assert!(only_read.allows(Capability::FsRead));
        // The listing reads, the others write, and `copy` does both.
        assert!(some.require("std::fs::list_dir").is_ok() && some.require("std::fs::walk").is_ok());
        for function in ["std::fs::create_dir", "std::fs::remove", "std::fs::copy"] {
            let e = some.require(function).unwrap_err();
            assert!(e.to_string().contains("`fs.write`"), "{function}: {e}");
        }
        let only_write = Caps::parse("fs.write").unwrap();
        let some =
            Interp::with_caps(telvax_hir::hir::Program::default(), false, only_write).unwrap();
        let e = some.require("std::fs::copy").unwrap_err();
        assert!(e.to_string().contains("`fs.read`"), "{e}");
        assert!(some.require("std::fs::create_dir").is_ok());
    }
}
