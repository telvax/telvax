//! Runtime values and their JSON conversion.

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex, RwLock};

use serde_json::{Map, Number, Value as Json};
use telvax_hir::hir::{self, LocalId};
use telvax_hir::{Prim, Ty};

pub type Env = HashMap<LocalId, Value>;

#[derive(Debug, Clone)]
pub struct StructVal {
    pub name: String,
    pub fields: Vec<(String, Value)>,
}

#[derive(Debug, Clone)]
pub struct EnumVal {
    pub name: String,
    pub variant: String,
    /// Where the enum declares the variant: the first is 0. It orders the keys of a `Map`.
    pub index: u32,
    pub fields: Vec<Value>,
}

/// Orders two numbers, strings or bools (numbers compare as numbers, `NaN` last).
pub fn compare(a: &Value, b: &Value) -> Result<std::cmp::Ordering, String> {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Ok(x.cmp(y)),
        (Value::Float(x), Value::Float(y)) => Ok(x.total_cmp(y)),
        (Value::Int(x), Value::Float(y)) => Ok((*x as f64).total_cmp(y)),
        (Value::Float(x), Value::Int(y)) => Ok(x.total_cmp(&(*y as f64))),
        (Value::Str(x), Value::Str(y)) => Ok(x.cmp(y)),
        (Value::Bool(x), Value::Bool(y)) => Ok(x.cmp(y)),
        _ => Err(format!(
            "cannot compare {} with {}: this needs numbers, strings or bools",
            a.type_name(),
            b.type_name()
        )),
    }
}

/// A map key: strings, integers, booleans and the variants of an enum that hold no values (what
/// the checker allows), ordered so that iteration and JSON output are deterministic.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum MapKey {
    Bool(bool),
    Int(i64),
    Str(Arc<str>),
    /// The enum and the variant.
    /// The enum, where it declares the variant, and the variant: keys go in the order declared.
    Variant(Arc<str>, u32, Arc<str>),
}

impl MapKey {
    pub fn from_value(v: &Value) -> Result<MapKey, String> {
        match v {
            Value::Bool(b) => Ok(MapKey::Bool(*b)),
            Value::Int(i) => Ok(MapKey::Int(*i)),
            Value::Str(s) => Ok(MapKey::Str(s.clone())),
            Value::Enum(e) if e.name != "Option" && e.fields.is_empty() => Ok(MapKey::Variant(
                e.name.as_str().into(),
                e.index,
                e.variant.as_str().into(),
            )),
            Value::Enum(e) if e.name != "Option" => Err(format!(
                "a Map key that is an enum cannot have values, found {v}"
            )),
            other => Err(format!(
                "a Map key must be a Text, an Int, a Bool or a variant without values, found {}",
                other.type_name()
            )),
        }
    }

    pub fn to_value(&self) -> Value {
        match self {
            MapKey::Bool(b) => Value::Bool(*b),
            MapKey::Int(i) => Value::Int(*i),
            MapKey::Str(s) => Value::Str(s.clone()),
            MapKey::Variant(name, index, variant) => Value::Enum(Arc::new(EnumVal {
                name: name.to_string(),
                variant: variant.to_string(),
                index: *index,
                fields: Vec::new(),
            })),
        }
    }

    /// JSON objects only have string keys.
    pub fn json_key(&self) -> String {
        match self {
            MapKey::Bool(b) => b.to_string(),
            MapKey::Int(i) => i.to_string(),
            MapKey::Str(s) => s.to_string(),
            MapKey::Variant(_, _, variant) => variant.to_string(),
        }
    }
}

/// A `Map<K, V>`: a reference value like `Vec`, so copies share the same entries.
#[derive(Debug, Default)]
pub struct MapVal(RwLock<std::collections::BTreeMap<MapKey, Value>>);

