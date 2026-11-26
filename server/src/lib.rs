//! Embedded HTTP layer: turns the `@endpoint` functions of a checked program into an Axum
//! router with routing, request validation and JSON (de)serialization.

use std::collections::{BTreeMap, HashMap};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use axum::Json;
use axum::Router;
use axum::body::Bytes;
use axum::extract::{Query, RawPathParams};
use axum::http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use serde_json::{Value, json};
use telvax_hir::hir::{self, HttpMethod};
use telvax_hir::{Dim, Prim, Ty};
use tokio_stream::StreamExt;

mod forms;
mod layers;
pub mod openapi;

pub use forms::Upload;
pub use layers::{
    Counters, ErrorFormat, Forwarded, Gauges, Kept, LogFormat, LogLevel, Metrics, Middleware,
    MiddlewareAnswer, MiddlewareNext, Options, RateLimit,
};

// ---- schemas -------------------------------------------------------------------------------

/// What a JSON value must look like to be accepted.
#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    Int(Prim),
    Float,
    Bool,
    Str,
    Array(Box<Kind>),
    /// Nested arrays; `None` dimensions accept any length.
    Tensor {
        elem: Box<Kind>,
        shape: Vec<Option<u64>>,
    },
    Object(Schema),
    /// Not statically known; accepted as-is.
    Any,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Schema {
    pub fields: Vec<(String, Kind)>,
    /// The fields a body may leave out, with the value they take then: the default of the field, or
    /// `null` for a `T?`.
    pub defaults: Vec<(String, Value)>,
}

/// One HTTP route generated from an `@endpoint` function.
#[derive(Debug, Clone, PartialEq)]
pub struct Route {
    pub name: String,
    pub method: HttpMethod,
    pub path: String,
    /// The parameters read from `{name}` segments of the path, in order.
    pub path_params: Vec<(String, Kind)>,
    /// Request payload, the parameter that is not in the path; `None` when there is none.
    pub payload: Option<Schema>,
    /// The value the function returns on success (the `T` of `Result[T, E]`); `None` when the
    /// function can answer with a `Response`, which has no type.
    pub response: Option<Kind>,
}

/// Builds the routing table for every endpoint in `program`.
pub fn routes(program: &hir::Program) -> Vec<Route> {
    let structs: HashMap<&str, &hir::StructDef> = program
        .structs
        .iter()
        .map(|s| (s.name.as_str(), s))
        .collect();
    program
        .fns
        .iter()
        .filter_map(|f| {
            let ep = f.endpoint.as_ref()?;
            let path_params = ep
                .path_params
                .iter()
                .filter_map(|n| f.params.iter().find(|p| &p.name == n))
                .map(|p| (p.name.clone(), kind_of(&p.ty, &structs, &mut Vec::new())))
                .collect();
            let payload = f
                .params
                .iter()
                .find(|p| {
                    !ep.path_params.contains(&p.name)
                        && !matches!(&p.ty, Ty::Named(n) if n == "Request")
                })
                .map(|p| match kind_of(&p.ty, &structs, &mut Vec::new()) {
                    Kind::Object(s) => s,
                    _ => Schema::default(),
                });
            let response = match &f.ret {
                Ty::Result(ok, _) => Some(&**ok),
                other => Some(other),
            }
            .filter(|t| !matches!(t, Ty::Named(n) if n == "Response"))
            .map(|t| kind_of(t, &structs, &mut Vec::new()));
            Some(Route {
                name: f.name.clone(),
                method: ep.method,
                path: ep.path.clone(),
                path_params,
                payload,
                response,
            })
        })
        .collect()
}

fn kind_of(ty: &Ty, structs: &HashMap<&str, &hir::StructDef>, stack: &mut Vec<String>) -> Kind {
    match ty {
        Ty::Prim(Prim::Bool) => Kind::Bool,
        Ty::Prim(p) if p.is_text() => Kind::Str,
        Ty::Prim(p) if p.is_float() => Kind::Float,
        Ty::Prim(p) => Kind::Int(*p),
        Ty::Vec(inner) => Kind::Array(Box::new(kind_of(inner, structs, stack))),
        Ty::Tensor { elem, shape } => Kind::Tensor {
            elem: Box::new(kind_of(elem, structs, stack)),
            shape: shape
                .iter()
                .map(|d| match d {
                    Dim::Lit(n) => Some(*n),
                    Dim::Named(_) => None,
                })
                .collect(),
        },
        Ty::Struct(name) => match structs.get(name.as_str()) {
            // Recursive structs cannot be described by a finite schema.
            Some(def) if !stack.contains(name) => {
                stack.push(name.clone());
                let fields = def
                    .fields
                    .iter()
                    .map(|(n, t)| (n.clone(), kind_of(t, structs, stack)))
                    .collect();
                stack.pop();
                let defaults = def
                    .fields
                    .iter()
                    .filter_map(|(n, t)| {
                        let given = def.defaults.iter().find(|(d, _)| d == n);
                        match (given, t) {
                            (Some((_, d)), _) => Some((n.clone(), default_value(d))),
                            (None, Ty::Option(_)) => Some((n.clone(), Value::Null)),
                            _ => None,
                        }
                    })
                    .collect();
                Kind::Object(Schema { fields, defaults })
            }
            _ => Kind::Any,
        },
        _ => Kind::Any,
    }
}

fn default_value(d: &hir::FieldDefault) -> Value {
    match d {
        hir::FieldDefault::Int(n) => json!(n),
        hir::FieldDefault::Float(x) => json!(x),
        hir::FieldDefault::Bool(b) => json!(b),
        hir::FieldDefault::Text(s) => json!(s),
    }
}

// ---- validation ----------------------------------------------------------------------------

/// Puts the default of every field a valid body left out, in objects and in lists of objects.
pub fn fill_defaults(schema: &Schema, value: &mut Value) {
    let Some(obj) = value.as_object_mut() else {
        return;
    };
    for (name, kind) in &schema.fields {
        match obj.get_mut(name) {
            Some(v) => fill_kind(kind, v),
            None => {
                if let Some((_, d)) = schema.defaults.iter().find(|(n, _)| n == name) {
                    obj.insert(name.clone(), d.clone());
                }
            }
        }
    }
}

fn fill_kind(kind: &Kind, value: &mut Value) {
    match kind {
        Kind::Object(s) => fill_defaults(s, value),
        Kind::Array(inner) => {
            if let Some(items) = value.as_array_mut() {
                items.iter_mut().for_each(|i| fill_kind(inner, i));
            }
        }
        _ => {}
    }
}

/// Checks `value` against `schema`, returning one message per problem (empty when valid).
pub fn validate(schema: &Schema, value: &Value) -> Vec<String> {
    let mut errors = Vec::new();
    check_object(schema, value, "payload", &mut errors);
    errors
}

fn check_object(schema: &Schema, value: &Value, path: &str, errors: &mut Vec<String>) {
    let Some(obj) = value.as_object() else {
        errors.push(format!(
            "{path}: expected an object, found {}",
            describe(value)
        ));
        return;
    };
    for (name, kind) in &schema.fields {
        match obj.get(name) {
            Some(v) => check(kind, v, &format!("{path}.{name}"), errors),
            None if schema.defaults.iter().any(|(n, _)| n == name) => {}
            None => errors.push(format!("{path}.{name}: missing required field")),
        }
    }
    for name in obj.keys() {
        if !schema.fields.iter().any(|(n, _)| n == name) {
            errors.push(format!("{path}.{name}: unknown field"));
        }
    }
}

fn check(kind: &Kind, v: &Value, path: &str, errors: &mut Vec<String>) {
    match kind {
        Kind::Any => {}
        Kind::Bool if v.is_boolean() => {}
        Kind::Str if v.is_string() => {}
        Kind::Float if v.is_number() => {}
        Kind::Int(p) => match v {
            Value::Number(n) if n.is_i64() || n.is_u64() => {
                if !fits(*p, n) {
                    errors.push(format!("{path}: {n} is out of range for {}", p.name()));
                }
            }
            _ => errors.push(format!(
                "{path}: expected {}, found {}",
                p.name(),
                describe(v)
            )),
        },
        Kind::Array(inner) => match v.as_array() {
            Some(items) => {
                for (i, item) in items.iter().enumerate() {
                    check(inner, item, &format!("{path}[{i}]"), errors);
                }
            }
            None => errors.push(format!("{path}: expected an array, found {}", describe(v))),
        },
        Kind::Tensor { elem, shape } => check_tensor(elem, shape, v, path, errors),
        Kind::Object(schema) => check_object(schema, v, path, errors),
        Kind::Bool => errors.push(format!("{path}: expected bool, found {}", describe(v))),
        Kind::Str => errors.push(format!("{path}: expected Text, found {}", describe(v))),
        Kind::Float => errors.push(format!("{path}: expected a number, found {}", describe(v))),
    }
}

