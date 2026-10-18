//! A worker: an HTTP service that runs calls of Telvax programs sent by a coordinator.
//!
//! * `POST /load`  `{entry, files, deps}` -> `{program}`  (idempotent: the id is a content hash)
//! * `POST /call`  `{program, function, args}` -> `{result}` or `{error}`
//! * `GET  /health` -> `{status, cpus, programs, calls}`
//!
//! A worker runs the code it is sent, so it must only be reachable by machines you trust. It
//! listens on the loopback interface by default and can require a bearer token.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use telvax_modules::Image;
use telvax_runtime::Interp;

impl std::fmt::Debug for Worker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Worker")
    }
}

pub struct Worker {
    token: Option<String>,
    programs: Mutex<HashMap<String, Arc<Interp>>>,
    calls: AtomicU64,
}

impl Worker {
    pub fn new(token: Option<String>) -> Arc<Worker> {
        Arc::new(Worker {
            token,
            programs: Mutex::new(HashMap::new()),
            calls: AtomicU64::new(0),
        })
    }

    fn authorised(&self, headers: &HeaderMap) -> bool {
        match &self.token {
            None => true,
            Some(t) => {
                headers.get("authorization").and_then(|v| v.to_str().ok())
                    == Some(&format!("Bearer {t}"))
            }
        }
    }

    /// Builds the program from its image and keeps it under its content hash.
    fn load(&self, body: &Value) -> Result<String, String> {
        let image: Image =
            serde_json::from_value(body.clone()).map_err(|e| format!("bad program image: {e}"))?;
        let id: String = Sha256::digest(serde_json::to_vec(&image).map_err(|e| e.to_string())?)
            .iter()
            .take(12)
            .map(|b| format!("{b:02x}"))
            .collect();
        if self
            .programs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains_key(&id)
        {
            return Ok(id);
        }
        telvax_runtime::set_embedded_files(image.data.clone());
        let loaded = image.load();
        if let Some(d) = loaded.diagnostics.iter().find(|d| d.is_error()) {
            return Err(format!("the program does not parse: {}", d.message));
        }
        let (hir, diags) = telvax_hir::lower(&loaded.program);
        if let Some(d) = diags.iter().find(|d| d.is_error()) {
            return Err(format!("the program does not type-check: {}", d.message));
        }
        let interp = Interp::new(hir).map_err(|e| e.to_string())?;
        self.programs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id.clone(), Arc::new(interp));
        Ok(id)
    }
}

fn reject(status: StatusCode, message: impl Into<String>) -> (StatusCode, Json<Value>) {
    (status, Json(json!({ "error": message.into() })))
}

pub fn router(worker: Arc<Worker>) -> Router {
    async fn health(State(w): State<Arc<Worker>>) -> Json<Value> {
        Json(json!({
            "status": "ok",
            "cpus": std::thread::available_parallelism().map_or(1, |n| n.get()),
            "programs": w.programs.lock().unwrap_or_else(|e| e.into_inner()).len(),
            "calls": w.calls.load(Ordering::Relaxed),
        }))
    }

    async fn load(
        State(w): State<Arc<Worker>>,
        headers: HeaderMap,
        Json(body): Json<Value>,
    ) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
        if !w.authorised(&headers) {
            return Err(reject(
                StatusCode::UNAUTHORIZED,
                "missing or wrong bearer token",
            ));
        }
        let id = tokio::task::spawn_blocking(move || w.load(&body))
            .await
            .map_err(|e| reject(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
            .map_err(|e| reject(StatusCode::UNPROCESSABLE_ENTITY, e))?;
        Ok(Json(json!({ "program": id })))
    }

    async fn call(
        State(w): State<Arc<Worker>>,
        headers: HeaderMap,
        Json(body): Json<Value>,
    ) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
        if !w.authorised(&headers) {
            return Err(reject(
                StatusCode::UNAUTHORIZED,
                "missing or wrong bearer token",
            ));
        }
        let program = body["program"].as_str().unwrap_or_default().to_string();
        let interp = w
            .programs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&program)
            .cloned();
        // 404 tells the coordinator to send the program again (e.g. after a worker restart).
        let Some(interp) = interp else {
            return Err(reject(
                StatusCode::NOT_FOUND,
                format!("unknown program `{program}`"),
            ));
        };
        let function = body["function"].as_str().unwrap_or_default().to_string();
        let args = body["args"].as_array().cloned().unwrap_or_default();
        w.calls.fetch_add(1, Ordering::Relaxed);
        let result = tokio::task::spawn_blocking(move || interp.call_json(&function, &args))
            .await
            .map_err(|e| {
                reject(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("the call panicked: {e}"),
                )
            })?;
        match result {
            Ok(v) => Ok(Json(json!({ "result": v }))),
            Err(e) => Err(reject(StatusCode::UNPROCESSABLE_ENTITY, e)),
        }
    }

    Router::new()
        .route("/health", get(health))
        .route("/load", post(load))
        .route("/call", post(call))
        .layer(axum::extract::DefaultBodyLimit::max(64 * 1024 * 1024))
        .with_state(worker)
}

/// Serves a worker on `addr` until Ctrl-C.
pub fn serve(addr: &str, token: Option<String>) -> Result<(), String> {
    let rt = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    rt.block_on(async {
        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .map_err(|e| format!("cannot bind {addr}: {e}"))?;
        eprintln!(
            "telvax worker listening on {} {}",
            listener.local_addr().map_err(|e| e.to_string())?,
            if token.is_some() {
                "(token required)"
            } else {
                "(no token: trusted network only)"
            }
        );
        axum::serve(listener, router(Worker::new(token)))
            .with_graceful_shutdown(async {
                let _ = tokio::signal::ctrl_c().await;
            })
            .await
            .map_err(|e| e.to_string())
    })
}

/// A worker running on a background thread, for tests and embedding.
#[derive(Debug)]
pub struct WorkerHandle {
    pub addr: std::net::SocketAddr,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl WorkerHandle {
    /// `host:port` for `ClusterExecutor::new`.
    pub fn address(&self) -> String {
        self.addr.to_string()
    }

    /// Stops the worker; later requests are refused, like a machine that went away.
    pub fn shutdown(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for WorkerHandle {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Starts a worker on a free loopback port.
pub fn spawn(token: Option<String>) -> Result<WorkerHandle, String> {
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let thread = std::thread::spawn(move || {
        let Ok(rt) = tokio::runtime::Runtime::new() else {
            return;
        };
        rt.block_on(async {
            let Ok(listener) = tokio::net::TcpListener::bind("127.0.0.1:0").await else {
                return;
            };
            let Ok(addr) = listener.local_addr() else {
                return;
            };
            let _ = ready_tx.send(addr);
            let _ = axum::serve(listener, router(Worker::new(token)))
                .with_graceful_shutdown(async {
                    let _ = stop_rx.await;
                })
                .await;
        });
    });
    let addr = ready_rx
        .recv()
        .map_err(|_| "the worker thread failed to start".to_string())?;
    Ok(WorkerHandle {
        addr,
        stop: Some(stop_tx),
        thread: Some(thread),
    })
}