impl MapVal {
    fn read(&self) -> std::sync::RwLockReadGuard<'_, std::collections::BTreeMap<MapKey, Value>> {
        self.0.read().unwrap_or_else(|e| e.into_inner())
    }

    fn write(&self) -> std::sync::RwLockWriteGuard<'_, std::collections::BTreeMap<MapKey, Value>> {
        self.0.write().unwrap_or_else(|e| e.into_inner())
    }

    pub fn insert(&self, k: MapKey, v: Value) {
        self.write().insert(k, v);
    }

    pub fn get(&self, k: &MapKey) -> Option<Value> {
        self.read().get(k).cloned()
    }

    pub fn remove(&self, k: &MapKey) -> Option<Value> {
        self.write().remove(k)
    }

    pub fn contains(&self, k: &MapKey) -> bool {
        self.read().contains_key(k)
    }

    pub fn len(&self) -> usize {
        self.read().len()
    }

    pub fn is_empty(&self) -> bool {
        self.read().is_empty()
    }

    /// A copy of the entries in key order.
    pub fn entries(&self) -> Vec<(MapKey, Value)> {
        self.read()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }
}

/// Shared, mutable element storage of a `Vec`.
#[derive(Debug, Default)]
pub struct VecVal(RwLock<Vec<Value>>);

impl VecVal {
    pub fn new(items: Vec<Value>) -> Self {
        Self(RwLock::new(items))
    }

    fn read(&self) -> std::sync::RwLockReadGuard<'_, Vec<Value>> {
        self.0.read().unwrap_or_else(|e| e.into_inner())
    }

    pub fn len(&self) -> usize {
        self.read().len()
    }

    pub fn is_empty(&self) -> bool {
        self.read().is_empty()
    }

    pub fn get(&self, i: usize) -> Option<Value> {
        self.read().get(i).cloned()
    }

    pub fn pop(&self) -> Option<Value> {
        self.0.write().unwrap_or_else(|e| e.into_inner()).pop()
    }

    pub fn reverse(&self) {
        self.0.write().unwrap_or_else(|e| e.into_inner()).reverse();
    }

    /// Sorts numbers or strings in place (numbers compare as numbers, `NaN` last).
    pub fn sort(&self) -> Result<(), String> {
        let mut items = self.0.write().unwrap_or_else(|e| e.into_inner());
        let ordering = |a: &Value, b: &Value| match (a, b) {
            (Value::Int(x), Value::Int(y)) => Ok(x.cmp(y)),
            (Value::Float(x), Value::Float(y)) => Ok(x.total_cmp(y)),
            (Value::Int(x), Value::Float(y)) => Ok((*x as f64).total_cmp(y)),
            (Value::Float(x), Value::Int(y)) => Ok(x.total_cmp(&(*y as f64))),
            (Value::Str(x), Value::Str(y)) => Ok(x.cmp(y)),
            (Value::Bool(x), Value::Bool(y)) => Ok(x.cmp(y)),
            _ => Err(format!(
                "cannot sort {} against {}: `sort` needs numbers, strings or bools",
                a.type_name(),
                b.type_name()
            )),
        };
        let mut failure = None;
        items.sort_by(|a, b| {
            ordering(a, b).unwrap_or_else(|e| {
                failure.get_or_insert(e);
                std::cmp::Ordering::Equal
            })
        });
        failure.map_or(Ok(()), Err)
    }

    /// Appends to the shared storage, so every copy of this `Vec` sees the new element.
    pub fn push(&self, v: Value) {
        self.0.write().unwrap_or_else(|e| e.into_inner()).push(v);
    }

    /// Replaces element `i`; `false` when out of bounds.
    pub fn set(&self, i: usize, v: Value) -> bool {
        match self.0.write().unwrap_or_else(|e| e.into_inner()).get_mut(i) {
            Some(slot) => {
                *slot = v;
                true
            }
            None => false,
        }
    }

    /// A copy of the current elements.
    pub fn snapshot(&self) -> Vec<Value> {
        self.read().clone()
    }
}

/// A dense tensor: a flat Arrow array (row-major) plus its shape. Cloning shares the buffer.
#[derive(Debug, Clone)]
pub struct TensorVal {
    pub shape: Vec<usize>,
    pub array: arrow::array::ArrayRef,
}

/// A columnar table; its columns follow the fields of the struct named `schema`.
#[derive(Debug, Clone)]
pub struct TableVal {
    pub schema: String,
    pub batch: arrow::record_batch::RecordBatch,
}

#[derive(Debug)]
pub struct Closure {
    pub params: Vec<LocalId>,
    pub body: hir::Expr,
    pub env: Env,
}