fn check_tensor(
    elem: &Kind,
    shape: &[Option<u64>],
    v: &Value,
    path: &str,
    errors: &mut Vec<String>,
) {
    let Some((dim, rest)) = shape.split_first() else {
        return check(elem, v, path, errors);
    };
    let Some(items) = v.as_array() else {
        return errors.push(format!("{path}: expected an array, found {}", describe(v)));
    };
    if let Some(n) = dim
        && items.len() as u64 != *n
    {
        errors.push(format!(
            "{path}: expected {n} element(s), found {}",
            items.len()
        ));
        return;
    }
    for (i, item) in items.iter().enumerate() {
        check_tensor(elem, rest, item, &format!("{path}[{i}]"), errors);
    }
}

fn fits(p: Prim, n: &serde_json::Number) -> bool {
    let (lo, hi): (i128, i128) = match p {
        Prim::I8 => (i8::MIN.into(), i8::MAX.into()),
        Prim::I16 => (i16::MIN.into(), i16::MAX.into()),
        Prim::I32 => (i32::MIN.into(), i32::MAX.into()),
        Prim::I64 => (i64::MIN.into(), i64::MAX.into()),
        Prim::U8 => (0, u8::MAX.into()),
        Prim::U16 => (0, u16::MAX.into()),
        Prim::U32 => (0, u32::MAX.into()),
        _ => (0, u64::MAX.into()),
    };
    let v = n
        .as_i64()
        .map(i128::from)
        .or_else(|| n.as_u64().map(i128::from));
    v.is_some_and(|v| (lo..=hi).contains(&v))
}

fn describe(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "an array",
        Value::Object(_) => "an object",
    }
}

/// Query-string values are strings; convert them to the JSON type the schema asks for.
fn coerce_query(schema: &Schema, query: HashMap<String, String>) -> Value {
    let mut out = serde_json::Map::new();
    for (k, raw) in query {
        let kind = schema.fields.iter().find(|(n, _)| *n == k).map(|(_, k)| k);
        let v = match kind {
            Some(Kind::Int(_)) => raw
                .parse::<i64>()
                .map(Value::from)
                .or_else(|_| raw.parse::<u64>().map(Value::from))
                .ok(),
            Some(Kind::Float) => raw.parse::<f64>().map(Value::from).ok(),
            Some(Kind::Bool) => raw.parse::<bool>().map(Value::from).ok(),
            _ => Some(Value::String(raw.clone())),
        };
        out.insert(k, v.unwrap_or(Value::String(raw)));
    }
    Value::Object(out)
}

// ---- router --------------------------------------------------------------------------------

/// An error a handler can return; becomes `{"error": message}` with `status`.
#[derive(Debug, Clone, PartialEq)]
pub struct HandlerError {
    pub status: u16,
    pub message: String,
    /// The reason that goes to the log when `message`, which the client reads, does not say it.
    pub log: Option<String>,
}

impl HandlerError {
    pub fn new(status: u16, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
            log: None,
        }
    }

    /// A `500` that tells the client `Internal server error` and keeps `reason` for the log.
    pub fn internal(reason: impl Into<String>) -> Self {
        Self {
            status: 500,
            message: "Internal server error".to_string(),
            log: Some(reason.into()),
        }
    }
}

type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;

/// The method, path and headers of a request. Header names are in lower case; a header whose value
/// is not text is left out.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RequestParts {
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
    /// The body as it came (empty for `GET` and `DELETE`).
    pub body: Arc<Vec<u8>>,
    /// The text fields of a form body (`application/x-www-form-urlencoded` or `multipart/form-data`).
    pub form: Vec<(String, String)>,
    /// The files of a `multipart/form-data` body.
    pub uploads: Vec<forms::Upload>,
}

impl RequestParts {
    fn new(method: &Method, uri: &Uri, headers: &HeaderMap) -> Self {
        Self {
            method: method.as_str().to_string(),
            path: uri.path().to_string(),
            headers: headers
                .iter()
                .filter_map(|(k, v)| Some((k.as_str().to_string(), v.to_str().ok()?.to_string())))
                .collect(),
            body: Arc::new(Vec::new()),
            form: Vec::new(),
            uploads: Vec::new(),
        }
    }
}

/// What a handler receives: the values of the `{name}` segments of the path (converted to their
/// types and checked), the validated payload (`null` when the endpoint has none) and the request.
#[derive(Debug, Clone, PartialEq)]
pub struct Call {
    pub params: BTreeMap<String, Value>,
    pub payload: Value,
    pub request: RequestParts,
}

/// What a handler answers: a status, headers (a header replaces the default of the same name, such
/// as `content-type`) and a JSON body.
#[derive(Debug, Clone, PartialEq)]
pub struct Reply {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Value,
    /// A body that is sent as it is (`Response.text`, `html`, `bytes`) instead of the JSON `body`.
    pub raw: Option<Vec<u8>>,
    /// A stream of server-sent events instead of the JSON `body`.
    pub events: Option<EventStream>,
}

/// The events of a `text/event-stream` answer, as the text of each one. It is taken by the first
/// response built from it.
#[derive(Debug, Clone)]
pub struct EventStream(Arc<Mutex<Option<tokio::sync::mpsc::Receiver<String>>>>);

impl EventStream {
    pub fn new(receiver: tokio::sync::mpsc::Receiver<String>) -> Self {
        Self(Arc::new(Mutex::new(Some(receiver))))
    }

    fn take(&self) -> Option<tokio::sync::mpsc::Receiver<String>> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).take()
    }
}

impl PartialEq for EventStream {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Reply {
    /// Status 200, no headers.
    pub fn ok(body: Value) -> Self {
        Self {
            status: 200,
            headers: Vec::new(),
            body,
            raw: None,
            events: None,
        }
    }
}

/// Receives the validated request and produces the answer.
pub type Handler = Arc<dyn Fn(Call) -> BoxFuture<Result<Reply, HandlerError>> + Send + Sync>;

/// Wraps an async closure that takes the payload and answers a JSON body as a [`Handler`]; path
/// parameters and the request are ignored.
pub fn handler<F, Fut>(f: F) -> Handler
where
    F: Fn(Value) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<Value, HandlerError>> + Send + 'static,
{
    Arc::new(move |c| {
        let fut = f(c.payload);
        Box::pin(async move { fut.await.map(Reply::ok) })
    })
}

/// Wraps an async closure that takes the whole [`Call`] and answers a JSON body as a [`Handler`].
pub fn call_handler<F, Fut>(f: F) -> Handler
where
    F: Fn(Call) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<Value, HandlerError>> + Send + 'static,
{
    Arc::new(move |c| {
        let fut = f(c);
        Box::pin(async move { fut.await.map(Reply::ok) })
    })
}

/// Wraps an async closure that takes the whole [`Call`] and answers a [`Reply`] as a [`Handler`].
pub fn reply_handler<F, Fut>(f: F) -> Handler
where
    F: Fn(Call) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<Reply, HandlerError>> + Send + 'static,
{
    Arc::new(move |c| Box::pin(f(c)))
}

#[derive(Debug, PartialEq)]
pub enum RouterError {
    /// A route has no registered handler, keyed by function name.
    MissingHandler(String),
    /// Two routes share a method and path.
    Duplicate(String),
}

impl std::fmt::Display for RouterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RouterError::MissingHandler(n) => write!(f, "no handler registered for endpoint `{n}`"),
            RouterError::Duplicate(r) => write!(f, "duplicate route {r}"),
        }
    }
}

impl std::error::Error for RouterError {}

/// A file of the static folder for `GET` or `HEAD` of `uri`. A path that leaves the folder (`..`, a link that
/// points out of it), a folder without an `index.html`, or anything that is not a file is a 404: there is no
/// listing. The answer has the content type of the extension, an `etag` (size and change time) and
/// `cache-control: no-cache`, so a browser asks again and gets a `304` when nothing changed.
async fn static_file(
    dir: &std::path::Path,
    method: &Method,
    uri: &Uri,
    headers: &HeaderMap,
) -> Response {
    if method != Method::GET && method != Method::HEAD {
        return StatusCode::NOT_FOUND.into_response();
    }
    let Some(relative) = static_relative(uri.path()) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let (Ok(root), Ok(mut path)) = (
        std::fs::canonicalize(dir),
        std::fs::canonicalize(dir.join(&relative)),
    ) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if path.is_dir() {
        path = path.join("index.html");
    }
    // After the links are followed the file still has to be inside the folder.
    let inside = std::fs::canonicalize(&path).is_ok_and(|p| p.starts_with(&root));
    let Ok(meta) = tokio::fs::metadata(&path).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if !inside || !meta.is_file() {
        return StatusCode::NOT_FOUND.into_response();
    }
    let changed = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_nanos());
    let etag = format!("\"{:x}-{:x}\"", meta.len(), changed);
    let kind = static_content_type(&path);
    let mut res = if headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.split(',').any(|t| t.trim() == etag || t.trim() == "*"))
    {
        StatusCode::NOT_MODIFIED.into_response()
    } else if method == Method::HEAD {
        let mut res = StatusCode::OK.into_response();
        if let Ok(v) = HeaderValue::from_str(&meta.len().to_string()) {
            res.headers_mut().insert(header::CONTENT_LENGTH, v);
        }
        res
    } else {
        match tokio::fs::read(&path).await {
            Ok(bytes) => (StatusCode::OK, bytes).into_response(),
            Err(_) => return StatusCode::NOT_FOUND.into_response(),
        }
    };
    let h = res.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_static(kind));
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    if let Ok(v) = HeaderValue::from_str(&etag) {
        h.insert(header::ETAG, v);
    }
    res
}

