use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const TELVAX: &str = env!("CARGO_BIN_EXE_telvax");

const APP: &str = r#"
type In:
    name: Text

type Out:
    greeting: Text

@endpoint(POST, "/hello")
fn hello(p: In) -> Result[Out, Text]:
    Ok(Out(greeting: f"hello, {p.name}!"))
"#;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("telvax-cli-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

/// A scratch path for an executable written by `telvax build --serve` (`.exe` on Windows). Each
/// one is a full copy of the `telvax` binary (over a gigabyte in a debug build), so it is deleted
/// when the test ends, passing or not.
struct TempExe(PathBuf);

impl Drop for TempExe {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

impl AsRef<std::ffi::OsStr> for TempExe {
    fn as_ref(&self) -> &std::ffi::OsStr {
        self.0.as_os_str()
    }
}

impl AsRef<Path> for TempExe {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

fn scratch_exe(name: &str) -> TempExe {
    TempExe(scratch(&format!("{name}{}", std::env::consts::EXE_SUFFIX)))
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// The listener can accept a connection a moment before the server answers requests, so a failed
/// exchange is retried for a few seconds. The endpoints under test have no side effects.
fn post(port: u16, path: &str, body: &str) -> Option<(u16, String)> {
    for _ in 0..50 {
        if let Some(reply) = post_once(port, path, body) {
            return Some(reply);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    None
}

fn post_once(port: u16, path: &str, body: &str) -> Option<(u16, String)> {
    let mut s = TcpStream::connect(("127.0.0.1", port)).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    write!(
        s,
        "POST {path} HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .ok()?;
    let mut raw = String::new();
    s.read_to_string(&mut raw).ok()?;
    let status = raw.split_whitespace().nth(1)?.parse().ok()?;
    Some((status, raw.split("\r\n\r\n").nth(1)?.to_string()))
}

struct Server(Child);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn wait_for(port: u16) {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(30) {
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("server did not start");
}

#[test]
fn a_bundled_binary_serves_its_program() {
    let src = scratch("hello.tvx");
    std::fs::write(&src, APP).unwrap();
    let exe = scratch_exe("hello-service");
    let out = Command::new(TELVAX)
        .args(["build", "--serve"])
        .arg(&src)
        .arg("-o")
        .arg(&exe)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let port = free_port();
    let child = Command::new(&exe)
        .args(["--host", "127.0.0.1", "--port", &port.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let _server = Server(child);
    wait_for(port);
    let (status, body) = post(port, "/hello", r#"{"name":"world"}"#).unwrap();
    assert_eq!(status, 200);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body).unwrap(),
        serde_json::json!({"greeting": "hello, world!"})
    );
    assert_eq!(post(port, "/hello", r#"{"nope":1}"#).unwrap().0, 400);
}

#[test]
fn the_port_can_come_from_the_environment() {
    let src = scratch("env.tvx");
    std::fs::write(&src, APP).unwrap();
    let exe = scratch_exe("env-service");
    assert!(
        Command::new(TELVAX)
            .args(["build", "--serve"])
            .arg(&src)
            .arg("-o")
            .arg(&exe)
            .status()
            .unwrap()
            .success()
    );
    let port = free_port();
    let child = Command::new(&exe)
        .env("PORT", port.to_string())
        .env("HOST", "127.0.0.1")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let _server = Server(child);
    wait_for(port);
    assert_eq!(post(port, "/hello", r#"{"name":"env"}"#).unwrap().0, 200);
}

#[test]
fn programs_without_endpoints_are_rejected() {
    let src = scratch("noep.tvx");
    std::fs::write(
        &src,
        r#"
fn main() -> i32:
    0
"#,
    )
    .unwrap();
    let out = Command::new(TELVAX)
        .args(["build", "--serve"])
        .arg(&src)
        .arg("-o")
        .arg(scratch_exe("noep"))
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("nothing to serve"));
}

#[test]
fn the_dockerfile_is_multi_stage_and_names_the_program() {
    let dir = scratch("docker-app");
    std::fs::create_dir_all(&dir).unwrap();
    let src = dir.join("app.tvx");
    std::fs::write(&src, APP).unwrap();
    let exe = scratch_exe("docker-app/app");
    let out = Command::new(TELVAX)
        .args(["build", "--serve"])
        .arg(&src)
        .arg("-o")
        .arg(&exe)
        .arg("--dockerfile")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let docker = std::fs::read_to_string(dir.join("Dockerfile")).unwrap();
    assert_eq!(docker.matches("\nFROM ").count(), 2, "{docker}");
    // Every module and embedded file is in the image, and the compiler comes from the published image.
    assert!(docker.contains("COPY . ."), "{docker}");
    assert!(docker.contains("FROM ${TELVAX_IMAGE} AS builder"));
    assert!(docker.contains("ARG TELVAX_IMAGE=ghcr.io/telvax/telvax:latest"));
    assert!(docker.contains("mkdir -p /out && telvax build --serve app.tvx -o /out/service"));
    assert!(!docker.contains("cargo build"));
    assert!(docker.contains("COPY --from=builder /out/service /service"));
    assert!(docker.contains("USER nonroot") && docker.contains("EXPOSE 8080"));
    assert!(docker.contains(r#"HEALTHCHECK --interval=10s --timeout=4s --start-period=10s CMD ["/service", "--healthcheck"]"#));
}

#[test]
fn a_multi_file_program_is_bundled_with_all_its_modules() {
    let dir = scratch("multi-app");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("main.tvx"),
        r#"
import util.shout

type In:
    name: Text

type Out:
    greeting: Text

@endpoint(POST, "/hello")
fn hello(p: In) -> Result[Out, Text]:
    Ok(Out(greeting: shout(p.name)))
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("util.tvx"),
        r#"
fn shout(s: Text) -> Text:
    f"{s}!"
"#,
    )
    .unwrap();
    let exe = scratch_exe("multi-service");
    let out = Command::new(TELVAX)
        .args(["build", "--serve"])
        .arg(dir.join("main.tvx"))
        .arg("-o")
        .arg(&exe)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    // The service must not read the sources again: delete them.
    std::fs::remove_dir_all(&dir).unwrap();

    let port = free_port();
    let child = Command::new(&exe)
        .args(["--host", "127.0.0.1", "--port", &port.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let _server = Server(child);
    wait_for(port);
    let (status, body) = post(port, "/hello", r#"{"name":"modules"}"#).unwrap();
    assert_eq!(status, 200);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body).unwrap(),
        serde_json::json!({"greeting": "modules!"})
    );
}

#[test]
fn errors_in_a_module_are_reported_against_that_file() {
    let dir = scratch("err-app");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("main.tvx"),
        r#"
import util.bad

fn main() -> i32:
    bad()
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("util.tvx"),
        r#"
fn bad() -> i32:
    "nope"
"#,
    )
    .unwrap();
    let out = Command::new(TELVAX)
        .arg("check")
        .arg(dir.join("main.tvx"))
        .output()
        .unwrap();
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("util.tvx:3:5") && err.contains("expected `i32`, found `Text`"),
        "{err}"
    );
}

#[test]
fn tvpm_projects_run_against_installed_dependencies() {
    use telvax_pkg::{Project, registry};

    let root = scratch("tvpm-e2e");
    let _ = std::fs::remove_dir_all(&root);
    let (reg, lib, app, home) = (
        root.join("registry"),
        root.join("mathx"),
        root.join("app"),
        root.join("home"),
    );
    std::fs::create_dir_all(lib.join("src")).unwrap();
    std::fs::create_dir_all(app.join("src")).unwrap();
    std::fs::write(
        lib.join("telvax.toml"),
        "[package]\nname = \"mathx\"\nversion = \"1.3.0\"\n",
    )
    .unwrap();
    std::fs::write(
        lib.join("src/lib.tvx"),
        r#"
fn cube(x: Int) -> Int:
    x * x * x
"#,
    )
    .unwrap();
    registry::publish(&reg, &lib).unwrap();

    std::fs::write(
        app.join("telvax.toml"),
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\n[dependencies]\nmathx = \"^1.2\"\n",
    )
    .unwrap();
    std::fs::write(
        app.join("src/main.tvx"),
        r#"
import mathx.cube

fn main() -> Int:
    cube(3) + 1
"#,
    )
    .unwrap();

    // Without a lockfile the compiler asks for `tvpm install`.
    let out = Command::new(TELVAX)
        .env("TVPM_HOME", &home)
        .arg("check")
        .arg(app.join("src/main.tvx"))
        .output()
        .unwrap();
    assert!(!out.status.success() && String::from_utf8_lossy(&out.stderr).contains("tvpm install"));

    let project = Project::open(&app).unwrap();
    let r = registry::open(reg.to_str().unwrap());
    unsafe { std::env::set_var("TVPM_HOME", &home) };
    project.lock(Some(r.as_ref())).unwrap();
    project.install(Some(r.as_ref())).unwrap();

    let out = Command::new(TELVAX)
        .env("TVPM_HOME", &home)
        .args(["run"])
        .arg(app.join("src/main.tvx"))
        .arg("main")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "28");
}

const EMBED_APP: &str = r#"
type Out:
    words: u64

let lexicon = std.fs.embed("data/words.txt")

@endpoint(POST, "/count")
fn count(p: Out) -> Result[Out, Text]:
    Ok(Out(words: lexicon.split("\n").len()))
"#;

#[test]
fn embedded_files_travel_inside_the_binary() {
    let dir = scratch("embed-app");
    std::fs::create_dir_all(dir.join("data")).unwrap();
    let src = dir.join("app.tvx");
    std::fs::write(&src, EMBED_APP).unwrap();
    std::fs::write(dir.join("data/words.txt"), "good\nbad\nfine").unwrap();
    let exe = scratch_exe("embed-service");
    let out = Command::new(TELVAX)
        .args(["build", "--serve"])
        .arg(&src)
        .arg("-o")
        .arg(&exe)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    // The binary must not read the file again: it is gone when the server starts.
    std::fs::remove_dir_all(dir.join("data")).unwrap();

    let port = free_port();
    let child = Command::new(&exe)
        .args(["--host", "127.0.0.1", "--port", &port.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let _server = Server(child);
    wait_for(port);
    let (status, body) = post(port, "/count", r#"{"words":0}"#).unwrap();
    assert_eq!(status, 200);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body).unwrap(),
        serde_json::json!({"words": 3})
    );
}

#[test]
fn embedding_a_missing_file_or_a_computed_path_is_a_build_error() {
    let dir = scratch("embed-bad");
    std::fs::create_dir_all(&dir).unwrap();
    let build = |name: &str, source: &str| {
        let src = dir.join(name);
        std::fs::write(&src, source).unwrap();
        let out = Command::new(TELVAX)
            .args(["build", "--serve"])
            .arg(&src)
            .arg("-o")
            .arg(scratch_exe("embed-bad"))
            .output()
            .unwrap();
        assert!(!out.status.success());
        String::from_utf8_lossy(&out.stderr).into_owned()
    };
    let missing = build(
        "missing.tvx",
        &EMBED_APP.replace("data/words.txt", "nope.txt"),
    );
    assert!(missing.contains("cannot embed `nope.txt`"), "{missing}");
    let computed = build(
        "computed.tvx",
        &EMBED_APP.replace("\"data/words.txt\"", "\"data/\" + \"words.txt\""),
    );
    assert!(computed.contains("takes a text literal"), "{computed}");
}

#[test]
fn an_endpoint_answers_202_and_the_task_finishes_later() {
    let src = scratch("jobs.tvx");
    std::fs::write(
        &src,
        r#"
import std.http.Response

type Job:
    id: Text

type State:
    state: Text

let jobs: Map[Text, Text] = {}

async fn work(id: Text):
    await std.task.sleep(300)
    jobs.insert(id, "done")

@endpoint(POST, "/start")
fn start(job: Job) -> Result[Response, Text]:
    jobs.insert(job.id, "running")
    std.task.spawn(work(job.id))
    Ok(Response.new(State(state: "running")).status(202))

@endpoint(POST, "/status")
fn status(job: Job) -> Result[State, Text]:
    Ok(State(state: jobs.get(job.id).unwrap_or("unknown")))
"#,
    )
    .unwrap();
    let port = free_port();
    let child = Command::new(TELVAX)
        .arg("serve")
        .arg(&src)
        .args(["--host", "127.0.0.1", "--port", &port.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let _server = Server(child);
    wait_for(port);
    assert_eq!(post(port, "/start", r#"{"id":"j1"}"#).unwrap().0, 202);
    let state = |port| post(port, "/status", r#"{"id":"j1"}"#).unwrap().1;
    assert!(state(port).contains("running"));
    let start = Instant::now();
    while !state(port).contains("done") {
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "the task never finished"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

const EVENTS_APP: &str = r#"
import std.http.Response
import std.http.Sink

type Batch:
    texts: List[Text]

@endpoint(POST, "/stream")
fn stream(input: Batch) -> Result[Response, Text]:
    fn produce(sink: Sink):
        for text in input.texts:
            sink.send(text)
        sink.event("done", "{}")
    Ok(Response.events(produce))

@endpoint(POST, "/broken")
fn broken(input: Batch) -> Result[Response, Text]:
    fn produce(sink: Sink):
        sink.send("first")
        std.test.fail("boom")
    Ok(Response.events(produce))
"#;

/// The whole answer, headers included, of a POST on a connection the server closes at the end.
fn raw_post(port: u16, path: &str, body: &str) -> String {
    for _ in 0..50 {
        if let Ok(mut s) = TcpStream::connect(("127.0.0.1", port)) {
            s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            if write!(
                s,
                "POST {path} HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .is_err()
            {
                std::thread::sleep(Duration::from_millis(100));
                continue;
            }
            let mut raw = String::new();
            let _ = s.read_to_string(&mut raw);
            if !raw.is_empty() {
                return raw;
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("no answer");
}

#[test]
fn an_endpoint_streams_server_sent_events() {
    let src = scratch("events.tvx");
    std::fs::write(&src, EVENTS_APP).unwrap();
    let port = free_port();
    let child = Command::new(TELVAX)
        .arg("serve")
        .arg(&src)
        .args(["--host", "127.0.0.1", "--port", &port.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let _server = Server(child);
    wait_for(port);

    let raw = raw_post(port, "/stream", r#"{"texts":["one","two\nlines"]}"#).to_lowercase();
    assert!(raw.starts_with("http/1.1 200"), "{raw}");
    assert!(raw.contains("content-type: text/event-stream"), "{raw}");
    assert!(raw.contains("cache-control: no-cache"), "{raw}");
    let (one, two, lines, done) = (
        raw.find("data: one\n\n").unwrap(),
        raw.find("data: two\n").unwrap(),
        raw.find("data: lines\n\n").unwrap(),
        raw.find("event: done\ndata: {}\n\n").unwrap(),
    );
    assert!(one < two && two < lines && lines < done, "{raw}");

    let raw = raw_post(port, "/broken", r#"{"texts":[]}"#);
    assert!(raw.contains("data: first\n\n"), "{raw}");
    assert!(raw.contains("event: error\ndata: "), "{raw}");
    assert!(raw.contains("boom"), "{raw}");
}

fn raw_get(port: u16, path: &str) -> String {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    write!(
        s,
        "GET {path} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut raw = String::new();
    let _ = s.read_to_string(&mut raw);
    raw
}

#[test]
fn metrics_and_json_logs_come_from_flags() {
    let src = scratch("observe.tvx");
    std::fs::write(
        &src,
        format!(
            "{APP}\n@endpoint(POST, \"/hit\")\nfn hit(p: In) -> Result[Out, Text]:\n    std.metrics.count(\"cache_hit\")\n    std.metrics.count(\"cache_hit\", 2)\n    Ok(Out(greeting: p.name))\n"
        ),
    )
    .unwrap();
    let port = free_port();
    let child = Command::new(TELVAX)
        .arg("serve")
        .arg(&src)
        .args(["--host", "127.0.0.1", "--port", &port.to_string()])
        .args(["--metrics", "--log-format", "json"])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut server = Server(child);
    wait_for(port);
    assert_eq!(post(port, "/hit", r#"{"name":"a"}"#).unwrap().0, 200);

    let metrics = raw_get(port, "/metrics");
    assert!(
        metrics.contains("x-request-id:") || metrics.contains("X-Request-Id:"),
        "{metrics}"
    );
    assert!(
        metrics
            .contains(r#"telvax_http_requests_total{method="POST",route="/hit",status="200"} 1"#),
        "{metrics}"
    );
    assert!(
        metrics.contains(r#"telvax_counter_total{name="cache_hit"} 3"#),
        "{metrics}"
    );

    let _ = server.0.kill();
    let mut log = String::new();
    server
        .0
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut log)
        .unwrap();
    let line = log
        .lines()
        .find(|l| l.starts_with('{') && l.contains("\"/hit\""))
        .unwrap_or_else(|| panic!("no log line in {log}"));
    let line: serde_json::Value = serde_json::from_str(line).unwrap();
    assert_eq!(line["method"], "POST");
    assert_eq!(line["status"], 200);
    assert_eq!(line["request_id"].as_str().unwrap().len(), 16);
    assert!(line["ts"].as_str().unwrap().ends_with('Z'));
}

#[test]
fn a_client_over_the_rate_limit_gets_429_and_a_bad_limit_is_refused() {
    let src = scratch("limit.tvx");
    std::fs::write(&src, APP).unwrap();
    let bad = Command::new(TELVAX)
        .arg("serve")
        .arg(&src)
        .args(["--rate-limit", "fast"])
        .output()
        .unwrap();
    assert!(!bad.status.success());
    assert!(String::from_utf8_lossy(&bad.stderr).contains("--rate-limit takes a count and a unit"));

    let port = free_port();
    let child = Command::new(TELVAX)
        .arg("serve")
        .arg(&src)
        .args(["--host", "127.0.0.1", "--port", &port.to_string()])
        .args(["--rate-limit", "2/min", "--security-headers", "--cors", "*"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let _server = Server(child);
    wait_for(port);
    assert_eq!(post(port, "/hello", r#"{"name":"a"}"#).unwrap().0, 200);
    assert_eq!(post(port, "/hello", r#"{"name":"a"}"#).unwrap().0, 200);
    let (status, body) = post(port, "/hello", r#"{"name":"a"}"#).unwrap();
    assert_eq!(status, 429);
    assert!(body.contains("rate limit exceeded"), "{body}");
}

#[test]
fn a_body_may_leave_out_fields_with_a_default_or_an_option() {
    let src = scratch("defaults.tvx");
    std::fs::write(
        &src,
        r#"
type Input:
    text: Text
    language: Text = "en"
    top_k: Int = -1
    note: Text?

type Output:
    text: Text
    language: Text
    top_k: Int
    note: Text

@endpoint(POST, "/echo")
fn echo(input: Input) -> Result[Output, Text]:
    Ok(Output(text: input.text, language: input.language, top_k: input.top_k, note: input.note.unwrap_or("none")))
"#,
    )
    .unwrap();
    let port = free_port();
    let child = Command::new(TELVAX)
        .arg("serve")
        .arg(&src)
        .args(["--host", "127.0.0.1", "--port", &port.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let _server = Server(child);
    wait_for(port);
    let (status, body) = post(port, "/echo", r#"{"text":"hi"}"#).unwrap();
    assert_eq!(status, 200, "{body}");
    assert!(body.contains(r#""language":"en""#), "{body}");
    assert!(body.contains(r#""top_k":-1"#), "{body}");
    assert!(body.contains(r#""note":"none""#), "{body}");
    let (_, body) = post(port, "/echo", r#"{"text":"oi","language":"pt","note":"x"}"#).unwrap();
    assert!(
        body.contains(r#""language":"pt""#) && body.contains(r#""note":"x""#),
        "{body}"
    );
    // A field without a default is still required.
    let (status, body) = post(port, "/echo", r#"{"language":"pt"}"#).unwrap();
    assert_eq!(status, 400);
    assert!(
        body.contains("payload.text: missing required field"),
        "{body}"
    );
}

#[test]
fn a_bundled_server_checks_its_own_health() {
    let src = scratch("health.tvx");
    std::fs::write(&src, APP).unwrap();
    let exe = scratch_exe("health-service");
    let out = Command::new(TELVAX)
        .args(["build", "--serve"])
        .arg(&src)
        .arg("-o")
        .arg(&exe)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let port = free_port();
    // Nothing listens yet: the check fails.
    let down = Command::new(&exe)
        .args(["--healthcheck", "--port", &port.to_string()])
        .output()
        .unwrap();
    assert_eq!(
        down.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&down.stderr)
    );
    let child = Command::new(&exe)
        .args(["--host", "127.0.0.1", "--port", &port.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let _server = Server(child);
    wait_for(port);
    let up = Command::new(&exe)
        .args(["--healthcheck", "--port", &port.to_string()])
        .output()
        .unwrap();
    assert!(
        up.status.success(),
        "{}",
        String::from_utf8_lossy(&up.stderr)
    );
    // The port can come from the environment, and a path that does not exist is not healthy.
    let env = Command::new(&exe)
        .arg("--healthcheck")
        .env("PORT", port.to_string())
        .output()
        .unwrap();
    assert!(env.status.success());
    let missing = Command::new(&exe)
        .args(["--healthcheck", "/nope", "--port", &port.to_string()])
        .output()
        .unwrap();
    assert_eq!(missing.status.code(), Some(1));
}

#[test]
fn a_failed_background_task_can_mark_its_job_as_failed() {
    let src = scratch("spawn-error.tvx");
    std::fs::write(
        &src,
        r#"
import std.http.Response

type Job:
    id: Text

type State:
    state: Text

let jobs: Map[Text, Text] = {}
let ids: Map[Text, Text] = {}

async fn work(id: Text):
    await std.task.sleep(100)
    let nothing: Int? = None
    nothing.unwrap()

fn failed(message: Text):
    jobs.insert("j1", "failed")

@endpoint(POST, "/start")
fn start(job: Job) -> Result[Response, Text]:
    jobs.insert(job.id, "running")
    std.task.spawn(work(job.id), failed)
    Ok(Response.new(State(state: "running")).status(202))

@endpoint(POST, "/status")
fn status(job: Job) -> Result[State, Text]:
    Ok(State(state: jobs.get(job.id).unwrap_or("unknown")))
"#,
    )
    .unwrap();
    let port = free_port();
    let child = Command::new(TELVAX)
        .arg("serve")
        .arg(&src)
        .args(["--host", "127.0.0.1", "--port", &port.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let _server = Server(child);
    wait_for(port);
    assert_eq!(post(port, "/start", r#"{"id":"j1"}"#).unwrap().0, 202);
    let start = Instant::now();
    loop {
        let body = post(port, "/status", r#"{"id":"j1"}"#).unwrap().1;
        if body.contains("failed") {
            break;
        }
        assert!(start.elapsed().as_secs() < 5, "never failed: {body}");
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

#[test]
fn labelled_counters_and_the_time_header_reach_the_clients() {
    let src = scratch("labels.tvx");
    std::fs::write(
        &src,
        r#"
type In:
    language: Text

type Out:
    ok: Bool

@endpoint(POST, "/seen")
fn seen(input: In) -> Result[Out, Text]:
    std.metrics.count("lbl_e2e_total", {"language": input.language})
    Ok(Out(ok: True))
"#,
    )
    .unwrap();
    let port = free_port();
    let child = Command::new(TELVAX)
        .arg("serve")
        .arg(&src)
        .args(["--host", "127.0.0.1", "--port", &port.to_string()])
        .args(["--metrics", "--process-time-header"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let _server = Server(child);
    wait_for(port);
    for language in ["en", "en", "pt"] {
        assert_eq!(
            post(port, "/seen", &format!(r#"{{"language":"{language}"}}"#))
                .unwrap()
                .0,
            200
        );
    }
    let metrics = raw_get(port, "/metrics");
    assert!(
        metrics.contains("# TYPE lbl_e2e_total counter"),
        "{metrics}"
    );
    assert!(
        metrics.contains(r#"lbl_e2e_total{language="en"} 2"#),
        "{metrics}"
    );
    assert!(
        metrics.contains(r#"lbl_e2e_total{language="pt"} 1"#),
        "{metrics}"
    );
    assert!(
        raw_get(port, "/healthz")
            .to_lowercase()
            .contains("x-process-time-ms:")
    );
}

#[cfg(unix)]
#[test]
fn a_stopped_server_waits_for_its_tasks_but_not_for_ever() {
    let src = scratch("shutdown.tvx");
    std::fs::write(
        &src,
        r#"
import std.http.Response

type Job:
    id: Text

type State:
    state: Text

let jobs: Map[Text, Text] = {}

async fn work(id: Text):
    await std.task.sleep(1500)
    jobs.insert(id, "done")

@endpoint(POST, "/start")
fn start(job: Job) -> Result[Response, Text]:
    std.task.spawn(work(job.id))
    Ok(Response.new(State(state: "running")).status(202))
"#,
    )
    .unwrap();
    let stop = |pid: u32| {
        Command::new("kill")
            .args(["-TERM", &pid.to_string()])
            .status()
            .unwrap();
    };
    for (wait, finishes) in [("30", true), ("0", false)] {
        let port = free_port();
        let mut child = Command::new(TELVAX)
            .arg("serve")
            .arg(&src)
            .args(["--host", "127.0.0.1", "--port", &port.to_string()])
            .args(["--shutdown-wait", wait])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        wait_for(port);
        assert_eq!(post(port, "/start", r#"{"id":"a"}"#).unwrap().0, 202);
        let started = Instant::now();
        stop(child.id());
        let status = child.wait().unwrap();
        if finishes {
            // It waited for the task, then left cleanly.
            assert!(status.success(), "{status:?}");
            assert!(started.elapsed().as_millis() >= 1000, "left too early");
        } else {
            assert_eq!(status.code(), Some(1), "{status:?}");
            assert!(started.elapsed().as_millis() < 1400, "waited for the task");
        }
    }
}

#[test]
fn errors_can_be_written_as_detail_with_422() {
    let src = scratch("detail.tvx");
    std::fs::write(
        &src,
        r#"
import std.http.Status

type In:
    text: Text
    top_k: Int = 3

type Out:
    n: Int

@endpoint(POST, "/count")
fn count(input: In) -> Result[Out, Status]:
    if input.text.is_empty():
        return Err(Status.BadRequest("No text provided"))
    Ok(Out(n: input.top_k))
"#,
    )
    .unwrap();
    for (format, bad_status, expect) in [
        ("telvax", 400, "\"error\":\"invalid request\""),
        ("detail", 422, "\"detail\":[\"payload.top_k: expected Int"),
    ] {
        let port = free_port();
        let child = Command::new(TELVAX)
            .arg("serve")
            .arg(&src)
            .args(["--host", "127.0.0.1", "--port", &port.to_string()])
            .args(["--error-format", format])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let _server = Server(child);
        wait_for(port);
        let (status, body) = post(port, "/count", r#"{"text":"x","top_k":"many"}"#).unwrap();
        assert_eq!(status, bad_status, "{format}: {body}");
        assert!(body.contains(expect), "{format}: {body}");
        // An error the program returns keeps its status; only the key changes.
        let (status, body) = post(port, "/count", r#"{"text":""}"#).unwrap();
        assert_eq!(status, 400, "{format}: {body}");
        if format == "detail" {
            assert!(body.contains(r#"{"detail":"No text provided"}"#), "{body}");
            // The body of an unknown route is a detail too.
            let (status, body) = post(port, "/nope", "{}").unwrap();
            assert_eq!(status, 404);
            assert!(body.contains(r#"{"detail":"Not Found"}"#), "{body}");
        } else {
            assert!(body.contains(r#"{"error":"No text provided"}"#), "{body}");
        }
    }
}

const SERVE_BLOCK_APP: &str = r#"
import std.http.Status

serve:
    metrics: True
    process_time_header: True
    error_format: "detail"

type In:
    text: Text

type Out:
    n: Int

@endpoint(POST, "/count")
fn count(input: In) -> Result[Out, Status]:
    Ok(Out(n: 1))
"#;

#[test]
fn a_serve_block_sets_the_options_and_a_flag_wins() {
    let src = scratch("serve-block.tvx");
    std::fs::write(&src, SERVE_BLOCK_APP).unwrap();
    for (flags, detail) in [(vec![], true), (vec!["--error-format", "telvax"], false)] {
        let port = free_port();
        let child = Command::new(TELVAX)
            .arg("serve")
            .arg(&src)
            .args(["--host", "127.0.0.1", "--port", &port.to_string()])
            .args(&flags)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let _server = Server(child);
        wait_for(port);
        let (status, body) = post(port, "/count", r#"{"text":1}"#).unwrap();
        assert_eq!(
            body.contains(r#""detail""#),
            detail,
            "{flags:?}: {status} {body}"
        );
        // The block turns the metrics and the time header on without a flag.
        assert!(raw_get(port, "/metrics").contains("# TYPE"), "{flags:?}");
        assert!(
            raw_get(port, "/healthz")
                .to_lowercase()
                .contains("x-process-time-ms:")
        );
    }
}

#[test]
fn the_log_level_leaves_out_the_lines_below_it() {
    let src = scratch("log-level.tvx");
    std::fs::write(&src, "serve:\n    log_format: \"text\"\n    log_level: \"warn\"\n\n".to_string() + &SERVE_BLOCK_APP.replace("serve:\n    metrics: True\n    process_time_header: True\n    error_format: \"detail\"\n", "")).unwrap();
    // The block says `warn`; the flag says `info` and wins.
    for (flags, info_lines) in [(vec![], false), (vec!["--log-level", "info"], true)] {
        let port = free_port();
        let log = scratch("log-level.log");
        let child = Command::new(TELVAX)
            .arg("serve")
            .arg(&src)
            .args(["--host", "127.0.0.1", "--port", &port.to_string()])
            .args(&flags)
            .stdout(Stdio::null())
            .stderr(std::fs::File::create(&log).unwrap())
            .spawn()
            .unwrap();
        let server = Server(child);
        wait_for(port);
        post(port, "/count", r#"{"text":"a"}"#).unwrap();
        post(port, "/nope", "{}").unwrap();
        drop(server);
        let lines = std::fs::read_to_string(&log).unwrap();
        assert!(lines.contains("warn POST /nope 404"), "{flags:?}: {lines}");
        assert_eq!(
            lines.contains("info POST /count 200"),
            info_lines,
            "{flags:?}: {lines}"
        );
    }
}

#[test]
fn std_log_writes_in_the_format_and_at_the_level_of_the_server() {
    let src = scratch("std-log.tvx");
    std::fs::write(
        &src,
        r#"
import std.http.Status

type In:
    text: Text

type Out:
    n: Int

@endpoint(POST, "/count")
fn count(input: In) -> Result[Out, Status]:
    std.log.info("counting", {"text": input.text})
    std.log.warn("slow count")
    Ok(Out(n: 1))
"#,
    )
    .unwrap();
    let port = free_port();
    let log = scratch("std-log.log");
    let child = Command::new(TELVAX)
        .arg("serve")
        .arg(&src)
        .args(["--host", "127.0.0.1", "--port", &port.to_string()])
        .args(["--log-format", "json", "--log-level", "warn"])
        .stdout(Stdio::null())
        .stderr(std::fs::File::create(&log).unwrap())
        .spawn()
        .unwrap();
    let server = Server(child);
    wait_for(port);
    post(port, "/count", r#"{"text":"a"}"#).unwrap();
    drop(server);
    let lines: Vec<serde_json::Value> = std::fs::read_to_string(&log)
        .unwrap()
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    let warn = lines.iter().find(|l| l["message"] == "slow count");
    assert_eq!(warn.expect("the warn line")["level"], "warn", "{lines:?}");
    assert!(
        lines.iter().all(|l| l["message"] != "counting"),
        "{lines:?}"
    );
}

#[test]
fn a_bundled_binary_keeps_the_serve_block() {
    let src = scratch("serve-block-bundle.tvx");
    std::fs::write(&src, SERVE_BLOCK_APP).unwrap();
    let exe = scratch_exe("serve-block-service");
    let out = Command::new(TELVAX)
        .args(["build", "--serve"])
        .arg(&src)
        .arg("-o")
        .arg(&exe)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let port = free_port();
    let child = Command::new(&exe)
        .args(["--host", "127.0.0.1", "--port", &port.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let _server = Server(child);
    wait_for(port);
    let (_, body) = post(port, "/count", r#"{"text":1}"#).unwrap();
    assert!(body.contains(r#""detail""#), "{body}");
    assert!(raw_get(port, "/metrics").contains("# TYPE"));
}

#[test]
fn check_reports_a_wrong_serve_block() {
    let src = scratch("serve-wrong.tvx");
    for (block, expect) in [
        (
            "    colour: True\n",
            "`colour` is not an option of `serve:`",
        ),
        ("    metrics: 3\n", "`metrics` takes True or False"),
        (
            "    log_format: \"yaml\"\n",
            "`log_format` takes \"json\" or \"text\"",
        ),
        (
            "    log_level: \"loud\"\n",
            "`log_level` takes \"debug\", \"info\", \"warn\" or \"error\"",
        ),
        (
            "    rate_limit: \"fast\"\n",
            "`rate_limit` takes a count and a unit",
        ),
        (
            "    metrics: True\n    metrics: False\n",
            "`metrics` is written twice",
        ),
    ] {
        std::fs::write(&src, format!("serve:\n{block}")).unwrap();
        let out = Command::new(TELVAX)
            .arg("check")
            .arg(&src)
            .output()
            .unwrap();
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(!out.status.success(), "{block}");
        assert!(err.contains(expect), "{block}: {err}");
    }
}

#[test]
fn a_serve_block_belongs_in_the_entry_file() {
    let dir = scratch("serve-module");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("settings.tvx"),
        "serve:\n    metrics: True\n\nfn helper():\n    pass\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("main.tvx"),
        "from settings import helper\n\nfn main():\n    pass\n",
    )
    .unwrap();
    let out = Command::new(TELVAX)
        .arg("check")
        .arg(dir.join("main.tvx"))
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success());
    assert!(err.contains("`serve:` goes in the entry file"), "{err}");
}