#[derive(Debug, Clone)]
pub enum Value {
    Unit,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(Arc<str>),
    /// Vectors are reference values: copies of a `Vec` share the same elements.
    Vec(Arc<VecVal>),
    Struct(Arc<StructVal>),
    Enum(Arc<EnumVal>),
    Map(Arc<MapVal>),
    /// Two or more values; the fields are numbered (`pair.0`).
    Tuple(Arc<Vec<Value>>),
    Tensor(Arc<TensorVal>),
    Table(Arc<TableVal>),
    Ok(Box<Value>),
    Err(Box<Value>),
    Status(u16, Arc<str>),
    Closure(Arc<Closure>),
    /// A reference to a top-level function, by name (callable, and shippable to other machines).
    Func(Arc<str>),
    /// What an `async fn` gives: work that has not run yet. `await` runs it.
    Task(Arc<TaskVal>),
    Native(Native),
}

/// The work a `Task` stands for.
#[derive(Debug)]
pub enum TaskVal {
    /// A call of an `async fn`, with its arguments.
    Call { name: String, args: Vec<Value> },
    /// `std.task.sleep(ms)`
    Sleep(u64),
    /// `std.task.gather(tasks)`: every task at the same time, the values in the same order.
    Gather(Vec<Value>),
}

impl Value {
    pub fn some(v: Value) -> Value {
        Value::option(Some(v))
    }

    pub fn none() -> Value {
        Value::option(None)
    }

    /// `Option<T>` is an enum named `Option` with the variants `Some` and `None`.
    pub fn option(v: Option<Value>) -> Value {
        Value::Enum(Arc::new(EnumVal {
            name: "Option".into(),
            variant: if v.is_some() { "Some" } else { "None" }.into(),
            index: u32::from(v.is_none()),
            fields: v.into_iter().collect(),
        }))
    }

    pub fn map(entries: Vec<(MapKey, Value)>) -> Value {
        let m = MapVal::default();
        for (k, v) in entries {
            m.insert(k, v);
        }
        Value::Map(Arc::new(m))
    }
}

/// The method, path and headers of an HTTP request. Header names are in lower case.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RequestInfo {
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
}

/// A value for a `$n` place of a SQL statement.
#[derive(Debug)]
pub(crate) enum PgParam {
    Null,
    Text(String),
    Int(i64),
    Float(f64),
    Bool(bool),
}

/// A PostgreSQL connection, opened by the first statement.
pub struct PgClient {
    pub url: String,
    #[cfg(feature = "live")]
    pub(crate) conn: Mutex<Option<postgres::Client>>,
}

impl PgClient {
    pub fn new(url: &str) -> Self {
        Self {
            url: url.to_string(),
            #[cfg(feature = "live")]
            conn: Mutex::new(None),
        }
    }
}

impl std::fmt::Debug for PgClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PostgresClient")
    }
}

/// The answer of an endpoint: a status, headers and a body.
#[derive(Debug, Clone)]
pub struct ResponseVal {
    pub status: i64,
    pub headers: Vec<(String, String)>,
    pub body: Value,
    /// For `Response.events(producer)`: the function that writes the events. The body is not used.
    pub events: Option<Value>,
}

/// Where `Sink` writes: gives the text of one event to the connection and says whether the client is
/// still there.
pub type EventSink = Arc<dyn Fn(String) -> bool + Send + Sync>;

/// The object a `Response.events` producer receives.
#[derive(Clone)]
pub struct SinkVal(pub EventSink);

impl std::fmt::Debug for SinkVal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Sink")
    }
}

/// Objects provided by the runtime's standard library.
#[derive(Debug, Clone)]
pub enum Native {
    /// `endpoint` is the Ollama host when running live; `None` uses the offline stand-in.
    Embedder {
        model: Arc<str>,
        endpoint: Option<Arc<str>>,
    },
    /// `endpoint` is `(base URL, API key)` when running live.
    Llm {
        model: Arc<str>,
        endpoint: Option<Arc<(String, String)>>,
    },
    Db(Arc<Db>),
    Query(Arc<Query>),
    /// A SQL connection to PostgreSQL (`std.db.postgres.PostgresClient`).
    Postgres(Arc<PgClient>),
    /// The request an `@endpoint` function is answering.
    Request(Arc<RequestInfo>),
    /// An answer of an `@endpoint` function with its status and headers.
    Response(Arc<ResponseVal>),
    /// Writes the events of a `Response.events` stream.
    Sink(SinkVal),
}

