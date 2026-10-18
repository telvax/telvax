//! What `telvax serve` can put around the handlers: a request id, metrics and a log line per request.

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use axum::extract::{ConnectInfo, MatchedPath, Request, State};
use axum::http::{HeaderName, HeaderValue, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

/// How a request is written to the log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogFormat {
    Json,
    Text,
}

/// The counters a program made itself, as `(name, value)`.
pub type Counters = Arc<dyn Fn() -> Vec<(String, u64)> + Send + Sync>;

/// At most `max` requests from one client in each `window`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RateLimit {
    pub max: u32,
    pub window: Duration,
}

/// What to put around the handlers. Everything is off by default.
#[derive(Clone, Default)]
pub struct Options {
    /// Origins allowed by CORS; `*` allows any.
    pub cors: Option<Vec<String>>,
    /// A limit per client.
    pub rate_limit: Option<RateLimit>,
    /// Add `x-content-type-options`, `x-frame-options`, `referrer-policy` and `cache-control`.
    pub security_headers: bool,
    /// Tell clients apart by the first address of `x-forwarded-for`, not by the connection.
    pub trust_proxy: bool,
    /// Serve `GET /metrics`.
    pub metrics: bool,
    /// Write one line per request to standard error.
    pub log: Option<LogFormat>,
    /// The counters of the program, listed in `/metrics`.
    pub counters: Option<Counters>,
<<<<<<< HEAD
    /// Add `x-process-time-ms`, the time the server took for the request.
    pub process_time: bool,
=======
    /// How the errors that the server itself makes are written.
    pub error_format: ErrorFormat,
}

/// The shape of the errors the server makes (a body that does not fit, a path parameter that does not
/// convert, a rate limit, an unknown route). The `Err(Status...)` of a program has the same shape.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ErrorFormat {
    /// `{"error": "..."}`, and `400` for a body that does not fit.
    #[default]
    Telvax,
    /// `{"detail": ...}`, and `422` for a body that does not fit, as FastAPI answers.
    Detail,
}

/// Marks an answer the server made, as opposed to one the program built with `Response`.
#[derive(Clone, Copy)]
pub(crate) struct OwnError;

/// An error answer of the server, in the `Telvax` shape; `detail_errors` rewrites it when asked to.
pub(crate) fn own_error(status: StatusCode, body: serde_json::Value) -> Response {
    let mut res = (status, axum::Json(body)).into_response();
    res.extensions_mut().insert(OwnError);
    res
}

/// Writes the errors of the server as `{"detail": ...}`, with `422` for a body that does not fit, and
/// gives the empty `404` and `405` of an unknown route a body too.
pub(crate) async fn detail_errors(req: Request, next: Next) -> Response {
    let res = next.run(req).await;
    let (status, own) = (res.status(), res.extensions().get::<OwnError>().is_some());
    let bare = (status == StatusCode::NOT_FOUND || status == StatusCode::METHOD_NOT_ALLOWED)
        && !res.headers().contains_key(header::CONTENT_TYPE);
    if bare {
        let message = if status == StatusCode::NOT_FOUND {
            "Not Found"
        } else {
            "Method Not Allowed"
        };
        let (mut parts, _) = res.into_parts();
        parts.headers.remove(header::CONTENT_LENGTH);
        return (parts, axum::Json(serde_json::json!({ "detail": message }))).into_response();
    }
    if !own {
        return res;
    }
    let (mut parts, body) = res.into_parts();
    let Ok(bytes) = axum::body::to_bytes(body, 1 << 20).await else {
        return Response::from_parts(parts, axum::body::Body::empty());
    };
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return Response::from_parts(parts, axum::body::Body::from(bytes));
    };
    let detail = match (value.get("details"), value.get("error")) {
        (Some(list), _) if parts.status == StatusCode::BAD_REQUEST => {
            parts.status = StatusCode::UNPROCESSABLE_ENTITY;
            list.clone()
        }
        (_, Some(message)) => message.clone(),
        _ => return Response::from_parts(parts, axum::body::Body::from(bytes)),
    };
    parts.headers.remove(header::CONTENT_LENGTH);
    (parts, axum::Json(serde_json::json!({ "detail": detail }))).into_response()
