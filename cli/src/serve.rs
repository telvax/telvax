//! `telvax serve` and the server half of bundled binaries.

use std::collections::BTreeMap;
use std::sync::Arc;

use telvax_hir::hir;
use telvax_runtime::Interp;
use telvax_server::{HandlerError, Reply, reply_handler};

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
}

impl Options {
    /// Parses `--port N`, `--host H`, `--seed FILE`, `--live`, `--allow LIST`, `--metrics` and
    /// `--log-format json|text`. The last two can also come from `TELVAX_METRICS` and
    /// `TELVAX_LOG_FORMAT`; a flag wins.
    pub fn parse(default_host: &str, flags: &[&str]) -> Result<Options, CliError> {
        let mut opts = Options {
            host: default_host.to_string(),
            port: 8080,
            seed: None,
            live: false,
            allow: telvax_runtime::Caps::ALL,
            quantize: telvax_hir::Quantization::None,
            device: telvax_runtime::Device::Cpu,
            metrics: env_flag("TELVAX_METRICS"),
            log: match std::env::var("TELVAX_LOG_FORMAT") {
                Ok(v) if !v.is_empty() => Some(log_format(&v)?),
                _ => None,
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
                _ => return Err(CliError::Usage),
            }
        }
        Ok(opts)
    }
}

/// Whether an environment variable is set to `1` or `true`.
fn env_flag(name: &str) -> bool {
    std::env::var(name).is_ok_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
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
    let interp = Arc::new(
        Interp::with_caps(program, opts.live, opts.allow).or_else(|e| failed(e.to_string()))?,
    );
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
                    .map(|r| Reply {
                        status: r.status,
                        headers: r.headers,
                        body: r.body,
                    })
                    .map_err(|e| HandlerError::new(e.status, e.message))
                }
            }),
        );
    }
    let layers = telvax_server::Options {
        metrics: opts.metrics,
        log: opts.log,
        counters: Some(Arc::new(telvax_runtime::counters)),
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
        axum::serve(listener, app)
            .with_graceful_shutdown(shutdown_signal())
            .await
            .or_else(|e| failed(e.to_string()))
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