#[derive(Debug)]
pub struct Doc {
    pub id: String,
    pub text: String,
    pub vector: Vec<f32>,
}

/// In-memory stand-in for a pgvector database.
#[derive(Debug, Default)]
pub struct Db {
    pub url: String,
    pub collections: Mutex<HashMap<String, Vec<Doc>>>,
    /// Set when the database is a real PostgreSQL server (live mode).
    #[cfg(feature = "live")]
    pub(crate) pg: Option<crate::services::PgStore>,
}

impl Db {
    pub fn insert(
        &self,
        collection: &str,
        id: &str,
        text: &str,
        vector: Vec<f32>,
    ) -> Result<(), crate::interp::RuntimeError> {
        #[cfg(feature = "live")]
        if let Some(pg) = &self.pg {
            return pg.insert(collection, id, text, &vector);
        }
        let mut cols = self.collections.lock().unwrap_or_else(|e| e.into_inner());
        let docs = cols.entry(collection.to_string()).or_default();
        // Upsert by id, like a primary-key conflict would.
        docs.retain(|d| d.id != id);
        docs.push(Doc {
            id: id.to_string(),
            text: text.to_string(),
            vector,
        });
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct Query {
    pub db: Arc<Db>,
    pub collection: String,
    pub vector: Option<Vec<f32>>,
    pub limit: usize,
}

impl Value {
    pub fn str(s: impl Into<Arc<str>>) -> Value {
        Value::Str(s.into())
    }

    pub fn vec(items: Vec<Value>) -> Value {
        Value::Vec(Arc::new(VecVal::new(items)))
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Unit => "None",
            Value::Bool(_) => "Bool",
            Value::Int(_) => "Int",
            Value::Float(_) => "Float",
            Value::Str(_) => "Text",
            Value::Vec(_) => "List",
            Value::Struct(_) => "type",
            Value::Enum(e) if e.name == "Option" => "Option",
            Value::Enum(_) => "enum",
            Value::Map(_) => "Map",
            Value::Tuple(_) => "tuple",
            Value::Tensor(_) => "Tensor",
            Value::Table(_) => "Table",
            Value::Ok(_) | Value::Err(_) => "Result",
            Value::Status(..) => "Status",
            Value::Closure(_) | Value::Func(_) => "function",
            Value::Task(_) => "Task",
            Value::Native(_) => "object",
        }
    }

    /// Structural equality; closures and native objects are never equal.
    pub fn equals(&self, other: &Value) -> bool {
        match (self, other) {
            (Value::Unit, Value::Unit) => true,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Int(a), Value::Int(b)) => a == b,
            (Value::Float(a), Value::Float(b)) => a == b,
            (Value::Int(a), Value::Float(b)) | (Value::Float(b), Value::Int(a)) => *a as f64 == *b,
            (Value::Str(a), Value::Str(b)) => a == b,
            (Value::Vec(a), Value::Vec(b)) => {
                let (a, b) = (a.snapshot(), b.snapshot());
                a.len() == b.len() && a.iter().zip(&b).all(|(x, y)| x.equals(y))
            }
            (Value::Struct(a), Value::Struct(b)) => {
                a.name == b.name
                    && a.fields.len() == b.fields.len()
                    && a.fields
                        .iter()
                        .zip(&b.fields)
                        .all(|((n1, v1), (n2, v2))| n1 == n2 && v1.equals(v2))
            }
            (Value::Enum(a), Value::Enum(b)) => {
                a.name == b.name
                    && a.variant == b.variant
                    && a.fields.iter().zip(&b.fields).all(|(x, y)| x.equals(y))
            }
            (Value::Tuple(a), Value::Tuple(b)) => {
                a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| x.equals(y))
            }
            (Value::Map(a), Value::Map(b)) => {
                let (a, b) = (a.entries(), b.entries());
                a.len() == b.len()
                    && a.iter()
                        .zip(&b)
                        .all(|((k1, v1), (k2, v2))| k1 == k2 && v1.equals(v2))
            }
            (Value::Tensor(a), Value::Tensor(b)) => a == b,
            (Value::Table(a), Value::Table(b)) => a.schema == b.schema && a.batch == b.batch,
            (Value::Ok(a), Value::Ok(b)) | (Value::Err(a), Value::Err(b)) => a.equals(b),
            (Value::Status(a, m), Value::Status(b, n)) => a == b && m == n,
            _ => false,
        }
    }

    pub fn to_json(&self) -> Json {
        match self {
            Value::Unit => Json::Null,
            Value::Bool(b) => Json::Bool(*b),
            Value::Int(i) => Json::from(*i),
            Value::Float(f) => Number::from_f64(*f).map_or(Json::Null, Json::Number),
            Value::Str(s) => Json::String(s.to_string()),
            Value::Vec(items) => Json::Array(items.snapshot().iter().map(Value::to_json).collect()),
            Value::Struct(s) => {
                let mut m = Map::new();
                for (k, v) in &s.fields {
                    m.insert(k.clone(), v.to_json());
                }
                Json::Object(m)
            }
            // `Option<T>` is `null` for `None` and the bare value for `Some`.
            Value::Enum(e) if e.name == "Option" => match e.fields.first() {
                Some(v) => v.to_json(),
                None => Json::Null,
            },
            Value::Tuple(items) => Json::Array(items.iter().map(Value::to_json).collect()),
            Value::Map(m) => Json::Object(
                m.entries()
                    .into_iter()
                    .map(|(k, v)| (k.json_key(), v.to_json()))
                    .collect(),
            ),
            Value::Enum(e) => match e.fields.as_slice() {
                [] => Json::String(e.variant.clone()),
                [one] => serde_json::json!({ e.variant.clone(): one.to_json() }),
                many => serde_json::json!({
                    e.variant.clone(): many.iter().map(Value::to_json).collect::<Vec<_>>()
                }),
            },
            Value::Tensor(t) => {
                let single = matches!(t.dtype(), arrow::datatypes::DataType::Float32);
                tensor_json(&t.shape, &t.to_f64_vec(), single)
            }
            Value::Table(t) => t.to_json(),
            Value::Ok(v) => v.to_json(),
            Value::Err(v) => serde_json::json!({ "error": v.to_json() }),
            Value::Status(code, msg) => serde_json::json!({ "status": code, "message": &**msg }),
            Value::Closure(_) | Value::Func(_) => Json::String("<function>".into()),
            Value::Task(_) => Json::String("<task>".into()),
            Value::Native(_) => Json::String("<object>".into()),
        }
    }
}