>>>>>>> f446b41 (feat: telvax serve --error-format detail writes errors as FastAPI does)
}

impl std::fmt::Debug for Options {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Options")
            .field("cors", &self.cors)
            .field("rate_limit", &self.rate_limit)
            .field("security_headers", &self.security_headers)
            .field("trust_proxy", &self.trust_proxy)
            .field("metrics", &self.metrics)
            .field("log", &self.log)
            .field("counters", &self.counters.is_some())
<<<<<<< HEAD
            .field("process_time", &self.process_time)
=======
            .field("error_format", &self.error_format)
>>>>>>> f446b41 (feat: telvax serve --error-format detail writes errors as FastAPI does)
            .finish()
    }
}

impl Options {
    pub(crate) fn observes(&self) -> bool {
        self.metrics || self.log.is_some() || self.process_time
    }
}

const BUCKETS: [f64; 11] = [
    0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
];

#[derive(Debug, Default)]
struct Histogram {
    buckets: [u64; BUCKETS.len()],
    count: u64,
    sum: f64,
}

/// What the middleware records and `/metrics` prints.
#[derive(Debug, Default)]
pub struct Metrics {
    requests: Mutex<BTreeMap<(String, String, u16), u64>>,
    durations: Mutex<BTreeMap<(String, String), Histogram>>,
}

impl Metrics {
    fn record(&self, method: &str, route: &str, status: u16, seconds: f64) {
        *self
            .requests
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entry((method.into(), route.into(), status))
            .or_default() += 1;
        let mut durations = self.durations.lock().unwrap_or_else(|e| e.into_inner());
        let h = durations.entry((method.into(), route.into())).or_default();
        h.count += 1;
        h.sum += seconds;
        for (i, le) in BUCKETS.iter().enumerate() {
            if seconds <= *le {
                h.buckets[i] += 1;
            }
        }
    }

    /// The Prometheus text format.
    pub fn render(&self, counters: &[(String, u64)]) -> String {
        let mut out = String::new();
        out.push_str("# HELP telvax_http_requests_total Requests answered.\n");
        out.push_str("# TYPE telvax_http_requests_total counter\n");
        for ((method, route, status), n) in self
            .requests
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
        {
            out.push_str(&format!(
                "telvax_http_requests_total{{method=\"{}\",route=\"{}\",status=\"{status}\"}} {n}\n",
                label(method),
                label(route)
            ));
        }
        out.push_str("# HELP telvax_http_request_duration_seconds Time to answer a request.\n");
        out.push_str("# TYPE telvax_http_request_duration_seconds histogram\n");
        for ((method, route), h) in self
            .durations
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
        {
            let labels = format!("method=\"{}\",route=\"{}\"", label(method), label(route));
            for (i, le) in BUCKETS.iter().enumerate() {
                out.push_str(&format!(
                    "telvax_http_request_duration_seconds_bucket{{{labels},le=\"{le}\"}} {}\n",
                    h.buckets[i]
                ));
            }
            out.push_str(&format!(
                "telvax_http_request_duration_seconds_bucket{{{labels},le=\"+Inf\"}} {}\n",
                h.count
            ));
            out.push_str(&format!(
                "telvax_http_request_duration_seconds_sum{{{labels}}} {}\n",
                h.sum
            ));
            out.push_str(&format!(
                "telvax_http_request_duration_seconds_count{{{labels}}} {}\n",
                h.count
            ));
        }
        out.push_str(
            "# HELP telvax_counter_total Counters made by the program with std.metrics.count.\n",
        );
        out.push_str("# TYPE telvax_counter_total counter\n");
        // A counter made with labels is listed under its own name, `name{label="value"}`.
        let mut typed = std::collections::BTreeSet::new();
        for (name, n) in counters {
            match name.split_once('{') {
                Some((base, _)) => {
                    if typed.insert(base.to_string()) {
                        out.push_str(&format!("# TYPE {base} counter\n"));
                    }
                    out.push_str(&format!("{name} {n}\n"));
                }
                None => out.push_str(&format!(
                    "telvax_counter_total{{name=\"{}\"}} {n}\n",
                    label(name)
                )),
            }
        }
        out
    }
}