/// The path of a request as a relative path of the folder: percent escapes read, empty parts dropped, and
/// `None` for anything that could leave the folder or name something odd (`..`, `.`, a backslash, a NUL).
fn static_relative(path: &str) -> Option<std::path::PathBuf> {
    let decoded = percent_decode(path)?;
    let mut out = std::path::PathBuf::new();
    for part in decoded.split('/') {
        if part.is_empty() {
            continue;
        }
        if part == ".." || part == "." || part.contains(['\\', '\0']) || part.contains(':') {
            return None;
        }
        out.push(part);
    }
    Some(out)
}

fn percent_decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

fn static_content_type(path: &std::path::Path) -> &'static str {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    match ext.as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "json" | "map" => "application/json",
        "txt" | "md" => "text/plain; charset=utf-8",
        "csv" => "text/csv; charset=utf-8",
        "xml" => "application/xml",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "pdf" => "application/pdf",
        "wasm" => "application/wasm",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        _ => "application/octet-stream",
    }
}

fn reply_response(reply: Reply) -> Response {
    let Ok(status) = StatusCode::from_u16(reply.status) else {
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            json!({ "error": format!("invalid status {}", reply.status) }),
        );
    };
    let mut res = match reply.events.as_ref().and_then(EventStream::take) {
        Some(events) => {
            let stream = tokio_stream::wrappers::ReceiverStream::new(events)
                .map(|e| Ok::<_, std::convert::Infallible>(axum::body::Bytes::from(e)));
            let mut res = (status, axum::body::Body::from_stream(stream)).into_response();
            res.headers_mut().insert(
                header::CONTENT_TYPE,
                HeaderValue::from_static("text/event-stream"),
            );
            res.headers_mut()
                .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
            res
        }
        None => match reply.raw {
            Some(raw) => (status, raw).into_response(),
            None => (status, Json(reply.body)).into_response(),
        },
    };
    for (name, value) in reply.headers {
        match (
            HeaderName::from_bytes(name.as_bytes()),
            HeaderValue::from_str(&value),
        ) {
            (Ok(n), Ok(v)) => {
                res.headers_mut().insert(n, v);
            }
            _ => {
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    json!({ "error": format!("invalid header `{name}`") }),
                );
            }
        }
    }
    res
}

fn error_response(status: StatusCode, body: Value) -> Response {
    layers::own_error(status, body)
}

/// Converts the raw `{name}` values of the URL to the types of the parameters.
fn path_values(route: &Route, raw: &RawPathParams) -> Result<BTreeMap<String, Value>, String> {
    let mut out = BTreeMap::new();
    for (name, kind) in &route.path_params {
        let text = raw
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.to_string())
            .unwrap_or_default();
        let value = match kind {
            Kind::Int(_) => text
                .parse::<i64>()
                .map(Value::from)
                .or_else(|_| text.parse::<u64>().map(Value::from))
                .ok(),
            Kind::Float => text.parse::<f64>().map(Value::from).ok(),
            Kind::Bool => text.parse::<bool>().map(Value::from).ok(),
            _ => Some(Value::String(text.clone())),
        };
        let expected = match kind {
            Kind::Int(p) => p.name(),
            Kind::Float => "a number",
            Kind::Bool => "bool",
            _ => "Text",
        };
        let Some(value) = value else {
            return Err(format!(
                "invalid path parameter {name}: expected {expected}, found `{text}`"
            ));
        };
        let mut errors = Vec::new();
        check(kind, &value, name, &mut errors);
        if let Some(e) = errors.into_iter().next() {
            return Err(format!("invalid path parameter {e}"));
        }
        out.insert(name.clone(), value);
    }
    Ok(out)
}