fn tensor_json(shape: &[usize], data: &[f64], single: bool) -> Json {
    match shape.split_first() {
        None => {
            let x = data[0];
            // f32 values print with their shortest decimal form, not widened f64 noise.
            let x = if single {
                format!("{}", x as f32).parse().unwrap_or(x)
            } else {
                x
            };
            Number::from_f64(x).map_or(Json::Null, Json::Number)
        }
        Some((&n, rest)) => {
            let chunk: usize = rest.iter().product();
            Json::Array(
                (0..n)
                    .map(|i| tensor_json(rest, &data[i * chunk..(i + 1) * chunk], single))
                    .collect(),
            )
        }
    }
}

impl Value {
    /// The text of the values that are not data: tensors, tables, functions, services.
    pub(crate) fn fallback_text(&self) -> String {
        match self {
            Value::Tensor(t) => format!("Tensor{:?}", t.shape),
            Value::Table(t) => format!("Table[{} rows]", t.rows()),
            Value::Status(code, msg) => format!("{code}: {msg}"),
            Value::Closure(_) => "<function>".to_string(),
            Value::Task(_) => "<task>".to_string(),
            Value::Func(name) => format!("<fn {name}>"),
            Value::Native(_) => "<object>".to_string(),
            other => other.to_string(),
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&crate::show::show(
            self,
            None,
            &crate::show::TypeEnv::default(),
            false,
        ))
    }
}