/// A label value: backslash, quote and newline are escaped.
fn label(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

pub(crate) struct Observer {
    pub metrics: Arc<Metrics>,
    pub record: bool,
    pub log: Option<LogFormat>,
    pub process_time: bool,
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn new_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as u64);
    let n = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    format!("{:016x}", nanos ^ n.wrapping_mul(0x9E37_79B9_7F4A_7C15))
}

/// The id the client sent, if it is short and plain; otherwise a new one.
fn request_id(req: &Request) -> String {
    req.headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .filter(|v| {
            !v.is_empty() && v.len() <= 128 && v.bytes().all(|b| (0x21..=0x7e).contains(&b))
        })
        .map_or_else(new_id, str::to_string)
}

/// Gives the request an id, times it, records it and logs it.
pub(crate) async fn observe(
    State(o): State<Arc<Observer>>,
    mut req: Request,
    next: Next,
) -> Response {
    let id = request_id(&req);
    let value = HeaderValue::from_str(&id).expect("a plain id");
    req.headers_mut()
        .insert(HeaderName::from_static("x-request-id"), value.clone());
    let method = req.method().to_string();
    let path = req.uri().path().to_string();
    // `Router::layer` runs after routing, so the matched route is on the request.
    let route = req
        .extensions()
        .get::<MatchedPath>()
        .map_or("unmatched", MatchedPath::as_str)
        .to_string();
    let started = Instant::now();
    let mut res = next.run(req).await;
    let elapsed = started.elapsed();
    let status = res.status().as_u16();
    res.headers_mut()
        .insert(HeaderName::from_static("x-request-id"), value);
    if o.process_time
        && let Ok(ms) = HeaderValue::from_str(&format!("{:.2}", elapsed.as_secs_f64() * 1000.0))
    {
        res.headers_mut()
            .insert(HeaderName::from_static("x-process-time-ms"), ms);
    }
    if o.record {
        o.metrics
            .record(&method, &route, status, elapsed.as_secs_f64());
    }
    if let Some(format) = o.log {
        let ms = elapsed.as_secs_f64() * 1000.0;
        match format {
            LogFormat::Json => eprintln!(
                "{}",
                serde_json::json!({
                    "ts": rfc3339(SystemTime::now()),
                    "request_id": id,
                    "method": method,
                    "path": path,
                    "status": status,
                    "duration_ms": (ms * 10.0).round() / 10.0,
                })
            ),
            LogFormat::Text => eprintln!("{method} {path} {status} {ms:.1}ms request_id={id}"),
        }
    }
    res
}

/// The headers of the answers that are asked to be safe by default.
pub(crate) async fn security_headers(req: Request, next: Next) -> Response {
    let mut res = next.run(req).await;
    let headers = res.headers_mut();
    for (name, value) in [
        ("x-content-type-options", "nosniff"),
        ("x-frame-options", "DENY"),
        ("referrer-policy", "no-referrer"),
        ("cache-control", "no-store"),
    ] {
        headers
            .entry(HeaderName::from_static(name))
            .or_insert(HeaderValue::from_static(value));
    }
    res
}

pub(crate) struct Cors {
    pub origins: Vec<String>,
}

impl Cors {
    fn allows(&self, origin: &str) -> bool {
        self.origins.iter().any(|o| o == "*" || o == origin)
    }
}

