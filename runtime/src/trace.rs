//! The trace context of the request a thread is answering (W3C Trace Context): the trace it belongs to, the span
//! of this server for it, and the request id. `std.log` adds the ids to its lines and `std.http.client` sends a
//! `traceparent` on, so a trace crosses services.

use std::cell::RefCell;

/// The context of one request.
#[derive(Debug, Clone, PartialEq)]
pub struct TraceContext {
    /// 32 lower case hex digits.
    pub trace_id: String,
    /// The span of this server for the request: 16 lower case hex digits.
    pub span_id: String,
    /// The trace flags, two hex digits (`01` when sampled).
    pub flags: String,
    pub request_id: Option<String>,
    /// The span of the caller that sent the request, from its `traceparent`.
    pub parent_span_id: Option<String>,
}

thread_local! {
    static CURRENT: RefCell<Option<TraceContext>> = const { RefCell::new(None) };
}

/// The parts of a `traceparent` header (`00-<trace id>-<parent id>-<flags>`), or `None` when it is not one: a
/// version that is not two hex digits or is `ff`, ids of the wrong length or not lower case hex, or all zeros.
pub fn parse_traceparent(header: &str) -> Option<(String, String, String)> {
    let parts: Vec<&str> = header.trim().split('-').collect();
    let hex = |s: &str, n: usize| {
        s.len() == n && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    };
    let [version, trace, parent, flags, ..] = parts.as_slice() else {
        return None;
    };
    // Version 00 has exactly four parts; a later version may add more, and is read the same way.
    if !hex(version, 2) || *version == "ff" || (*version == "00" && parts.len() != 4) {
        return None;
    }
    if !hex(trace, 32) || !hex(parent, 16) || !hex(flags, 2) {
        return None;
    }
    if trace.bytes().all(|b| b == b'0') || parent.bytes().all(|b| b == b'0') {
        return None;
    }
    Some((trace.to_string(), parent.to_string(), flags.to_string()))
}

/// `n` random bytes as lower case hex, never all zeros.
fn random_hex(n: usize) -> String {
    let mut bytes = vec![0u8; n];
    #[cfg(not(target_arch = "wasm32"))]
    let _ = getrandom::fill(&mut bytes);
    if bytes.iter().all(|b| *b == 0) {
        bytes[n - 1] = 1;
    }
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn new_span_id() -> String {
    random_hex(8)
}

impl TraceContext {
    /// The context of a request from its headers: the trace of its `traceparent` when it has a valid one, or a new
    /// trace, and a new span of this server in either case.
    pub fn from_headers(headers: &[(String, String)]) -> TraceContext {
        let find = |name: &str| {
            headers
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(name))
                .map(|(_, v)| v.as_str())
        };
        let (trace_id, parent, flags) = match find("traceparent").and_then(parse_traceparent) {
            Some((trace, parent, flags)) => (trace, Some(parent), flags),
            None => (random_hex(16), None, "01".to_string()),
        };
        TraceContext {
            trace_id,
            span_id: new_span_id(),
            flags,
            request_id: find("x-request-id").map(str::to_string),
            parent_span_id: parent,
        }
    }

    /// The `traceparent` for a call this span makes: the same trace, a new span whose parent is this one.
    pub fn child_traceparent(&self) -> String {
        self.child().1
    }

    /// A new span for a call this span makes, and its `traceparent`.
    pub fn child(&self) -> (String, String) {
        let span = new_span_id();
        let header = format!("00-{}-{}-{}", self.trace_id, span, self.flags);
        (span, header)
    }
}

/// Runs `f` with `ctx` as the context of this thread, and clears it after, even when `f` panics.
pub fn with_context<R>(ctx: TraceContext, f: impl FnOnce() -> R) -> R {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            CURRENT.with(|c| *c.borrow_mut() = None);
        }
    }
    CURRENT.with(|c| *c.borrow_mut() = Some(ctx));
    let _reset = Reset;
    f()
}

/// The context of the request this thread is answering, if any.
pub fn current() -> Option<TraceContext> {
    CURRENT.with(|c| c.borrow().clone())
}

// ---- spans and their export ------------------------------------------------------------------

/// A finished span, as OTLP describes one.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Span {
    pub trace_id: String,
    pub span_id: String,
    pub parent_span_id: Option<String>,
    pub name: String,
    /// `2` for the server side of a request, `3` for a call to another service (OTLP's `SpanKind`).
    pub kind: u8,
    pub start_unix_nanos: u128,
    pub end_unix_nanos: u128,
    pub attributes: Vec<(String, AttrValue)>,
    /// The error, when the span failed.
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AttrValue {
    Text(String),
    Int(i64),
}

