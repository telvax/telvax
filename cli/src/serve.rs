//! `telvax serve` and the server half of bundled binaries.

use std::collections::BTreeMap;
use std::sync::Arc;

use telvax_hir::hir;
use telvax_runtime::Interp;
use telvax_server::{EventStream, HandlerError, Reply, reply_handler};

use crate::{CliError, failed};

#[derive(Debug, Clone, PartialEq)]
pub struct Options {
    pub host: String,
    pub port: u16,
    pub seed: Option<String>,
    pub live: bool,
    pub allow: telvax_runtime::Caps,
    pub quantize: telvax_hir::Quantization,
    pub device: telvax_runtime::Device,
    /// Serve `GET /metrics`.
    pub metrics: bool,
    /// Write one log line per request.
    pub log: Option<telvax_server::LogFormat>,
    /// Origins allowed by CORS.
    pub cors: Option<Vec<String>>,
    pub rate_limit: Option<telvax_server::RateLimit>,
    pub security_headers: bool,
    pub trust_proxy: bool,
    /// Add `x-process-time-ms` to every answer.
    pub process_time: bool,
    /// How long a stopping server waits for its requests and tasks.
    pub shutdown_wait: std::time::Duration,
    /// How the errors of the server are written.
    pub error_format: telvax_server::ErrorFormat,
}

impl Options {
    /// Parses `--port N`, `--host H`, `--seed FILE`, `--live`, `--allow LIST`, `--metrics` and
    /// `--log-format json|text`, `--cors ORIGINS`, `--rate-limit N/unit`, `--security-headers` and
    /// `--trust-proxy`. Each option after `--allow` that is not about where the program runs can
    /// also be a line of the `serve:` block of the program (`block`), or come from an environment
    /// variable (`TELVAX_METRICS`, `TELVAX_LOG_FORMAT`, `TELVAX_CORS`, `TELVAX_RATE_LIMIT`,
    /// `TELVAX_SECURITY_HEADERS`, `TELVAX_TRUST_PROXY`); the order is block, variable, flag.
    pub fn parse(
        default_host: &str,
        flags: &[&str],
        block: &[(String, hir::FieldDefault)],
    ) -> Result<Options, CliError> {
        // The `serve:` block comes first: the variables and the flags override it.
        let text = |name: &str| {
            block.iter().find_map(|(n, v)| match v {
                hir::FieldDefault::Text(t) if n == name => Some(t.as_str()),
                _ => None,
            })
        };
        let on = |name: &str| {
            block
                .iter()
                .any(|(n, v)| n == name && *v == hir::FieldDefault::Bool(true))
        };
        let mut opts = Options {
            host: default_host.to_string(),
            port: 8080,
            seed: None,
            live: false,
            allow: telvax_runtime::Caps::ALL,
            quantize: telvax_hir::Quantization::None,
            device: telvax_runtime::Device::Cpu,
            metrics: on("metrics") || env_flag("TELVAX_METRICS"),
            log: match std::env::var("TELVAX_LOG_FORMAT") {
                Ok(v) if !v.is_empty() => Some(log_format(&v)?),
                _ => text("log_format").map(log_format).transpose()?,
            },
            cors: std::env::var("TELVAX_CORS")
                .ok()
                .filter(|v| !v.is_empty())
                .or_else(|| text("cors").map(String::from))
                .map(|v| origins(&v)),
            rate_limit: match std::env::var("TELVAX_RATE_LIMIT") {
                Ok(v) if !v.is_empty() => Some(rate_limit(&v)?),
                _ => text("rate_limit").map(rate_limit).transpose()?,
            },
            security_headers: on("security_headers") || env_flag("TELVAX_SECURITY_HEADERS"),
            trust_proxy: on("trust_proxy") || env_flag("TELVAX_TRUST_PROXY"),
            process_time: on("process_time_header") || env_flag("TELVAX_PROCESS_TIME_HEADER"),
            shutdown_wait: match std::env::var("TELVAX_SHUTDOWN_WAIT") {
                Ok(v) if !v.is_empty() => shutdown_wait(&v)?,
                _ => match block.iter().find(|(n, _)| n == "shutdown_wait") {
                    Some((_, hir::FieldDefault::Int(n))) => {
                        std::time::Duration::from_secs((*n).max(0) as u64)
                    }
                    _ => std::time::Duration::from_secs(30),
                },
            },
            error_format: match std::env::var("TELVAX_ERROR_FORMAT") {
                Ok(v) if !v.is_empty() => error_format(&v)?,
                _ => text("error_format")
                    .map(error_format)
                    .transpose()?
                    .unwrap_or(telvax_server::ErrorFormat::Telvax),
            },
        };
        let mut it = flags.iter();
        while let Some(flag) = it.next() {
            if *flag == "--live" {
                opts.live = true;
                continue;
            }
            if *flag == "--metrics" {
                opts.metrics = true;
                continue;
            }
            if *flag == "--security-headers" {
                opts.security_headers = true;
                continue;
            }
            if *flag == "--trust-proxy" {
                opts.trust_proxy = true;
                continue;
            }
            if *flag == "--process-time-header" {
                opts.process_time = true;
                continue;
            }
            if *flag == "--allow" {
                let (c, _) = crate::take_allow(&["--allow", it.next().copied().unwrap_or("")])?;
                opts.allow = c;
                continue;
            }
            if *flag == "--quantize" {
                let (q, _) =
                    crate::take_quantize(&["--quantize", it.next().copied().unwrap_or("")])?;
                opts.quantize = q;
                continue;
            }
            if *flag == "--device" {
                let (d, _) = crate::take_device(&["--device", it.next().copied().unwrap_or("")])?;
                opts.device = d;
                continue;
            }
            match (*flag, it.next()) {
                ("--port", Some(p)) => {
                    opts.port = p
                        .parse()
                        .or_else(|_| failed(format!("invalid port `{p}`")))?
                }
                ("--host", Some(h)) => opts.host = h.to_string(),
                ("--seed", Some(f)) => opts.seed = Some(f.to_string()),
                ("--log-format", Some(f)) => opts.log = Some(log_format(f)?),
                ("--cors", Some(f)) => opts.cors = Some(origins(f)),
                ("--rate-limit", Some(f)) => opts.rate_limit = Some(rate_limit(f)?),
                ("--shutdown-wait", Some(f)) => opts.shutdown_wait = shutdown_wait(f)?,
                ("--error-format", Some(f)) => opts.error_format = error_format(f)?,
                _ => return Err(CliError::Usage),
            }
        }
        Ok(opts)
    }
}

