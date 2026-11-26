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
        let (trace_id, flags) = match find("traceparent").and_then(parse_traceparent) {
            Some((trace, _, flags)) => (trace, flags),
            None => (random_hex(16), "01".to_string()),
        };
        TraceContext {
            trace_id,
            span_id: new_span_id(),
            flags,
            request_id: find("x-request-id").map(str::to_string),
        }
    }

    /// The `traceparent` for a call this span makes: the same trace, a new span whose parent is this one.
    pub fn child_traceparent(&self) -> String {
        format!("00-{}-{}-{}", self.trace_id, new_span_id(), self.flags)
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
}