/// Converts request JSON into a runtime value of static type `ty`.
pub fn from_json(
    ty: &Ty,
    json: &Json,
    structs: &HashMap<String, Vec<(String, Ty)>>,
    enums: &HashMap<String, Vec<(String, Vec<Ty>)>>,
) -> Result<Value, String> {
    let bad = |what: &str| format!("expected {what}, found {json}");
    match ty {
        Ty::Prim(Prim::Bool) => json.as_bool().map(Value::Bool).ok_or_else(|| bad("bool")),
        Ty::Prim(p) if p.is_text() => json.as_str().map(Value::str).ok_or_else(|| bad(p.name())),
        Ty::Prim(p) if p.is_float() => json
            .as_f64()
            .map(Value::Float)
            .ok_or_else(|| bad("a number")),
        Ty::Prim(_) => json
            .as_i64()
            .map(Value::Int)
            .ok_or_else(|| bad("an integer")),
        Ty::Vec(inner) => {
            let items = json.as_array().ok_or_else(|| bad("an array"))?;
            items
                .iter()
                .map(|j| from_json(inner, j, structs, enums))
                .collect::<Result<Vec<_>, _>>()
                .map(Value::vec)
        }
        Ty::Tensor { elem, shape } => {
            let mut dims = Vec::new();
            let mut data = Vec::new();
            flatten(json, shape.len(), 0, &mut dims, &mut data)?;
            let array: arrow::array::ArrayRef = match &**elem {
                Ty::Prim(Prim::F64) => Arc::new(arrow::array::Float64Array::from(data)),
                Ty::Prim(Prim::I32) => Arc::new(arrow::array::Int32Array::from(
                    data.iter().map(|x| *x as i32).collect::<Vec<_>>(),
                )),
                Ty::Prim(Prim::I64) => Arc::new(arrow::array::Int64Array::from(
                    data.iter().map(|x| *x as i64).collect::<Vec<_>>(),
                )),
                _ => Arc::new(arrow::array::Float32Array::from(
                    data.iter().map(|x| *x as f32).collect::<Vec<_>>(),
                )),
            };
            TensorVal::from_array(dims, array)
                .map(|t| Value::Tensor(Arc::new(t)))
                .map_err(|e| e.message)
        }
        Ty::Table(name) => {
            let fields = structs
                .get(name)
                .ok_or_else(|| format!("unknown type `{name}`"))?;
            let items = json.as_array().ok_or_else(|| bad("an array of rows"))?;
            let rows = items
                .iter()
                .map(|j| from_json(&Ty::Struct(name.clone()), j, structs, enums))
                .collect::<Result<Vec<_>, _>>()?;
            crate::table::rows_to_batch(fields, &rows)
                .map(|batch| {
                    Value::Table(Arc::new(TableVal {
                        schema: name.clone(),
                        batch,
                    }))
                })
                .map_err(|e| e.message)
        }
        Ty::Struct(name) => {
            let fields = structs
                .get(name)
                .ok_or_else(|| format!("unknown type `{name}`"))?;
            let obj = json.as_object().ok_or_else(|| bad("an object"))?;
            let mut out = Vec::new();
            for (fname, fty) in fields {
                let v = obj
                    .get(fname)
                    .ok_or_else(|| format!("missing field `{fname}`"))?;
                out.push((
                    fname.clone(),
                    from_json(fty, v, structs, enums).map_err(|e| format!("{fname}: {e}"))?,
                ));
            }
            Ok(Value::Struct(Arc::new(StructVal {
                name: name.clone(),
                fields: out,
            })))
        }
        Ty::Tuple(parts) => {
            let items = json.as_array().ok_or_else(|| bad("an array"))?;
            if items.len() != parts.len() {
                return Err(format!(
                    "expected {} values in a tuple, found {}",
                    parts.len(),
                    items.len()
                ));
            }
            parts
                .iter()
                .zip(items)
                .map(|(t, j)| from_json(t, j, structs, enums))
                .collect::<Result<Vec<_>, _>>()
                .map(|v| Value::Tuple(Arc::new(v)))
        }
        Ty::Option(inner) => {
            if json.is_null() {
                Ok(Value::none())
            } else {
                from_json(inner, json, structs, enums).map(Value::some)
            }
        }
        Ty::Map(key_ty, value_ty) => {
            let obj = json.as_object().ok_or_else(|| bad("an object"))?;
            let mut entries = Vec::with_capacity(obj.len());
            for (k, v) in obj {
                let key = match &**key_ty {
                    Ty::Prim(Prim::Str) => MapKey::Str(k.as_str().into()),
                    Ty::Prim(Prim::Bool) => {
                        MapKey::Bool(k.parse().map_err(|_| format!("`{k}` is not a bool key"))?)
                    }
                    Ty::Prim(_) => MapKey::Int(
                        k.parse()
                            .map_err(|_| format!("`{k}` is not an integer key"))?,
                    ),
                    Ty::Enum(name) => {
                        let at = enums
                            .get(name)
                            .and_then(|vs| vs.iter().position(|(v, f)| v == k && f.is_empty()));
                        let Some(at) = at else {
                            return Err(format!(
                                "`{k}` is not a variant of `{name}` without values"
                            ));
                        };
                        MapKey::Variant(name.as_str().into(), at as u32, k.as_str().into())
                    }
                    _ => MapKey::Str(k.as_str().into()),
                };
                entries.push((
                    key,
                    from_json(value_ty, v, structs, enums).map_err(|e| format!("{k}: {e}"))?,
                ));
            }
            Ok(Value::map(entries))
        }
        Ty::Enum(name) => {
            let variants = enums
                .get(name)
                .ok_or_else(|| format!("unknown enum `{name}`"))?;
            // A unit variant is its name; one with values is `{"Variant": value}` (one value) or
            // `{"Variant": [values..]}` (several).
            let (variant, payload) = match json {
                Json::String(s) => (s.as_str(), None),
                Json::Object(o) if o.len() == 1 => {
                    let (k, v) = o.iter().next().expect("one entry");
                    (k.as_str(), Some(v))
                }
                _ => return Err(bad("a variant name or a single-key object")),
            };
            let (_, tys) = variants
                .iter()
                .find(|(n, _)| n == variant)
                .ok_or_else(|| format!("`{name}` has no variant `{variant}`"))?;
            let values: Vec<&Json> = match (tys.len(), payload) {
                (0, None) => Vec::new(),
                (1, Some(v)) => vec![v],
                (n, Some(Json::Array(items))) if items.len() == n && n > 1 => {
                    items.iter().collect()
                }
                _ => return Err(format!("`{name}.{variant}` needs {} value(s)", tys.len())),
            };
            let fields = tys
                .iter()
                .zip(values)
                .map(|(t, j)| from_json(t, j, structs, enums))
                .collect::<Result<Vec<_>, _>>()?;
            let index = variants.iter().position(|(n, _)| n == variant).unwrap_or(0);
            Ok(Value::Enum(Arc::new(EnumVal {
                name: name.clone(),
                variant: variant.to_string(),
                index: index as u32,
                fields,
            })))
        }
        _ => Ok(generic_from_json(json)),
    }
}