/// Seconds to wait for the work in progress when the server is stopped.
fn shutdown_wait(seconds: &str) -> Result<std::time::Duration, CliError> {
    match seconds.trim().parse::<u64>() {
        Ok(n) => Ok(std::time::Duration::from_secs(n)),
        Err(_) => failed(format!(
            "--shutdown-wait takes a number of seconds, not `{seconds}`"
        )),
    }
}

/// Whether an environment variable is set to `1` or `true`.
fn env_flag(name: &str) -> bool {
    std::env::var(name).is_ok_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
}

/// `*` or origins separated by commas.
fn origins(list: &str) -> Vec<String> {
    list.split(',')
        .map(|o| o.trim().trim_end_matches('/').to_string())
        .filter(|o| !o.is_empty())
        .collect()
}

/// `100/min`: a count, then `second`, `minute` or `hour` (or `s`, `m`, `h`).
fn rate_limit(spec: &str) -> Result<telvax_server::RateLimit, CliError> {
    let bad = || {
        failed(format!(
            "--rate-limit takes a count and a unit, such as `100/minute`, not `{spec}`"
        ))
    };
    let Some((count, unit)) = spec.split_once('/') else {
        return bad();
    };
    let seconds = match unit {
        "s" | "sec" | "second" => 1,
        "m" | "min" | "minute" => 60,
        "h" | "hour" => 3600,
        _ => return bad(),
    };
    match count.trim().parse::<u32>() {
        Ok(max) if max > 0 => Ok(telvax_server::RateLimit {
            max,
            window: std::time::Duration::from_secs(seconds),
        }),
        _ => bad(),
    }
}