/// CORS: the headers that let a browser page of an allowed origin read the answer, and the answer to a
/// preflight request.
pub(crate) async fn cors(State(c): State<Arc<Cors>>, req: Request, next: Next) -> Response {
    let origin = req
        .headers()
        .get(header::ORIGIN)
        .and_then(|v| v.to_str().ok())
        .filter(|o| c.allows(o))
        .map(str::to_string);
    let Some(origin) = origin else {
        return next.run(req).await;
    };
    let any = c.origins.iter().any(|o| o == "*");
    let value = if any { "*".to_string() } else { origin };
    let preflight = req.method() == Method::OPTIONS
        && req
            .headers()
            .contains_key(header::ACCESS_CONTROL_REQUEST_METHOD);
    let asked = req
        .headers()
        .get(header::ACCESS_CONTROL_REQUEST_HEADERS)
        .cloned();
    let mut res = if preflight {
        let mut res = Response::new(axum::body::Body::empty());
        *res.status_mut() = StatusCode::NO_CONTENT;
        let h = res.headers_mut();
        h.insert(
            header::ACCESS_CONTROL_ALLOW_METHODS,
            HeaderValue::from_static("GET, POST, PUT, PATCH, DELETE, OPTIONS"),
        );
        h.insert(
            header::ACCESS_CONTROL_ALLOW_HEADERS,
            asked.unwrap_or(HeaderValue::from_static(
                "content-type, authorization, x-request-id",
            )),
        );
        h.insert(
            header::ACCESS_CONTROL_MAX_AGE,
            HeaderValue::from_static("600"),
        );
        res
    } else {
        next.run(req).await
    };
    if let Ok(v) = HeaderValue::from_str(&value) {
        res.headers_mut()
            .insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, v);
    }
    if !any {
        res.headers_mut()
            .append(header::VARY, HeaderValue::from_static("origin"));
    }
    res
}

pub(crate) struct Limiter {
    pub limit: RateLimit,
    pub trust_proxy: bool,
    /// Per client: when its window started and how many requests came in it.
    clients: Mutex<HashMap<String, (Instant, u32)>>,
}

impl Limiter {
    pub fn new(limit: RateLimit, trust_proxy: bool) -> Self {
        Self {
            limit,
            trust_proxy,
            clients: Mutex::new(HashMap::new()),
        }
    }

    /// `None` if the request may go on; otherwise the seconds to wait.
    pub(crate) fn check(&self, client: &str, now: Instant) -> Option<u64> {
        let mut clients = self.clients.lock().unwrap_or_else(|e| e.into_inner());
        if clients.len() > 10_000 {
            clients.retain(|_, (start, _)| now.duration_since(*start) < self.limit.window);
        }
        let entry = clients.entry(client.to_string()).or_insert((now, 0));
        if now.duration_since(entry.0) >= self.limit.window {
            *entry = (now, 0);
        }
        if entry.1 >= self.limit.max {
            let left = self
                .limit
                .window
                .saturating_sub(now.duration_since(entry.0));
            return Some(left.as_secs().max(1));
        }
        entry.1 += 1;
        None
    }
}

/// The client of a request: the address of the connection, or the first address of
/// `x-forwarded-for` when the proxy is trusted.
fn client_of(req: &Request, trust_proxy: bool) -> String {
    if trust_proxy
        && let Some(first) = req
            .headers()
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split(',').next())
            .map(str::trim)
            .filter(|v| !v.is_empty())
    {
        return first.to_string();
    }
    req.extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map_or_else(|| "unknown".into(), |c| c.0.ip().to_string())
}

pub(crate) async fn rate_limit(
    State(l): State<Arc<Limiter>>,
    req: Request,
    next: Next,
) -> Response {
    if req.uri().path() != "/healthz"
        && let Some(wait) = l.check(&client_of(&req, l.trust_proxy), Instant::now())
    {
        let mut res = own_error(
            StatusCode::TOO_MANY_REQUESTS,
            serde_json::json!({ "error": "rate limit exceeded" }),
        );
        res.headers_mut()
            .insert(header::RETRY_AFTER, HeaderValue::from(wait));
        return res;
    }
    next.run(req).await
}

/// `2026-10-18T12:00:00.123Z`
fn rfc3339(t: SystemTime) -> String {
    let d = t.duration_since(UNIX_EPOCH).unwrap_or_default();
    let (secs, millis) = (d.as_secs() as i64, d.subsec_millis());
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Days since 1970-01-01 to a calendar date (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{millis:03}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_are_rfc3339() {
        let t = |s: u64| UNIX_EPOCH + std::time::Duration::from_secs(s);
        assert_eq!(rfc3339(t(0)), "1970-01-01T00:00:00.000Z");
        assert_eq!(rfc3339(t(951_782_400)), "2000-02-29T00:00:00.000Z");
        assert_eq!(rfc3339(t(1_791_000_000)), "2026-10-03T04:00:00.000Z");
    }

    #[test]
    fn labels_are_escaped() {
        assert_eq!(label("a\"b\\c\nd"), "a\\\"b\\\\c\\nd");
    }
}
