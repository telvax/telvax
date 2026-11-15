//! `telvax serve --watch` replaces the server when the program changes, and keeps it when the change has errors.
#![cfg(unix)]

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const TELVAX: &str = env!("CARGO_BIN_EXE_telvax");

fn app(greeting: &str) -> String {
    format!(
        r#"
type In:
    name: Text

type Out:
    greeting: Text

@endpoint(POST, "/hello")
fn hello(p: In) -> Result[Out, Text]:
    Ok(Out(greeting: f"{greeting}, {{p.name}}!"))
"#
    )
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("telvax-watch-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn greeting(port: u16) -> Option<String> {
    let body = r#"{"name":"x"}"#;
    let mut s = TcpStream::connect(("127.0.0.1", port)).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    write!(
        s,
        "POST /hello HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .ok()?;
    let mut raw = String::new();
    s.read_to_string(&mut raw).ok()?;
    let json: serde_json::Value = serde_json::from_str(raw.split("\r\n\r\n").nth(1)?).ok()?;
    json["greeting"].as_str().map(str::to_string)
}

/// Waits until the server answers `want`.
fn wait_for(port: u16, want: &str) {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(60) {
        if greeting(port).as_deref() == Some(want) {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("the server did not answer `{want}`");
}

/// Stops `telvax serve --watch` with SIGTERM, which stops its server too.
struct Watch(Child);

impl Drop for Watch {
    fn drop(&mut self) {
        let _ = Command::new("kill")
            .args(["-TERM", &self.0.id().to_string()])
            .status();
        let _ = self.0.wait();
    }
}

#[test]
fn a_change_restarts_the_server_and_a_mistake_keeps_it() {
    let src = scratch("app.tvx");
    std::fs::write(&src, app("hello")).unwrap();
    let port = free_port();
    let _watch = Watch(
        Command::new(TELVAX)
            .arg("serve")
            .arg(&src)
            .args([
                "--host",
                "127.0.0.1",
                "--port",
                &port.to_string(),
                "--watch",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    wait_for(port, "hello, x!");

    std::fs::write(&src, app("hi")).unwrap();
    wait_for(port, "hi, x!");

    // A program with an error: the old answer keeps coming.
    std::fs::write(&src, app("oops").replace("p.name", "p.nope")).unwrap();
    std::thread::sleep(Duration::from_secs(2));
    assert_eq!(greeting(port).as_deref(), Some("hi, x!"));

    std::fs::write(&src, app("fixed")).unwrap();
    wait_for(port, "fixed, x!");
}