fn error_format(name: &str) -> Result<telvax_server::ErrorFormat, CliError> {
    match name {
        "telvax" => Ok(telvax_server::ErrorFormat::Telvax),
        "detail" => Ok(telvax_server::ErrorFormat::Detail),
        other => failed(format!(
            "--error-format takes `telvax` or `detail`, not `{other}`"
        )),
    }
}

fn log_format(name: &str) -> Result<telvax_server::LogFormat, CliError> {
    match name {
        "json" => Ok(telvax_server::LogFormat::Json),
        "text" => Ok(telvax_server::LogFormat::Text),
        other => failed(format!(
            "--log-format takes `json` or `text`, not `{other}`"
        )),
    }
}

/// Loads `{"collection": .., "id": .., "text": ..}` lines into every database the program opened.
fn seed(interp: &Interp, path: &str) -> Result<usize, CliError> {
    let data =
        std::fs::read_to_string(path).or_else(|e| failed(format!("cannot read `{path}`: {e}")))?;
    let dbs = interp.dbs();
    let mut n = 0;
    for (i, line) in data
        .lines()
        .enumerate()
        .filter(|(_, l)| !l.trim().is_empty())
    {
        let doc: serde_json::Value =
            serde_json::from_str(line).or_else(|e| failed(format!("{path}:{}: {e}", i + 1)))?;
        let field = |k: &str| doc[k].as_str().map(str::to_string);
        let (Some(col), Some(id), Some(text)) = (field("collection"), field("id"), field("text"))
        else {
            return failed(format!(
                "{path}:{}: expected string fields `collection`, `id`, `text`",
                i + 1
            ));
        };
        let vector = interp.embed(&text).or_else(|e| failed(e.to_string()))?;
        for db in &dbs {
            db.insert(&col, &id, &text, vector.clone())
                .or_else(|e| failed(e.to_string()))?;
        }
        n += 1;
    }
    Ok(n)
}