async fn run(
    route: &Route,
    handlers: &Handler,
    raw: &RawPathParams,
    request: RequestParts,
    mut payload: Value,
    log: Option<LogFormat>,
) -> Response {
    let params = match path_values(route, raw) {
        Ok(p) => p,
        Err(e) => return error_response(StatusCode::BAD_REQUEST, json!({ "error": e })),
    };
    if let Some(schema) = &route.payload {
        let errors = validate(schema, &payload);
        if !errors.is_empty() {
            return error_response(
                StatusCode::BAD_REQUEST,
                json!({ "error": "invalid request", "details": errors }),
            );
        }
        fill_defaults(schema, &mut payload);
    }
    let (method, path) = (request.method.clone(), request.path.clone());
    let id = request
        .headers
        .iter()
        .find(|(k, _)| k == "x-request-id")
        .map(|(_, v)| v.clone());
    // The trace id of the `traceparent` that the observing layer made sure of, for the line of a failure.
    let trace_id = request
        .headers
        .iter()
        .find(|(k, _)| k == "traceparent")
        .and_then(|(_, v)| v.get(3..35))
        .map(str::to_string);
    let call = Call {
        params,
        payload,
        request,
    };
    match handlers(call).await {
        Ok(reply) => reply_response(reply),
        Err(e) => {
            if let Some(reason) = &e.log {
                eprintln!(
                    "{}",
                    layers::failure_line(
                        log,
                        id.as_deref(),
                        trace_id.as_deref(),
                        &method,
                        &path,
                        e.status,
                        reason
                    )
                );
            }
            let status =
                StatusCode::from_u16(e.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
            error_response(status, json!({ "error": e.message }))
        }
    }
}

/// Builds the router. Every route needs a handler keyed by the endpoint function's name.
/// `GET`/`DELETE` read the payload from the query string; other methods from a JSON body.
pub fn router(
    routes: &[Route],
    handlers: BTreeMap<String, Handler>,
) -> Result<Router, RouterError> {
    router_with(routes, handlers, &Options::default())
}

/// Like [`router`], with a request id, metrics and a log line per request as `options` say.
pub fn router_with(
    routes: &[Route],
    mut handlers: BTreeMap<String, Handler>,
    options: &Options,
) -> Result<Router, RouterError> {
    let mut by_path: BTreeMap<String, axum::routing::MethodRouter> = BTreeMap::new();
    let mut seen = std::collections::HashSet::new();
    let log = options.log;

    for route in routes {
        // `/a/{x}` and `/a/{y}` are the same route.
        let shape = route
            .path
            .split('/')
            .map(|seg| if seg.starts_with('{') { "{}" } else { seg })
            .collect::<Vec<_>>()
            .join("/");
        if !seen.insert((route.method, shape)) {
            return Err(RouterError::Duplicate(format!(
                "{:?} {}",
                route.method, route.path
            )));
        }
        let h = handlers
            .remove(&route.name)
            .ok_or_else(|| RouterError::MissingHandler(route.name.clone()))?;
        let route = Arc::new(route.clone());

        let mr = by_path.remove(&route.path).unwrap_or_default();
        let mr = match route.method {
            HttpMethod::Get | HttpMethod::Delete => {
                let r = route.clone();
                let f = move |raw: RawPathParams,
                              method: Method,
                              uri: Uri,
                              headers: HeaderMap,
                              Query(q): Query<HashMap<String, String>>| {
                    let (r, h) = (r.clone(), h.clone());
                    async move {
                        let request = RequestParts::new(&method, &uri, &headers);
                        let payload = match &r.payload {
                            Some(s) => coerce_query(s, q),
                            None => Value::Null,
                        };
                        run(&r, &h, &raw, request, payload, log).await
                    }
                };
                if route.method == HttpMethod::Get {
                    mr.get(f)
                } else {
                    mr.delete(f)
                }
            }
            HttpMethod::Post | HttpMethod::Put | HttpMethod::Patch => {
                let r = route.clone();
                let f = move |raw: RawPathParams,
                              method: Method,
                              uri: Uri,
                              headers: HeaderMap,
                              body: Bytes| {
                    let (r, h) = (r.clone(), h.clone());
                    async move {
                        let mut request = RequestParts::new(&method, &uri, &headers);
                        let content_type = headers
                            .get(header::CONTENT_TYPE)
                            .and_then(|v| v.to_str().ok())
                            .unwrap_or("")
                            .to_string();
                        let kind = content_type.to_ascii_lowercase();
                        // A form: its text fields are the payload, read by the types of the fields as a
                        // query string is, and the files go to the request.
                        let form = if kind.starts_with("application/x-www-form-urlencoded") {
                            Some(forms::parse_urlencoded(&body).map(|f| (f, Vec::new())))
                        } else if kind.starts_with("multipart/form-data") {
                            Some(forms::parse_multipart(&body, &content_type))
                        } else {
                            None
                        };
                        request.body = Arc::new(body.to_vec());
                        let payload = match form {
                            Some(Err(e)) => {
                                return error_response(
                                    StatusCode::BAD_REQUEST,
                                    json!({ "error": format!("invalid form: {e}") }),
                                );
                            }
                            Some(Ok((fields, files))) => {
                                let payload = match &r.payload {
                                    Some(s) => coerce_query(s, fields.iter().cloned().collect()),
                                    None => Value::Null,
                                };
                                request.form = fields;
                                request.uploads = files;
                                payload
                            }
                            None if r.payload.is_none() => Value::Null,
                            None => match serde_json::from_slice::<Value>(&body) {
                                Ok(v) => v,
                                Err(e) => {
                                    return error_response(
                                        StatusCode::BAD_REQUEST,
                                        json!({ "error": format!("invalid JSON: {e}") }),
                                    );
                                }
                            },
                        };
                        run(&r, &h, &raw, request, payload, log).await
                    }
                };
                match route.method {
                    HttpMethod::Post => mr.post(f),
                    HttpMethod::Put => mr.put(f),
                    _ => mr.patch(f),
                }
            }
        };
        by_path.insert(route.path.clone(), mr);
    }

    let draining = options.draining.clone();
    let mut app = Router::new().route(
        "/healthz",
        get(move || {
            let stopping = draining
                .as_ref()
                .is_some_and(|d| d.load(std::sync::atomic::Ordering::Relaxed));
            async move {
                if stopping {
                    (StatusCode::SERVICE_UNAVAILABLE, "stopping")
                } else {
                    (StatusCode::OK, "ok")
                }
            }
        }),
    );
    let document = Arc::new(openapi::document_with(
        "Telvax service",
        routes,
        options.error_format,
    ));
    if !by_path.contains_key("/openapi.json") {
        app = app.route(
            "/openapi.json",
            get(move || {
                let document = document.clone();
                async move { Json((*document).clone()) }
            }),
        );
    }
    if !by_path.contains_key("/docs") {
        app = app.route(
            "/docs",
            get(|| async { axum::response::Html(openapi::DOCS_PAGE) }),
        );
    }
    let metrics = Arc::new(Metrics::default());
    if options.metrics && !by_path.contains_key("/metrics") {
        let (metrics, counters, gauges) = (
            metrics.clone(),
            options.counters.clone(),
            options.gauges.clone(),
        );
        app = app.route(
            "/metrics",
            get(move || {
                let (metrics, counters, gauges) =
                    (metrics.clone(), counters.clone(), gauges.clone());
                async move {
                    let counters = counters.map(|c| c()).unwrap_or_default();
                    let gauges = gauges.map(|g| g()).unwrap_or_default();
                    (
                        [(
                            axum::http::header::CONTENT_TYPE,
                            "text/plain; version=0.0.4; charset=utf-8",
                        )],
                        metrics.render(&counters, &gauges),
                    )
                }
            }),
        );
    }
    for (path, mr) in by_path {
        app = app.route(&path, mr);
    }
    // The files of the static folder answer what no route takes; the layers below apply to them too.
    if let Some(dir) = &options.static_dir {
        let dir = Arc::new(dir.clone());
        app = app.fallback(move |method: Method, uri: Uri, headers: HeaderMap| {
            let dir = dir.clone();
            async move { static_file(&dir, &method, &uri, &headers).await }
        });
    }
    // The middleware of the program come first, so they are inside the built-in ones; the first of the
    // list is added last, so it is the outermost of them.
    for middleware in options.middleware.iter().rev() {
        app = app.layer(axum::middleware::from_fn_with_state(
            middleware.clone(),
            layers::run_middleware,
        ));
    }
    if let Some(limit) = options.rate_limit {
        let limiter = Arc::new(layers::Limiter::new(limit, options.trust_proxy));
        app = app.layer(axum::middleware::from_fn_with_state(
            limiter,
            layers::rate_limit,
        ));
    }
    if let Some(origins) = &options.cors {
        let cors = Arc::new(layers::Cors {
            origins: origins.clone(),
        });
        app = app.layer(axum::middleware::from_fn_with_state(cors, layers::cors));
    }
    if options.security_headers {
        app = app.layer(axum::middleware::from_fn(layers::security_headers));
    }
    if options.error_format == layers::ErrorFormat::Detail {
        app = app.layer(axum::middleware::from_fn(layers::detail_errors));
    }
    if options.observes() {
        let observer = Arc::new(layers::Observer {
            metrics,
            record: options.metrics,
            log: options.log,
            log_level: options.log_level,
            process_time: options.process_time,
        });
        app = app.layer(axum::middleware::from_fn_with_state(
            observer,
            layers::observe,
        ));
    }
    Ok(app)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, header};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    #[tokio::test]
    async fn the_request_gets_a_traceparent_that_the_program_sees() {
        let hir = program(
            "import std.http.Status\n\n@endpoint(GET, \"/t\")\nfn t() -> Result[Int, Status]:\n    Ok(1)\n",
        );
        let mut hs = BTreeMap::new();
        hs.insert(
            "t".to_string(),
            reply_handler(|call| async move {
                let seen = call
                    .request
                    .headers
                    .iter()
                    .find(|(k, _)| k == "traceparent")
                    .map(|(_, v)| v.clone());
                Ok(Reply::ok(json!(seen)))
            }),
        );
        let options = Options {
            log: Some(LogFormat::Json),
            ..Default::default()
        };
        let app = router_with(&routes(&hir), hs, &options).unwrap();
        let ask = |header: Option<&str>| {
            let mut req = Request::builder().uri("/t");
            if let Some(h) = header {
                req = req.header("traceparent", h);
            }
            let app = app.clone();
            async move {
                let res = app.oneshot(req.body(Body::empty()).unwrap()).await.unwrap();
                let bytes = res.into_body().collect().await.unwrap().to_bytes();
                serde_json::from_slice::<Value>(&bytes)
                    .unwrap()
                    .as_str()
                    .unwrap_or("")
                    .to_string()
            }
        };
        let valid = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";
        assert_eq!(ask(Some(valid)).await, valid);
        // No header, or one that is not valid: a new trace, in the W3C form.
        for given in [
            None,
            Some("garbage"),
            Some("00-00000000000000000000000000000000-00f067aa0ba902b7-01"),
        ] {
            let made = ask(given).await;
            let parts: Vec<&str> = made.split('-').collect();
            assert!(
                parts.len() == 4
                    && parts[0] == "00"
                    && parts[1].len() == 32
                    && parts[2].len() == 16,
                "{given:?} -> {made}"
            );
            assert_ne!(parts[1], "00000000000000000000000000000000");
        }
    }

    #[tokio::test]
    async fn a_form_fills_the_payload_and_the_request_keeps_the_fields_and_files() {
        let hir = program(
            "import std.http.Request\nimport std.http.Status\n\ntype Signup:\n    name: Text\n    age: Int\n\n\
             @endpoint(POST, \"/signup\")\nfn signup(s: Signup, req: Request) -> Result[Int, Status]:\n    Ok(s.age)\n",
        );
        let mut hs = BTreeMap::new();
        hs.insert(
            "signup".to_string(),
            reply_handler(|call| async move {
                let files: Vec<Value> = call
                    .request
                    .uploads
                    .iter()
                    .map(|u| json!([u.name, u.filename, u.content_type, u.data]))
                    .collect();
                Ok(Reply::ok(json!({
                    "payload": call.payload,
                    "form": call.request.form,
                    "files": files,
                    "body": call.request.body.len(),
                })))
            }),
        );
        let app = router(&routes(&hir), hs).unwrap();
        let post = |kind: &str, body: Vec<u8>| {
            Request::builder()
                .method("POST")
                .uri("/signup")
                .header(header::CONTENT_TYPE, kind)
                .body(Body::from(body))
                .unwrap()
        };
        let json_of = |res: Response| async move {
            let status = res.status();
            let bytes = res.into_body().collect().await.unwrap().to_bytes();
            (status, serde_json::from_slice::<Value>(&bytes).unwrap())
        };
        // An HTML form: the fields are read by their types, as a query string is.
        let (status, v) = json_of(
            app.clone()
                .oneshot(post(
                    "application/x-www-form-urlencoded",
                    b"name=Ana+Maria&age=31".to_vec(),
                ))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{v}");
        assert_eq!(v["payload"], json!({"name": "Ana Maria", "age": 31}));
        assert_eq!(v["form"], json!([["name", "Ana Maria"], ["age", "31"]]));
        // multipart: the text fields, and the file with its bytes as they were.
        let body = b"--B\r\nContent-Disposition: form-data; name=\"name\"\r\n\r\nBo\r\n\
--B\r\nContent-Disposition: form-data; name=\"age\"\r\n\r\n5\r\n\
--B\r\nContent-Disposition: form-data; name=\"pic\"; filename=\"p.png\"\r\nContent-Type: image/png\r\n\r\n\x89\r\n\x00\r\n--B--\r\n"
            .to_vec();
        let (status, v) = json_of(
            app.clone()
                .oneshot(post("multipart/form-data; boundary=B", body))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{v}");
        assert_eq!(v["payload"], json!({"name": "Bo", "age": 5}));
        assert_eq!(
            v["files"],
            json!([["pic", "p.png", "image/png", [0x89, 13, 10, 0]]])
        );
        // JSON is still JSON, and a field of the wrong type or a broken form is a 400 that says why.
        let (status, v) = json_of(
            app.clone()
                .oneshot(post(
                    "application/json",
                    br#"{"name":"C","age":2}"#.to_vec(),
                ))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(
            (status, &v["payload"]),
            (StatusCode::OK, &json!({"name": "C", "age": 2}))
        );
        let (status, v) = json_of(
            app.clone()
                .oneshot(post(
                    "application/x-www-form-urlencoded",
                    b"name=A&age=old".to_vec(),
                ))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{v}");
        let (status, v) = json_of(
            app.clone()
                .oneshot(post(
                    "application/x-www-form-urlencoded",
                    b"name=%zz&age=1".to_vec(),
                ))
                .await
                .unwrap(),
        )
        .await;
        assert!(
            status == StatusCode::BAD_REQUEST
                && v["error"].as_str().unwrap().contains("invalid form"),
            "{v}"
        );
        let (status, v) = json_of(
            app.oneshot(post("multipart/form-data", b"--x".to_vec()))
                .await
                .unwrap(),
        )
        .await;
        assert!(
            status == StatusCode::BAD_REQUEST
                && v["error"].as_str().unwrap().contains("no boundary"),
            "{v}"
        );
    }

    fn program(src: &str) -> hir::Program {
        let (ast, errs) = telvax_parser_for_tests(src);
        assert!(errs.is_empty(), "{errs:?}");
        ast
    }

    fn telvax_parser_for_tests(src: &str) -> (hir::Program, Vec<String>) {
        let (ast, perrs) = telvax_parser::parse(src);
        assert!(perrs.is_empty(), "{perrs:?}");
        let (hir, diags) = telvax_hir::lower(&ast.unwrap());
        (hir, diags.into_iter().map(|d| d.message).collect())
    }

    const SRC: &str = r#"
import std.http.Request

type QueryPayload:
    query: Text
    top_k: i32

type Embed:
    v: Tensor[f32, 3]
    tags: List[Text]
    small: u8

@endpoint(POST, "/v1/rag/query")
fn handle_rag(p: QueryPayload) -> Result[QueryPayload, Text]:
    Ok(p)

@endpoint(PUT, "/v1/embed")
fn embed(p: Embed) -> Result[Embed, Text]:
    Ok(p)

@endpoint(path: "/v1/search")
fn search(p: QueryPayload) -> Result[QueryPayload, Text]:
    Ok(p)

@endpoint(path: "/v1/ping")
fn ping() -> Result[Text, Text]:
    Ok("pong")

@endpoint(GET, "/v1/results/{job_id}")
fn result(job_id: Text) -> Result[Text, Text]:
    Ok(job_id)

@endpoint(DELETE, "/v1/cache")
fn clear(req: Request) -> Result[Text, Text]:
    Ok(req.header("X-Admin-Token").unwrap_or("none"))

@endpoint(PUT, "/v1/jobs/{id}/steps/{step}")
fn step(id: u8, step: Text, p: QueryPayload) -> Result[QueryPayload, Text]:
    Ok(p)

fn helper():
    pass
"#;

    fn app() -> Router {
        app_with(&Options::default())
    }

    fn app_with(options: &Options) -> Router {
        let hir = program(SRC);
        let echo = || handler(|v| async move { Ok(v) });
        let mut hs = BTreeMap::new();
        hs.insert("handle_rag".to_string(), echo());
        hs.insert("embed".to_string(), echo());
        hs.insert(
            "search".to_string(),
            handler(|v| async move {
                if v["top_k"] == 0 {
                    Err(HandlerError::new(422, "top_k must be positive"))
                } else {
                    Ok(v)
                }
            }),
        );
        hs.insert("ping".to_string(), handler(|_| async { Ok(json!("pong")) }));
        hs.insert(
            "result".to_string(),
            call_handler(|c| async move { Ok(c.params["job_id"].clone()) }),
        );
        hs.insert(
            "clear".to_string(),
            call_handler(|c| async move {
                let token = c
                    .request
                    .headers
                    .iter()
                    .find(|(k, _)| k == "x-admin-token")
                    .map(|(_, v)| v.clone());
                Ok(json!({"method": c.request.method, "path": c.request.path, "token": token}))
            }),
        );
        hs.insert(
            "step".to_string(),
            call_handler(|c| async move {
                Ok(json!({"id": c.params["id"], "step": c.params["step"], "payload": c.payload}))
            }),
        );
        router_with(&routes(&hir), hs, options).unwrap()
    }

    async fn call(app: &Router, method: &str, uri: &str, body: &str) -> (StatusCode, Value) {
        let req = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        let status = res.status();
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    #[test]
    fn routing_table_comes_from_endpoints_only() {
        let rs = routes(&program(SRC));
        let names: Vec<_> = rs.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "handle_rag",
                "embed",
                "search",
                "ping",
                "result",
                "clear",
                "step"
            ]
        );
        assert_eq!(rs[0].method, HttpMethod::Post);
        assert_eq!(
            rs[0].payload.as_ref().unwrap().fields,
            [
                ("query".to_string(), Kind::Str),
                ("top_k".to_string(), Kind::Int(Prim::I32))
            ]
        );
        assert!(rs[3].payload.is_none());
    }

    #[test]
    fn fields_with_a_default_or_an_option_are_not_required() {
        let src = "type Q:\n    text: Text\n    language: Text = \"en\"\n    note: Text?\n\n@endpoint(POST, \"/q\")\nfn q(input: Q) -> Result[Q, Text]:\n    Ok(input)\n";
        let rs = routes(&program(src));
        let schema = rs[0].payload.as_ref().unwrap();
        assert_eq!(
            schema.defaults,
            [
                ("language".to_string(), json!("en")),
                ("note".to_string(), Value::Null)
            ]
        );
        let mut body = json!({"text": "hi"});
        assert!(validate(schema, &body).is_empty());
        fill_defaults(schema, &mut body);
        assert_eq!(body, json!({"text": "hi", "language": "en", "note": null}));
        let doc = openapi::document("t", &rs);
        let s = &doc["paths"]["/q"]["post"]["requestBody"]["content"]["application/json"]["schema"];
        assert_eq!(s["required"], json!(["text"]));
        assert_eq!(s["properties"]["language"]["default"], "en");
    }

    #[test]
    fn rag_example_routes() {
        let src = include_str!("../../examples/rag-service.tvx");
        let rs = routes(&program(src));
        assert_eq!(rs.len(), 1);
        assert_eq!(rs[0].path, "/v1/rag/query");
    }

    #[tokio::test]
    async fn the_openapi_document_describes_the_routes() {
        let (s, doc) = call(&app(), "GET", "/openapi.json", "").await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(doc["openapi"], "3.0.3");

        let rag = &doc["paths"]["/v1/rag/query"]["post"];
        assert_eq!(rag["operationId"], "handle_rag");
        let body = &rag["requestBody"]["content"]["application/json"]["schema"];
        assert_eq!(body["properties"]["query"], json!({"type": "string"}));
        assert_eq!(
            body["properties"]["top_k"],
            json!({"type": "integer", "format": "int32"})
        );
        assert_eq!(body["required"], json!(["query", "top_k"]));
        assert!(rag["responses"]["400"].is_object());
        let ok = &rag["responses"]["200"]["content"]["application/json"]["schema"];
        assert_eq!(ok["properties"]["query"], json!({"type": "string"}));

        let embed = &doc["paths"]["/v1/embed"]["put"]["requestBody"]["content"]["application/json"]
            ["schema"]["properties"];
        assert_eq!(embed["small"]["minimum"], 0);
        assert_eq!(embed["v"]["minItems"], 3);
        assert_eq!(
            embed["tags"],
            json!({"type": "array", "items": {"type": "string"}})
        );

        // A path parameter and a query payload become parameters, not a body.
        let get = &doc["paths"]["/v1/results/{job_id}"]["get"];
        assert_eq!(
            get["parameters"][0],
            json!({"name": "job_id", "in": "path", "required": true, "schema": {"type": "string"}})
        );
        let search = &doc["paths"]["/v1/search"]["get"];
        assert!(search["requestBody"].is_null());
        assert_eq!(search["parameters"][0]["in"], "query");
        // No payload, no 400.
        assert!(doc["paths"]["/v1/ping"]["get"]["responses"]["400"].is_null());
    }

    async fn text(app: &Router, req: Request<Body>) -> (StatusCode, HeaderMap, String) {
        let res = app.clone().oneshot(req).await.unwrap();
        let (status, headers) = (res.status(), res.headers().clone());
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, headers, String::from_utf8(body.to_vec()).unwrap())
    }

    #[tokio::test]
    async fn requests_get_an_id_and_are_counted() {
        let options = Options {
            metrics: true,
            counters: Some(Arc::new(|| vec![("cache_hit".into(), 3)])),
            gauges: Some(Arc::new(|| vec![("cache_size".into(), 1042.0)])),
            ..Options::default()
        };
        let app = app_with(&options);
        let get = |uri: &str| Request::builder().uri(uri).body(Body::empty()).unwrap();

        let (_, headers, _) = text(&app, get("/v1/ping")).await;
        assert_eq!(headers["x-request-id"].len(), 16);
        let sent = Request::builder()
            .uri("/v1/ping")
            .header("x-request-id", "abc-123")
            .body(Body::empty())
            .unwrap();
        assert_eq!(text(&app, sent).await.1["x-request-id"], "abc-123");
        let bad = Request::builder()
            .uri("/v1/ping")
            .header("x-request-id", "has space")
            .body(Body::empty())
            .unwrap();
        assert_ne!(text(&app, bad).await.1["x-request-id"], "has space");

        text(&app, get("/v1/results/7")).await;
        text(&app, get("/nowhere")).await;
        let (status, headers, body) = text(&app, get("/metrics")).await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            headers["content-type"]
                .to_str()
                .unwrap()
                .starts_with("text/plain")
        );
        assert!(
            body.contains("http_requests_total{method=\"GET\",path=\"/v1/ping\",status=\"200\"} 3"),
            "{body}"
        );
        // The route as written, not the URL, so the series do not multiply.
        assert!(
            body.contains("path=\"/v1/results/{job_id}\",status=\"200\"} 1"),
            "{body}"
        );
        assert!(
            body.contains("path=\"unmatched\",status=\"404\"} 1"),
            "{body}"
        );
        assert!(
            body.contains(
                "http_request_duration_seconds_count{method=\"GET\",path=\"/v1/ping\"} 3"
            ),
            "{body}"
        );
        assert!(body.contains("le=\"+Inf\""), "{body}");
        assert!(
            body.contains("# TYPE cache_hit counter\ncache_hit 3\n"),
            "{body}"
        );
        assert!(
            body.contains("# TYPE cache_size gauge\ncache_size 1042\n"),
            "{body}"
        );
        assert!(!body.contains("telvax_"), "{body}");
    }

    fn req(method: &str, uri: &str, headers: &[(&str, &str)]) -> Request<Body> {
        let mut b = Request::builder().method(method).uri(uri);
        for (k, v) in headers {
            b = b.header(*k, *v);
        }
        b.body(Body::empty()).unwrap()
    }

    #[tokio::test]
    async fn cors_headers_go_to_allowed_origins_only() {
        let options = Options {
            cors: Some(vec!["https://a.example".into()]),
            ..Options::default()
        };
        let app = app_with(&options);
        let (_, h, _) = text(
            &app,
            req("GET", "/v1/ping", &[("origin", "https://a.example")]),
        )
        .await;
        assert_eq!(h["access-control-allow-origin"], "https://a.example");
        assert_eq!(h["vary"], "origin");
        let (_, h, _) = text(
            &app,
            req("GET", "/v1/ping", &[("origin", "https://b.example")]),
        )
        .await;
        assert!(h.get("access-control-allow-origin").is_none());

        // A preflight is answered without reaching the route (which has no OPTIONS).
        let pre = req(
            "OPTIONS",
            "/v1/rag/query",
            &[
                ("origin", "https://a.example"),
                ("access-control-request-method", "POST"),
                ("access-control-request-headers", "content-type,x-token"),
            ],
        );
        let (status, h, _) = text(&app, pre).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        assert_eq!(h["access-control-allow-origin"], "https://a.example");
        assert!(
            h["access-control-allow-methods"]
                .to_str()
                .unwrap()
                .contains("POST")
        );
        assert_eq!(h["access-control-allow-headers"], "content-type,x-token");

        let any = app_with(&Options {
            cors: Some(vec!["*".into()]),
            ..Options::default()
        });
        let (_, h, _) = text(
            &any,
            req("GET", "/v1/ping", &[("origin", "https://z.example")]),
        )
        .await;
        assert_eq!(h["access-control-allow-origin"], "*");
        assert!(h.get("vary").is_none());
    }

    #[tokio::test]
    async fn clients_over_the_limit_wait() {
        let app = app_with(&Options {
            rate_limit: Some(RateLimit {
                max: 2,
                window: std::time::Duration::from_secs(60),
            }),
            trust_proxy: true,
            ..Options::default()
        });
        let from = |ip: &'static str| req("GET", "/v1/ping", &[("x-forwarded-for", ip)]);
        assert_eq!(text(&app, from("1.1.1.1, 9.9.9.9")).await.0, StatusCode::OK);
        assert_eq!(text(&app, from("1.1.1.1")).await.0, StatusCode::OK);
        let (status, h, body) = text(&app, from("1.1.1.1")).await;
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
        assert!(h["retry-after"].to_str().unwrap().parse::<u64>().unwrap() >= 1);
        assert!(body.contains("rate limit exceeded"));
        // Another client has its own count, and the health check is never limited.
        assert_eq!(text(&app, from("2.2.2.2")).await.0, StatusCode::OK);
        for _ in 0..5 {
            assert_eq!(
                text(
                    &app,
                    req("GET", "/healthz", &[("x-forwarded-for", "1.1.1.1")])
                )
                .await
                .0,
                StatusCode::OK
            );
        }
    }

    #[test]
    fn a_window_starts_again() {
        let l = layers::Limiter::new(
            RateLimit {
                max: 1,
                window: std::time::Duration::from_secs(10),
            },
            false,
        );
        let t0 = std::time::Instant::now();
        assert_eq!(l.check("c", t0), None);
        assert!(
            l.check("c", t0 + std::time::Duration::from_secs(3))
                .is_some()
        );
        assert_eq!(l.check("c", t0 + std::time::Duration::from_secs(10)), None);
    }

    #[tokio::test]
    async fn security_headers_do_not_replace_the_programs_own() {
        let app = app_with(&Options {
            security_headers: true,
            ..Options::default()
        });
        let (_, h, _) = text(&app, req("GET", "/v1/ping", &[])).await;
        assert_eq!(h["x-content-type-options"], "nosniff");
        assert_eq!(h["x-frame-options"], "DENY");
        assert_eq!(h["referrer-policy"], "no-referrer");
        assert_eq!(h["cache-control"], "no-store");
    }

    #[tokio::test]
    async fn nothing_is_added_unless_asked() {
        let app = app();
        let req = Request::builder()
            .uri("/v1/ping")
            .body(Body::empty())
            .unwrap();
        let (_, headers, _) = text(&app, req).await;
        assert!(headers.get("x-request-id").is_none());
        let req = Request::builder()
            .uri("/metrics")
            .body(Body::empty())
            .unwrap();
        assert_eq!(text(&app, req).await.0, StatusCode::NOT_FOUND);
    }

    #[test]
    fn counters_are_listed_under_their_own_name() {
        let m = Metrics::default();
        let out = m.render(
            &[
                ("jobs_total".to_string(), 5),
                ("plain".to_string(), 2),
                (r#"jobs_total{status="done"}"#.to_string(), 3),
                (r#"jobs_total{status="failed"}"#.to_string(), 1),
            ],
            &[],
        );
        assert!(out.contains("# TYPE plain counter\nplain 2\n"), "{out}");
        // The same name with and without labels has one `# TYPE`.
        assert!(out.contains("jobs_total 5\n"), "{out}");
        assert_eq!(out.matches("# TYPE jobs_total counter").count(), 1, "{out}");
        assert!(out.contains("jobs_total{status=\"done\"} 3\n"), "{out}");
        assert!(out.contains("jobs_total{status=\"failed\"} 1\n"), "{out}");
    }

    #[test]
    fn gauges_are_listed_with_their_own_type() {
        let m = Metrics::default();
        let out = m.render(
            &[],
            &[
                ("cache_size".to_string(), 1042.0),
                (r#"jobs_running{queue="high"}"#.to_string(), 3.0),
                (r#"jobs_running{queue="low"}"#.to_string(), 0.5),
                ("ratio".to_string(), f64::NAN),
                ("top".to_string(), f64::INFINITY),
                ("bottom".to_string(), f64::NEG_INFINITY),
            ],
        );
        assert!(
            out.contains("# TYPE cache_size gauge\ncache_size 1042\n"),
            "{out}"
        );
        assert_eq!(out.matches("# TYPE jobs_running gauge").count(), 1, "{out}");
        assert!(out.contains("jobs_running{queue=\"high\"} 3\n"), "{out}");
        assert!(out.contains("jobs_running{queue=\"low\"} 0.5\n"), "{out}");
        // Not finite, as Prometheus writes it.
        assert!(out.contains("\nratio NaN\n"), "{out}");
        assert!(out.contains("\ntop +Inf\n"), "{out}");
        assert!(out.contains("\nbottom -Inf\n"), "{out}");
    }

    #[tokio::test]
    async fn the_time_of_the_request_is_a_header_when_asked() {
        let timed = app_with(&Options {
            process_time: true,
            ..Options::default()
        });
        let (_, h, _) = text(&timed, req("GET", "/v1/ping", &[])).await;
        let ms: f64 = h["x-process-time-ms"].to_str().unwrap().parse().unwrap();
        assert!(ms >= 0.0);
        let (_, h, _) = text(&app(), req("GET", "/v1/ping", &[])).await;
        assert!(h.get("x-process-time-ms").is_none());
    }

    #[test]
    fn the_openapi_document_follows_the_error_format() {
        let rs = routes(&program(SRC));
        let plain = openapi::document("t", &rs);
        assert!(plain["paths"]["/v1/rag/query"]["post"]["responses"]["400"].is_object());
        assert!(plain["components"]["schemas"]["Error"]["properties"]["error"].is_object());
        let detail = openapi::document_with("t", &rs, ErrorFormat::Detail);
        let responses = &detail["paths"]["/v1/rag/query"]["post"]["responses"];
        assert!(responses["422"].is_object() && responses["400"].is_null());
        assert!(detail["components"]["schemas"]["Error"]["properties"]["detail"].is_object());
    }

    #[tokio::test]
    async fn healthz_says_503_while_the_server_is_stopping() {
        let draining = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let app = app_with(&Options {
            draining: Some(draining.clone()),
            ..Options::default()
        });
        assert_eq!(call(&app, "GET", "/healthz", "").await.0, StatusCode::OK);
        draining.store(true, std::sync::atomic::Ordering::Relaxed);
        let (status, _, body) = text(&app, req("GET", "/healthz", &[])).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body, "stopping");
    }

    #[tokio::test]
    async fn the_docs_page_is_html() {
        let req = Request::builder().uri("/docs").body(Body::empty()).unwrap();
        let res = app().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert!(
            res.headers()[header::CONTENT_TYPE]
                .to_str()
                .unwrap()
                .starts_with("text/html")
        );
    }

    #[tokio::test]
    async fn valid_post_reaches_handler() {
        let (s, v) = call(
            &app(),
            "POST",
            "/v1/rag/query",
            r#"{"query":"hi","top_k":3}"#,
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(v, json!({"query": "hi", "top_k": 3}));
    }

    #[tokio::test]
    async fn invalid_payloads_are_rejected_with_details() {
        let a = app();
        let (s, v) = call(&a, "POST", "/v1/rag/query", r#"{"query":1,"extra":true}"#).await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
        let details = v["details"].to_string();
        assert!(
            details.contains("payload.query: expected Text"),
            "{details}"
        );
        assert!(
            details.contains("payload.top_k: missing required field"),
            "{details}"
        );
        assert!(
            details.contains("payload.extra: unknown field"),
            "{details}"
        );

        let (s, v) = call(&a, "POST", "/v1/rag/query", "not json").await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
        assert!(v["error"].as_str().unwrap().starts_with("invalid JSON"));

        let (s, _) = call(
            &a,
            "POST",
            "/v1/rag/query",
            r#"{"query":"a","top_k":99999999999}"#,
        )
        .await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
        let (s, _) = call(&a, "POST", "/v1/rag/query", r#"{"query":"a","top_k":1.5}"#).await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn tensor_shape_and_nested_types_are_validated() {
        let a = app();
        let ok = r#"{"v":[1,2.5,3],"tags":["a"],"small":255}"#;
        assert_eq!(call(&a, "PUT", "/v1/embed", ok).await.0, StatusCode::OK);
        let (s, v) = call(
            &a,
            "PUT",
            "/v1/embed",
            r#"{"v":[1,2],"tags":[1],"small":256}"#,
        )
        .await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
        let d = v["details"].to_string();
        assert!(
            d.contains("payload.v: expected 3 element(s), found 2"),
            "{d}"
        );
        assert!(d.contains("payload.tags[0]: expected Text"), "{d}");
        assert!(d.contains("256 is out of range for u8"), "{d}");
    }

    #[tokio::test]
    async fn get_reads_the_query_string() {
        let a = app();
        let (s, v) = call(&a, "GET", "/v1/search?query=rust&top_k=5", "").await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(v, json!({"query": "rust", "top_k": 5}));
        let (s, _) = call(&a, "GET", "/v1/search?query=rust&top_k=abc", "").await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
        let (s, _) = call(&a, "GET", "/v1/search?query=rust", "").await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
    }

    #[test]
    fn path_parameters_are_not_part_of_the_payload() {
        let rs = routes(&program(SRC));
        let step = rs.iter().find(|r| r.name == "step").unwrap();
        assert_eq!(
            step.path_params,
            [
                ("id".to_string(), Kind::Int(Prim::U8)),
                ("step".to_string(), Kind::Str)
            ]
        );
        assert_eq!(step.payload.as_ref().unwrap().fields.len(), 2);
        let result = rs.iter().find(|r| r.name == "result").unwrap();
        assert!(result.payload.is_none());
    }

    #[tokio::test]
    async fn path_parameters_are_read_converted_and_checked() {
        let a = app();
        let (s, v) = call(&a, "GET", "/v1/results/abc%20d", "").await;
        assert_eq!((s, v), (StatusCode::OK, json!("abc d")));

        let body = r#"{"query":"q","top_k":1}"#;
        let (s, v) = call(&a, "PUT", "/v1/jobs/7/steps/go", body).await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(
            v,
            json!({"id": 7, "step": "go", "payload": {"query": "q", "top_k": 1}})
        );

        let (s, v) = call(&a, "PUT", "/v1/jobs/x/steps/go", body).await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
        assert_eq!(
            v["error"],
            "invalid path parameter id: expected u8, found `x`"
        );
        let (s, v) = call(&a, "PUT", "/v1/jobs/256/steps/go", body).await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
        assert!(v["error"].as_str().unwrap().contains("out of range for u8"));
    }

    #[tokio::test]
    async fn the_request_reaches_the_handler_and_is_not_the_payload() {
        let a = app();
        let rs = routes(&program(SRC));
        let clear = rs.iter().find(|r| r.name == "clear").unwrap();
        assert!(clear.payload.is_none());

        let req = Request::builder()
            .method("DELETE")
            .uri("/v1/cache?x=1")
            .header("X-Admin-Token", "secret")
            .body(Body::empty())
            .unwrap();
        let res = a.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            v,
            json!({"method": "DELETE", "path": "/v1/cache", "token": "secret"})
        );
        let (_, v) = call(&a, "DELETE", "/v1/cache", "").await;
        assert_eq!(v["token"], Value::Null);
    }

    #[tokio::test]
    async fn a_reply_sets_the_status_and_the_headers() {
        let hir = program(
            r#"
@endpoint(GET, "/a")
fn a() -> Result[Text, Text]:
    Ok("a")

@endpoint(GET, "/b")
fn b() -> Result[Text, Text]:
    Ok("b")
"#,
        );
        let mut hs = BTreeMap::new();
        hs.insert(
            "a".to_string(),
            reply_handler(|_| async {
                Ok(Reply {
                    status: 202,
                    headers: vec![
                        ("location".into(), "/v1/results/1".into()),
                        ("content-type".into(), "application/vnd.job+json".into()),
                    ],
                    body: json!({"job_id": "1"}),
                    raw: None,
                    events: None,
                })
            }),
        );
        hs.insert(
            "b".to_string(),
            reply_handler(|_| async {
                Ok(Reply {
                    status: 200,
                    headers: vec![("bad header".into(), "x".into())],
                    body: json!(null),
                    raw: None,
                    events: None,
                })
            }),
        );
        let app = router(&routes(&hir), hs).unwrap();
        let get = |uri: &str| Request::builder().uri(uri).body(Body::empty()).unwrap();

        let res = app.clone().oneshot(get("/a")).await.unwrap();
        assert_eq!(res.status(), StatusCode::ACCEPTED);
        assert_eq!(res.headers()["location"], "/v1/results/1");
        assert_eq!(res.headers()["content-type"], "application/vnd.job+json");
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(
            serde_json::from_slice::<Value>(&bytes).unwrap(),
            json!({"job_id": "1"})
        );

        let res = app.oneshot(get("/b")).await.unwrap();
        assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn handler_errors_map_to_their_status() {
        let (s, v) = call(&app(), "GET", "/v1/search?query=x&top_k=0", "").await;
        assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(v, json!({"error": "top_k must be positive"}));
    }

    #[tokio::test]
    async fn payloadless_endpoint_health_and_unknown_routes() {
        let a = app();
        assert_eq!(
            call(&a, "GET", "/v1/ping", "").await,
            (StatusCode::OK, json!("pong"))
        );
        assert_eq!(call(&a, "GET", "/healthz", "").await.0, StatusCode::OK);
        assert_eq!(call(&a, "GET", "/nope", "").await.0, StatusCode::NOT_FOUND);
        assert_eq!(
            call(&a, "GET", "/v1/rag/query", "").await.0,
            StatusCode::METHOD_NOT_ALLOWED
        );
    }

    #[test]
    fn router_construction_errors() {
        let rs = routes(&program(SRC));
        let err = router(&rs, BTreeMap::new()).unwrap_err();
        assert_eq!(err, RouterError::MissingHandler("handle_rag".into()));

        let dup = vec![rs[3].clone(), rs[3].clone()];
        let mut hs = BTreeMap::new();
        hs.insert("ping".to_string(), handler(|_| async { Ok(json!("pong")) }));
        assert!(matches!(router(&dup, hs), Err(RouterError::Duplicate(_))));
    }

    #[tokio::test]
    async fn a_failure_inside_the_program_answers_internal_server_error() {
        let src = r#"
@endpoint(GET, "/boom")
fn boom() -> Result[Text, Text]:
    Ok("x")

@endpoint(GET, "/chosen")
fn chosen() -> Result[Text, Text]:
    Ok("x")
"#;
        for error_format in [ErrorFormat::Telvax, ErrorFormat::Detail] {
            let mut hs = BTreeMap::new();
            hs.insert(
                "boom".to_string(),
                handler(|_| async {
                    Err(HandlerError::internal("runtime error: index out of bounds"))
                }),
            );
            hs.insert(
                "chosen".to_string(),
                handler(|_| async { Err(HandlerError::new(500, "the cache is down")) }),
            );
            let options = Options {
                error_format,
                ..Options::default()
            };
            let app = router_with(&routes(&program(src)), hs, &options).unwrap();
            let key = if error_format == ErrorFormat::Detail {
                "detail"
            } else {
                "error"
            };
            let (status, body) = call(&app, "GET", "/boom", "").await;
            assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
            assert_eq!(body[key], "Internal server error");
            let (status, body) = call(&app, "GET", "/chosen", "").await;
            assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
            assert_eq!(body[key], "the cache is down");
        }
    }

    #[test]
    fn the_log_line_of_a_failure_names_the_request_and_the_reason() {
        let text = layers::failure_line(
            None,
            Some("abc"),
            None,
            "GET",
            "/x",
            500,
            "runtime error: boom",
        );
        assert_eq!(text, "error GET /x 500 request_id=abc: runtime error: boom");
        let json = layers::failure_line(
            Some(LogFormat::Json),
            Some("abc"),
            Some("4bf9"),
            "GET",
            "/x",
            500,
            "runtime error: boom",
        );
        let v: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["request_id"], "abc");
        assert_eq!(v["trace_id"], "4bf9");
        assert_eq!(v["level"], "error");
        assert_eq!(v["error"], "runtime error: boom");
    }
}

#[cfg(test)]
mod text_replies {
    use super::*;
    use http_body_util::BodyExt;

    fn make(text: &str, headers: Vec<(String, String)>) -> Reply {
        Reply {
            status: 200,
            headers,
            body: serde_json::json!(null),
            raw: Some(text.as_bytes().to_vec()),
            events: None,
        }
    }

    #[tokio::test]
    async fn a_text_reply_is_sent_as_it_is_with_the_content_type_it_names() {
        // No header: plain bytes, and the body is not written as JSON (no quotes, no escapes).
        let res = reply_response(make("<h1>\"hi\"</h1>\n", vec![]));
        assert_eq!(res.headers()["content-type"], "application/octet-stream");
        let body = res.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(&body[..], b"<h1>\"hi\"</h1>\n");
        // A header of the program replaces it.
        let res = reply_response(make(
            "x",
            vec![("content-type".into(), "text/html; charset=utf-8".into())],
        ));
        assert_eq!(res.headers()["content-type"], "text/html; charset=utf-8");
        // The JSON body is still what a reply without text sends.
        let res = reply_response(Reply::ok(serde_json::json!({"a": 1})));
        assert_eq!(res.headers()["content-type"], "application/json");
    }
}

#[cfg(test)]
mod static_files {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    fn folder() -> (std::path::PathBuf, std::path::PathBuf) {
        let base = std::env::temp_dir().join(format!("telvax-static-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let dir = base.join("public");
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::create_dir_all(dir.join("guide")).unwrap();
        std::fs::create_dir_all(dir.join("docs")).unwrap();
        std::fs::write(dir.join("index.html"), "<h1>home</h1>").unwrap();
        std::fs::write(dir.join("app.js"), "console.log(1)").unwrap();
        std::fs::write(dir.join("a b.txt"), "spaced").unwrap();
        std::fs::write(dir.join("sub/page.html"), "<p>page</p>").unwrap();
        std::fs::write(dir.join("guide/index.html"), "<p>guide</p>").unwrap();
        std::fs::write(dir.join("docs/index.html"), "<p>mine</p>").unwrap();
        // Outside the folder: what a request must never reach.
        std::fs::write(base.join("secret.txt"), "secret").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(base.join("secret.txt"), dir.join("link.txt")).unwrap();
        (base, dir)
    }

    async fn send(
        app: &Router,
        method: &str,
        uri: &str,
        headers: &[(&str, &str)],
    ) -> (StatusCode, HeaderMap, Vec<u8>) {
        let mut req = Request::builder().method(method).uri(uri);
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        let res = app
            .clone()
            .oneshot(req.body(Body::empty()).unwrap())
            .await
            .unwrap();
        let (status, headers) = (res.status(), res.headers().clone());
        let body = res.into_body().collect().await.unwrap().to_bytes().to_vec();
        (status, headers, body)
    }

    #[tokio::test]
    async fn files_are_served_and_nothing_outside_the_folder_is() {
        let (base, dir) = folder();
        let options = Options {
            static_dir: Some(dir.clone()),
            ..Default::default()
        };
        let app = router_with(&[], BTreeMap::new(), &options).unwrap();
        let (s, h, b) = send(&app, "GET", "/", &[]).await;
        assert_eq!((s, &b[..]), (StatusCode::OK, &b"<h1>home</h1>"[..]));
        assert_eq!(h["content-type"], "text/html; charset=utf-8");
        let (s, h, b) = send(&app, "GET", "/app.js", &[]).await;
        assert_eq!((s, &b[..]), (StatusCode::OK, &b"console.log(1)"[..]));
        assert_eq!(h["content-type"], "text/javascript; charset=utf-8");
        assert_eq!(h["cache-control"], "no-cache");
        let etag = h["etag"].to_str().unwrap().to_string();
        // The same version again is a 304 with no body.
        let (s, _, b) = send(&app, "GET", "/app.js", &[("if-none-match", &etag)]).await;
        assert_eq!((s, b.len()), (StatusCode::NOT_MODIFIED, 0));
        let (s, _, b) = send(&app, "GET", "/app.js", &[("if-none-match", "\"other\"")]).await;
        assert_eq!((s, b.len()), (StatusCode::OK, 14));
        // HEAD: the headers, no body.
        let (s, h, b) = send(&app, "HEAD", "/app.js", &[]).await;
        assert_eq!((s, b.len()), (StatusCode::OK, 0));
        assert_eq!(h["content-length"], "14");
        // Nested files, a folder with an index, and an escaped space.
        assert_eq!(
            send(&app, "GET", "/sub/page.html", &[]).await.2,
            b"<p>page</p>"
        );
        assert_eq!(send(&app, "GET", "/guide/", &[]).await.2, b"<p>guide</p>");
        assert_eq!(send(&app, "GET", "/guide", &[]).await.2, b"<p>guide</p>");
        // A path the server answers itself wins over a file of the same name.
        let (_, _, b) = send(&app, "GET", "/docs", &[]).await;
        assert!(!String::from_utf8_lossy(&b).contains("<p>mine</p>"));
        assert_eq!(send(&app, "GET", "/a%20b.txt", &[]).await.2, b"spaced");
        // No listing, no way out, nothing else.
        for (method, uri) in [
            ("GET", "/sub/"),
            ("GET", "/missing.css"),
            ("GET", "/../secret.txt"),
            ("GET", "/%2e%2e/secret.txt"),
            ("GET", "/sub/%2e%2e/%2e%2e/secret.txt"),
            ("GET", "/..%2fsecret.txt"),
            ("GET", "/%zz"),
            ("POST", "/app.js"),
            ("DELETE", "/index.html"),
        ] {
            let (s, _, b) = send(&app, method, uri, &[]).await;
            assert_eq!(s, StatusCode::NOT_FOUND, "{method} {uri}");
            assert!(
                !String::from_utf8_lossy(&b).contains("secret"),
                "{method} {uri}"
            );
        }
        #[cfg(unix)]
        {
            let (s, _, b) = send(&app, "GET", "/link.txt", &[]).await;
            assert_eq!(s, StatusCode::NOT_FOUND, "a link out of the folder");
            assert!(b.is_empty());
        }
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn a_request_path_never_names_a_parent() {
        assert_eq!(
            static_relative("/a/b.txt"),
            Some(std::path::PathBuf::from("a/b.txt"))
        );
        assert_eq!(
            static_relative("//a//b"),
            Some(std::path::PathBuf::from("a/b"))
        );
        assert_eq!(static_relative("/"), Some(std::path::PathBuf::new()));
        for bad in [
            "/../x",
            "/a/../../x",
            "/%2E%2E/x",
            "/a/./b",
            "/a%5cb",
            "/a%00b",
            "/c:/x",
            "/%",
        ] {
            assert_eq!(static_relative(bad), None, "{bad}");
        }
    }
}