/// Runs `call`, a call this program makes to another service, as a client span of the request being answered (when
/// there is one and spans are exported): `name`, `attributes`, and the error when `call` fails.
pub fn client_span<T, E: std::fmt::Display>(
    name: &str,
    attributes: Vec<(String, AttrValue)>,
    call: impl FnOnce() -> Result<T, E>,
) -> Result<T, E> {
    let ctx = current().filter(|_| exporting());
    let start = now_unix_nanos();
    let result = call();
    if let Some(ctx) = ctx {
        record(Span {
            trace_id: ctx.trace_id.clone(),
            span_id: new_span_id(),
            parent_span_id: Some(ctx.span_id),
            name: name.to_string(),
            kind: 3,
            start_unix_nanos: start,
            end_unix_nanos: now_unix_nanos(),
            attributes,
            error: result.as_ref().err().map(|e| e.to_string()),
        });
    }
    result
}

/// Nanoseconds since 1970, for the times of a span.
pub fn now_unix_nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos())
}

struct Exporter {
    endpoint: String,
    service: String,
    pending: Vec<Span>,
}

static EXPORTER: std::sync::Mutex<Option<Exporter>> = std::sync::Mutex::new(None);

/// The URL spans are sent to, from the standard variables: `OTEL_EXPORTER_OTLP_TRACES_ENDPOINT` as it is, or
/// `OTEL_EXPORTER_OTLP_ENDPOINT` with `/v1/traces`. `None` when neither is set, and then nothing is recorded.
pub fn endpoint_from_env() -> Option<String> {
    endpoint_from(|n| std::env::var(n).ok())
}

fn endpoint_from(var: impl Fn(&str) -> Option<String>) -> Option<String> {
    let var = |n: &str| var(n).filter(|v| !v.trim().is_empty());
    if let Some(url) = var("OTEL_EXPORTER_OTLP_TRACES_ENDPOINT") {
        return Some(url.trim().to_string());
    }
    var("OTEL_EXPORTER_OTLP_ENDPOINT")
        .map(|base| format!("{}/v1/traces", base.trim().trim_end_matches('/')))
}

/// Starts sending spans to `endpoint` as `service`: they are kept and sent every 2 seconds, or at once when 512 are
/// waiting, by a thread of their own, so a request never waits for the collector.
pub fn start_export(endpoint: String, service: String) {
    *EXPORTER.lock().unwrap_or_else(|e| e.into_inner()) = Some(Exporter {
        endpoint,
        service,
        pending: Vec::new(),
    });
    std::thread::spawn(|| {
        loop {
            std::thread::sleep(std::time::Duration::from_secs(2));
            flush();
        }
    });
}

/// Whether spans are being exported.
pub fn exporting() -> bool {
    EXPORTER.lock().unwrap_or_else(|e| e.into_inner()).is_some()
}

/// Keeps a finished span for the next send; nothing happens when no export was started.
pub fn record(span: Span) {
    let full = {
        let mut guard = EXPORTER.lock().unwrap_or_else(|e| e.into_inner());
        let Some(exporter) = guard.as_mut() else {
            return;
        };
        // A collector that is down must not make the server hold every span in memory.
        if exporter.pending.len() < 10_000 {
            exporter.pending.push(span);
        }
        exporter.pending.len() >= 512
    };
    if full {
        std::thread::spawn(flush);
    }
}

/// Sends the spans that are waiting. A collector that does not answer, or answers with an error, loses them, with one
/// line on the standard error: tracing never stops the program.
pub fn flush() {
    let (endpoint, body, count) = {
        let mut guard = EXPORTER.lock().unwrap_or_else(|e| e.into_inner());
        let Some(exporter) = guard.as_mut() else {
            return;
        };
        if exporter.pending.is_empty() {
            return;
        }
        let spans = std::mem::take(&mut exporter.pending);
        (
            exporter.endpoint.clone(),
            otlp_json(&exporter.service, &spans),
            spans.len(),
        )
    };
    #[cfg(feature = "live")]
    {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(std::time::Duration::from_secs(5)))
            .http_status_as_error(false)
            .build()
            .into();
        match agent
            .post(&endpoint)
            .header("content-type", "application/json")
            .send(body.to_string())
        {
            Ok(r) if r.status().is_success() => {}
            Ok(r) => eprintln!(
                "otlp: {endpoint} answered {} for {count} span(s)",
                r.status()
            ),
            Err(e) => eprintln!("otlp: {count} span(s) not sent to {endpoint}: {e}"),
        }
    }
    #[cfg(not(feature = "live"))]
    let _ = (endpoint, body, count);
}