/// Serves the `@endpoint` functions of `program` until Ctrl-C / SIGTERM.
pub fn run(
    program: hir::Program,
    opts: &Options,
    cluster: Option<(Vec<String>, crate::bundle::Bundle)>,
) -> Result<(), CliError> {
    let routes = telvax_server::routes(&program);
    if routes.is_empty() {
        return failed("no `@endpoint` functions found, nothing to serve");
    }
    let interp = Interp::with_caps(program, opts.live, opts.allow)
        .or_else(|e| failed(e.to_string()))?
        .into_shared();
    interp.set_quantization(opts.quantize);
    interp.set_device(opts.device);
    if let Some((workers, image)) = cluster {
        crate::attach_cluster(&interp, workers, &image)?;
        eprintln!("std::dist::map runs on the cluster");
    }
    if opts.live {
        eprintln!("live mode: using the real Ollama / OpenAI / PostgreSQL services");
    }
    if let Some(path) = &opts.seed {
        let n = seed(&interp, path)?;
        eprintln!("seeded {n} document(s) from {path}");
    }

    let mut handlers = BTreeMap::new();
    for route in &routes {
        let (interp, name) = (interp.clone(), route.name.clone());
        handlers.insert(
            route.name.clone(),
            reply_handler(move |call| {
                let (interp, name) = (interp.clone(), name.clone());
                let interp2 = interp.clone();
                async move {
                    // The evaluator is synchronous and may be CPU-heavy; keep it off the async workers.
                    tokio::task::spawn_blocking(move || {
                        let request = telvax_runtime::RequestInfo {
                            method: call.request.method,
                            path: call.request.path,
                            headers: call.request.headers,
                        };
                        interp.call_endpoint_reply(&name, &call.params, &call.payload, &request)
                    })
                    .await
                    .map_err(|e| HandlerError::new(500, format!("handler panicked: {e}")))?
                    .map(|r| {
                        let events = r.events.map(|producer| start_events(&interp2, producer));
                        Reply {
                            status: r.status,
                            headers: r.headers,
                            body: r.body,
                            events,
                        }
                    })
                    .map_err(|e| HandlerError::new(e.status, e.message))
                }
            }),
        );
    }
    let draining = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let layers = telvax_server::Options {
        metrics: opts.metrics,
        log: opts.log,
        counters: Some(Arc::new(telvax_runtime::counters)),
        cors: opts.cors.clone(),
        rate_limit: opts.rate_limit,
        security_headers: opts.security_headers,
        trust_proxy: opts.trust_proxy,
        process_time: opts.process_time,
        draining: Some(draining.clone()),
        error_format: opts.error_format,
    };
    let app = telvax_server::router_with(&routes, handlers, &layers)
        .or_else(|e| failed(e.to_string()))?;

    let rt = tokio::runtime::Runtime::new().or_else(|e| failed(e.to_string()))?;
    rt.block_on(async {
        let addr = (opts.host.as_str(), opts.port);
        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .or_else(|e| failed(format!("cannot bind {}:{}: {e}", opts.host, opts.port)))?;
        for r in &routes {
            eprintln!(
                "  {:?} http://{}:{}{}",
                r.method, opts.host, opts.port, r.path
            );
        }
        eprintln!(
            "listening on http://{}:{} (Ctrl-C to stop)",
            opts.host, opts.port
        );
        // The first signal starts the stop: `/healthz` says so, no new connection is taken, and the
        // requests in progress are finished. The wait for all of it, and then for the background
        // tasks, is limited, and a second signal ends it at once.
        let (stopped, first_signal) = tokio::sync::oneshot::channel::<()>();
        let flag = draining.clone();
        let signal = async move {
            shutdown_signal().await;
            flag.store(true, std::sync::atomic::Ordering::Relaxed);
            let _ = stopped.send(());
        };
        let mut server = Box::pin(std::future::IntoFuture::into_future(
            axum::serve(
                listener,
                app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
            )
            .with_graceful_shutdown(signal),
        ));
        let wait = opts.shutdown_wait;
        let give_up = async {
            let _ = first_signal.await;
            eprintln!(
                "stopping: waiting up to {}s for the work in progress",
                wait.as_secs()
            );
            tokio::select! {
                () = tokio::time::sleep(wait) => {}
                () = shutdown_signal() => {}
            }
        };
        let cut_requests = tokio::select! {
            r = &mut server => {
                r.or_else(|e| failed(e.to_string()))?;
                false
            }
            () = give_up => true,
        };
        if !draining.load(std::sync::atomic::Ordering::Relaxed) {
            return Ok(());
        }
        let deadline = std::time::Instant::now() + wait;
        let mut tasks = interp.background_tasks();
        while tasks > 0 && !cut_requests && std::time::Instant::now() < deadline {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            tasks = interp.background_tasks();
        }
        if cut_requests || tasks > 0 {
            return failed(format!(
                "stopped with {} background task(s) unfinished{}",
                tasks,
                if cut_requests {
                    " and requests cut"
                } else {
                    ""
                }
            ));
        }
        eprintln!("stopped");
        Ok(())
    })
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        if let Ok(mut term) = signal(SignalKind::terminate()) {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {}
                _ = term.recv() => {}
            }
            return;
        }
    }
    let _ = tokio::signal::ctrl_c().await;
}

/// Runs the producer of a `Response.events` answer on a blocking thread, feeding a bounded channel
/// that the server reads: a slow client slows the producer down. If the producer fails, the stream
/// ends with an `error` event.
fn start_events(interp: &Arc<Interp>, producer: telvax_runtime::Value) -> EventStream {
    let (tx, rx) = tokio::sync::mpsc::channel::<String>(16);
    let interp = interp.clone();
    tokio::task::spawn_blocking(move || {
        let sender = tx.clone();
        let send: telvax_runtime::EventSink =
            Arc::new(move |event| sender.blocking_send(event).is_ok());
        if let Err(e) = interp.run_events(&producer, send) {
            let data = e.to_string().replace('\n', " ");
            let _ = tx.blocking_send(format!("event: error\ndata: {data}\n\n"));
        }
    });
    EventStream::new(rx)
}