fn flatten(
    json: &Json,
    rank: usize,
    depth: usize,
    dims: &mut Vec<usize>,
    data: &mut Vec<f64>,
) -> Result<(), String> {
    if depth == rank {
        let x = json
            .as_f64()
            .ok_or_else(|| format!("expected a number, found {json}"))?;
        data.push(x);
        return Ok(());
    }
    let items = json
        .as_array()
        .ok_or_else(|| format!("expected an array, found {json}"))?;
    if dims.len() == depth {
        dims.push(items.len());
    } else if dims[depth] != items.len() {
        return Err("tensor rows have different lengths".into());
    }
    items
        .iter()
        .try_for_each(|j| flatten(j, rank, depth + 1, dims, data))
}

fn generic_from_json(json: &Json) -> Value {
    match json {
        Json::Null => Value::Unit,
        Json::Bool(b) => Value::Bool(*b),
        Json::Number(n) => n
            .as_i64()
            .map_or_else(|| Value::Float(n.as_f64().unwrap_or(f64::NAN)), Value::Int),
        Json::String(s) => Value::str(s.as_str()),
        Json::Array(a) => Value::vec(a.iter().map(generic_from_json).collect()),
        Json::Object(o) => Value::Struct(Arc::new(StructVal {
            name: "Object".into(),
            fields: o
                .iter()
                .map(|(k, v)| (k.clone(), generic_from_json(v)))
                .collect(),
        })),
    }
}