/// The body of an OTLP/HTTP request in its JSON encoding: ids as hex, times as text, one resource for the service.
pub fn otlp_json(service: &str, spans: &[Span]) -> serde_json::Value {
    use serde_json::json;
    let attr = |(k, v): &(String, AttrValue)| match v {
        AttrValue::Text(t) => json!({"key": k, "value": {"stringValue": t}}),
        AttrValue::Int(n) => json!({"key": k, "value": {"intValue": n.to_string()}}),
    };
    let spans: Vec<serde_json::Value> = spans
        .iter()
        .map(|s| {
            let mut span = json!({
                "traceId": s.trace_id,
                "spanId": s.span_id,
                "name": s.name,
                "kind": s.kind,
                "startTimeUnixNano": s.start_unix_nanos.to_string(),
                "endTimeUnixNano": s.end_unix_nanos.to_string(),
                "attributes": s.attributes.iter().map(attr).collect::<Vec<_>>(),
                "status": match &s.error {
                    Some(m) => json!({"code": 2, "message": m}),
                    None => json!({"code": 0}),
                },
            });
            if let Some(parent) = &s.parent_span_id {
                span["parentSpanId"] = json!(parent);
            }
            span
        })
        .collect();
    json!({"resourceSpans": [{
        "resource": {"attributes": [
            {"key": "service.name", "value": {"stringValue": service}},
            {"key": "telemetry.sdk.name", "value": {"stringValue": "telvax"}},
        ]},
        "scopeSpans": [{"scope": {"name": "telvax"}, "spans": spans}],
    }]})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_traceparent_is_read_as_the_w3c_says() {
        let ok = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";
        assert_eq!(
            parse_traceparent(ok),
            Some((
                "4bf92f3577b34da6a3ce929d0e0e4736".into(),
                "00f067aa0ba902b7".into(),
                "01".into()
            ))
        );
        // A later version may add parts after the flags.
        assert!(
            parse_traceparent("01-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01-x")
                .is_some()
        );
        for bad in [
            "",
            "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7",
            "00-4BF92F3577B34DA6A3CE929D0E0E4736-00f067aa0ba902b7-01",
            "00-00000000000000000000000000000000-00f067aa0ba902b7-01",
            "00-4bf92f3577b34da6a3ce929d0e0e4736-0000000000000000-01",
            "ff-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
            "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01-extra",
            "00-4bf92f3577b34da6a3ce929d0e0e473-00f067aa0ba902b7-01",
        ] {
            assert_eq!(parse_traceparent(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn a_request_keeps_the_trace_it_came_with_and_starts_one_otherwise() {
        let headers = vec![
            (
                "Traceparent".to_string(),
                "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01".to_string(),
            ),
            ("x-request-id".to_string(), "req-1".to_string()),
        ];
        let ctx = TraceContext::from_headers(&headers);
        assert_eq!(ctx.trace_id, "4bf92f3577b34da6a3ce929d0e0e4736");
        assert_ne!(
            ctx.span_id, "00f067aa0ba902b7",
            "this server has a span of its own"
        );
        assert_eq!(ctx.request_id.as_deref(), Some("req-1"));
        let child = parse_traceparent(&ctx.child_traceparent()).unwrap();
        assert_eq!(child.0, ctx.trace_id);
        assert_ne!(child.1, ctx.span_id);
        let fresh = TraceContext::from_headers(&[]);
        assert!(fresh.trace_id.len() == 32 && fresh.span_id.len() == 16 && fresh.flags == "01");
        assert_ne!(fresh.trace_id, TraceContext::from_headers(&[]).trace_id);
    }

    #[test]
    fn the_context_is_cleared_after_the_call_even_on_a_panic() {
        let ctx = TraceContext::from_headers(&[]);
        let seen = with_context(ctx.clone(), current);
        assert_eq!(seen, Some(ctx.clone()));
        assert_eq!(current(), None);
        let _ = std::panic::catch_unwind(|| with_context(ctx, || panic!("boom")));
        assert_eq!(current(), None);
    }

    #[test]
    fn the_endpoint_comes_from_the_standard_variables() {
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |n: &str| {
                pairs
                    .iter()
                    .find(|(k, _)| *k == n)
                    .map(|(_, v)| v.to_string())
            }
        };
        assert_eq!(endpoint_from(env(&[])), None);
        assert_eq!(
            endpoint_from(env(&[("OTEL_EXPORTER_OTLP_ENDPOINT", "  ")])),
            None
        );
        assert_eq!(
            endpoint_from(env(&[("OTEL_EXPORTER_OTLP_ENDPOINT", "http://c:4318/")])).as_deref(),
            Some("http://c:4318/v1/traces")
        );
        // The one for traces is used as it is, and wins.
        assert_eq!(
            endpoint_from(env(&[
                ("OTEL_EXPORTER_OTLP_ENDPOINT", "http://c:4318"),
                ("OTEL_EXPORTER_OTLP_TRACES_ENDPOINT", "http://t:9/x"),
            ]))
            .as_deref(),
            Some("http://t:9/x")
        );
    }

    #[test]
    fn spans_are_written_in_the_json_of_otlp() {
        let span = Span {
            trace_id: "4bf92f3577b34da6a3ce929d0e0e4736".into(),
            span_id: "00f067aa0ba902b7".into(),
            parent_span_id: Some("1111111111111111".into()),
            name: "GET /items/{n}".into(),
            kind: 2,
            start_unix_nanos: 1_700_000_000_000_000_000,
            end_unix_nanos: 1_700_000_000_500_000_000,
            attributes: vec![
                ("http.route".into(), AttrValue::Text("/items/{n}".into())),
                ("http.response.status_code".into(), AttrValue::Int(500)),
            ],
            error: Some("runtime error: boom".into()),
        };
        let body = otlp_json("shop", &[span]);
        let rs = &body["resourceSpans"][0];
        assert_eq!(
            rs["resource"]["attributes"][0]["value"]["stringValue"],
            "shop"
        );
        let s = &rs["scopeSpans"][0]["spans"][0];
        assert_eq!(s["traceId"], "4bf92f3577b34da6a3ce929d0e0e4736");
        assert_eq!(s["parentSpanId"], "1111111111111111");
        assert_eq!(s["kind"], 2);
        // Times and whole numbers are text in the JSON of OTLP: 64-bit numbers do not fit in JSON numbers.
        assert_eq!(s["startTimeUnixNano"], "1700000000000000000");
        assert_eq!(s["attributes"][1]["value"]["intValue"], "500");
        assert_eq!(s["status"]["code"], 2);
        assert_eq!(s["status"]["message"], "runtime error: boom");
        // A span with no parent has no parentSpanId.
        let root = Span::default();
        let body = otlp_json("shop", &[root]);
        let s = &body["resourceSpans"][0]["scopeSpans"][0]["spans"][0];
        assert!(s.get("parentSpanId").is_none() && s["status"]["code"] == 0);
    }

    #[test]
    fn a_call_made_while_answering_a_request_is_a_child_span() {
        // An export with no thread, so the test reads the spans that wait.
        *EXPORTER.lock().unwrap() = Some(Exporter {
            endpoint: "http://127.0.0.1:9/v1/traces".into(),
            service: "t".into(),
            pending: Vec::new(),
        });
        // Outside a request nothing is recorded.
        let _ = client_span("GET", vec![], || Ok::<_, String>(1));
        let ctx = TraceContext::from_headers(&[]);
        let parent = ctx.span_id.clone();
        with_context(ctx.clone(), || {
            let ok = client_span(
                "SELECT",
                vec![(
                    "db.system.name".into(),
                    AttrValue::Text("postgresql".into()),
                )],
                || Ok::<_, String>(7),
            );
            assert_eq!(ok, Ok(7));
            let failed = client_span("POST", vec![], || Err::<i32, _>("refused".to_string()));
            assert!(failed.is_err());
        });
        let spans = std::mem::take(&mut EXPORTER.lock().unwrap().as_mut().unwrap().pending);
        *EXPORTER.lock().unwrap() = None;
        let ours: Vec<&Span> = spans
            .iter()
            .filter(|s| s.trace_id == ctx.trace_id)
            .collect();
        assert_eq!(ours.len(), 2, "{spans:?}");
        assert!(
            ours.iter()
                .all(|s| s.parent_span_id.as_deref() == Some(parent.as_str()) && s.kind == 3)
        );
        assert_eq!((ours[0].name.as_str(), &ours[0].error), ("SELECT", &None));
        assert_eq!(
            (ours[1].name.as_str(), ours[1].error.as_deref()),
            ("POST", Some("refused"))
        );
        assert!(ours[0].end_unix_nanos >= ours[0].start_unix_nanos);
    }
}
