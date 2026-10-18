//! Live-mode clients, exercised against local stand-in HTTP servers.

#![cfg(feature = "live")]

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::thread;

use telvax_runtime::{Interp, Value};

/// Environment variables are process-global, so these tests run one at a time.
static ENV: Mutex<()> = Mutex::new(());

struct Seen {
    request_line: String,
    headers: String,
    body: String,
}

/// Serves `replies` (status, JSON body) in order on a random port and records the requests.
fn mock(replies: Vec<(u16, String)>) -> (String, Arc<Mutex<Vec<Seen>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    thread::spawn(move || {
        for (status, body) in replies {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let mut buf = Vec::new();
            let mut chunk = [0u8; 4096];
            let (head_end, content_length) = loop {
                let n = stream.read(&mut chunk).unwrap_or(0);
                if n == 0 {
                    break (buf.len(), 0);
                }
                buf.extend_from_slice(&chunk[..n]);
                if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&buf[..i]).to_lowercase();
                    let len = head
                        .lines()
                        .find_map(|l| l.strip_prefix("content-length:"))
                        .and_then(|v| v.trim().parse::<usize>().ok())
                        .unwrap_or(0);
                    break (i + 4, len);
                }
            };
            while buf.len() < head_end + content_length {
                let n = stream.read(&mut chunk).unwrap_or(0);
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&chunk[..n]);
            }
            let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
            let (request_line, headers) = head.split_once("\r\n").unwrap_or((&head, ""));
            log.lock().unwrap().push(Seen {
                request_line: request_line.to_string(),
                headers: headers.to_string(),
                body: String::from_utf8_lossy(&buf[head_end..]).into_owned(),
            });
            let reply = format!(
                "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(reply.as_bytes());
        }
    });
    (url, seen)
}

fn load_live(src: &str) -> Result<Interp, telvax_runtime::RuntimeError> {
    let (ast, perrs) = telvax_parser::parse(src);
    assert!(perrs.is_empty(), "{perrs:?}");
    let (hir, diags) = telvax_hir::lower(&ast.unwrap());
    assert!(
        diags.is_empty(),
        "{:?}",
        diags.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
    Interp::with_live(hir, true)
}

const EMBED: &str = r#"
import std.ai.embeddings.OllamaEmbedder

let e = OllamaEmbedder.new("nomic-embed-text")

fn dims(text: Text) -> u64:
    std.iter.count(std.tensor.to_vec(text |> e.embed()))

fn first(text: Text) -> f32:
    (text |> e.embed())[0]
"#;

#[test]
fn ollama_embeddings_over_http() {
    let _g = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let vector: Vec<f32> = (0..768).map(|i| i as f32 / 1000.0).collect();
    let reply = serde_json::json!({ "embeddings": [vector] }).to_string();
    let (url, seen) = mock(vec![(200, reply.clone()), (200, reply)]);
    unsafe { std::env::set_var("OLLAMA_HOST", &url) };
    let i = load_live(EMBED).unwrap();
    assert!(matches!(
        i.call("dims", vec![Value::str("hello world")]).unwrap(),
        Value::Int(768)
    ));
    let first = i.call("first", vec![Value::str("x")]);
    assert!(first.is_ok(), "{first:?}");

    let seen = seen.lock().unwrap();
    assert!(
        seen[0].request_line.starts_with("POST /api/embed"),
        "{}",
        seen[0].request_line
    );
    let body: serde_json::Value = serde_json::from_str(&seen[0].body).unwrap();
    assert_eq!(body["model"], "nomic-embed-text");
    assert_eq!(body["input"], "hello world");
}

#[test]
fn wrong_embedding_size_is_an_error() {
    let _g = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let (url, _) = mock(vec![(200, r#"{"embeddings":[[0.1,0.2,0.3]]}"#.into())]);
    unsafe { std::env::set_var("OLLAMA_HOST", &url) };
    let i = load_live(EMBED).unwrap();
    let e = i.call("dims", vec![Value::str("x")]).unwrap_err();
    assert!(
        e.message
            .contains("returned 3 dimensions but the type requires 768"),
        "{e}"
    );
}

#[test]
fn unreachable_services_report_a_clear_error() {
    let _g = ENV.lock().unwrap_or_else(|e| e.into_inner());
    unsafe { std::env::set_var("OLLAMA_HOST", "http://127.0.0.1:1") };
    let i = load_live(EMBED).unwrap();
    let e = i.call("dims", vec![Value::str("x")]).unwrap_err();
    assert!(
        e.message
            .contains("request to http://127.0.0.1:1/api/embed failed"),
        "{e}"
    );
}

const LLM: &str = r#"
import std.ai.llm.OpenAIClient

let llm = OpenAIClient.new("gpt-4o-mini")

fn ask(q: Text) -> Text:
    q |> llm.complete()
"#;

#[test]
fn openai_chat_completions_over_http() {
    let _g = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let (url, seen) = mock(vec![(
        200,
        r#"{"choices":[{"message":{"role":"assistant","content":"forty-two"}}]}"#.into(),
    )]);
    unsafe {
        std::env::set_var("OPENAI_BASE_URL", &url);
        std::env::set_var("OPENAI_API_KEY", "test-key");
    }
    let i = load_live(LLM).unwrap();
    assert_eq!(
        i.call("ask", vec![Value::str("meaning of life?")])
            .unwrap()
            .to_string(),
        "forty-two"
    );

    let seen = seen.lock().unwrap();
    assert!(
        seen[0].request_line.starts_with("POST /chat/completions"),
        "{}",
        seen[0].request_line
    );
    assert!(
        seen[0]
            .headers
            .to_lowercase()
            .contains("authorization: bearer test-key"),
        "{}",
        seen[0].headers
    );
    let body: serde_json::Value = serde_json::from_str(&seen[0].body).unwrap();
    assert_eq!(body["model"], "gpt-4o-mini");
    assert_eq!(body["messages"][0]["role"], "user");
    assert_eq!(body["messages"][0]["content"], "meaning of life?");
}

#[test]
fn openai_errors_and_missing_key() {
    let _g = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let (url, _) = mock(vec![(
        401,
        r#"{"error":{"message":"Incorrect API key"}}"#.into(),
    )]);
    unsafe {
        std::env::set_var("OPENAI_BASE_URL", &url);
        std::env::set_var("OPENAI_API_KEY", "bad");
    }
    let i = load_live(LLM).unwrap();
    let e = i.call("ask", vec![Value::str("hi")]).unwrap_err();
    assert!(
        e.message.contains("HTTP 401") && e.message.contains("Incorrect API key"),
        "{e}"
    );

    unsafe { std::env::remove_var("OPENAI_API_KEY") };
    let e = load_live(LLM).unwrap_err();
    assert!(e.message.contains("OPENAI_API_KEY"), "{e}");
}

#[test]
fn openai_failures_say_what_happened() {
    let _g = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let (url, _) = mock(vec![
        (429, r#"{"error":{"message":"Rate limit reached"}}"#.into()),
        (502, "<html>bad gateway</html>".into()),
        (200, r#"{"choices":[]}"#.into()),
        (200, r#"{"choices":[{"message":{"content":null}}]}"#.into()),
    ]);
    unsafe {
        std::env::set_var("OPENAI_BASE_URL", &url);
        std::env::set_var("OPENAI_API_KEY", "k");
    }
    let i = load_live(LLM).unwrap();
    let ask = || i.call("ask", vec![Value::str("hi")]).unwrap_err().message;
    let e = ask();
    assert!(
        e.contains("HTTP 429") && e.contains("Rate limit reached"),
        "{e}"
    );
    let e = ask();
    assert!(e.contains("HTTP 502") && e.contains("unreadable"), "{e}");
    let e = ask();
    assert!(e.contains("choices[0].message.content"), "{e}");
    let e = ask();
    assert!(e.contains("choices[0].message.content"), "{e}");
}

#[test]
fn offline_mode_never_touches_the_network() {
    let _g = ENV.lock().unwrap_or_else(|e| e.into_inner());
    unsafe { std::env::set_var("OLLAMA_HOST", "http://127.0.0.1:1") };
    let (ast, _) = telvax_parser::parse(EMBED);
    let (hir, _) = telvax_hir::lower(&ast.unwrap());
    let i = Interp::new(hir).unwrap();
    assert!(matches!(
        i.call("dims", vec![Value::str("x")]).unwrap(),
        Value::Int(768)
    ));
}

/// Needs a PostgreSQL server with pgvector: set TELVAX_TEST_PG_URL to run it.
#[test]
fn pgvector_round_trip() {
    let Ok(url) = std::env::var("TELVAX_TEST_PG_URL") else {
        eprintln!("skipped: TELVAX_TEST_PG_URL is not set");
        return;
    };
    let src = format!(
        r#"
import std.ai.embeddings.OllamaEmbedder
import std.db.pgvector.PgVectorClient

let db = PgVectorClient.connect("{url}")

fn put(id: Text, text: Text, v: Tensor[f32, 768]):
    db.insert("telvax_test_docs", id, text, v)

fn nearest(v: Tensor[f32, 768], k: i32) -> List[Text]:
    db.search_collection("telvax_test_docs")
        |> db.with_vector(v)
        |> db.limit(k)
        |> db.execute()
        |> std.iter.map(d => d.text)
"#
    );
    let i = load_live(&src).unwrap();
    let vec_of = |axis: usize| {
        let mut v = vec![0.0f32; 768];
        v[axis] = 1.0;
        Value::Tensor(Arc::new(telvax_runtime::TensorVal::from_f32(vec![768], v)))
    };
    for (id, text, axis) in [("a", "alpha", 0), ("b", "beta", 1), ("c", "gamma", 2)] {
        i.call("put", vec![Value::str(id), Value::str(text), vec_of(axis)])
            .unwrap();
    }
    // Upserting the same id replaces it.
    i.call(
        "put",
        vec![Value::str("a"), Value::str("alpha v2"), vec_of(0)],
    )
    .unwrap();
    let Value::Vec(hits) = i.call("nearest", vec![vec_of(0), Value::Int(2)]).unwrap() else {
        panic!()
    };
    let hits: Vec<String> = hits.snapshot().iter().map(|v| v.to_string()).collect();
    assert_eq!(hits[0], "alpha v2");
    assert_eq!(hits.len(), 2);
    let e = i.call("put", vec![Value::str("x"), Value::str("y"), vec_of(3)]);
    assert!(e.is_ok());
}

/// Needs a PostgreSQL server: set TELVAX_TEST_PG_URL to run it.
#[test]
fn postgres_sql_round_trip() {
    let Ok(url) = std::env::var("TELVAX_TEST_PG_URL") else {
        eprintln!("skipped: TELVAX_TEST_PG_URL is not set");
        return;
    };
    let src = format!(
        r#"
import std.db.postgres.PostgresClient

type Job:
    id: Text
    status: Text
    attempts: Int
    score: Float
    done: Bool
    note: Text?
    payload: Text

let db = PostgresClient.connect("{url}")

fn setup() -> Result[Int, Text]:
    let _ = db.execute("DROP TABLE IF EXISTS telvax_test_jobs", [])?
    db.execute("CREATE TABLE telvax_test_jobs (id text PRIMARY KEY, status text NOT NULL, attempts int4 NOT NULL, score float8 NOT NULL, done bool NOT NULL, note text, payload jsonb NOT NULL, created timestamptz NOT NULL DEFAULT now())", [])

fn put(id: Text, status: Text, attempts: Int, note: Text?) -> Result[Int, Text]:
    db.execute("INSERT INTO telvax_test_jobs (id, status, attempts, score, done, note, payload) VALUES ($1, $2, $3, $4, $5, $6, $7)", (id, status, attempts, 1.5, False, note, "{{\"k\": [1, 2]}}"))

fn finish(id: Text) -> Result[Int, Text]:
    db.execute("UPDATE telvax_test_jobs SET status = $1, done = $2 WHERE id = $3", ("done", True, id))

fn by_status(status: Text) -> Result[List[Job], Text]:
    let rows: List[Job] = db.query("SELECT id, status, attempts, score, done, note, payload FROM telvax_test_jobs WHERE status = $1 ORDER BY id", [status])?
    Ok(rows)

fn count_all() -> Result[u64, Text]:
    let rows: List[Job] = db.query("SELECT id, status, attempts, score, done, note, payload FROM telvax_test_jobs", [])?
    Ok(rows.len())

fn bad_sql() -> Result[Int, Text]:
    db.execute("INSERT INTO telvax_test_jobs (id) VALUES ($1)", ["x"])

fn timestamp_column() -> Result[List[Job], Text]:
    let rows: List[Job] = db.query("SELECT id, status, attempts, score, done, note, payload, created FROM telvax_test_jobs", [])?
    Ok(rows)

fn wrong_type() -> Result[Int, Text]:
    db.execute("SELECT $1::int4", ["not a number"])
"#
    );
    let i = load_live(&src).unwrap();
    let call = |f: &str, args: Vec<Value>| i.call(f, args).unwrap().to_string();
    let s = Value::str;
    assert_eq!(call("setup", vec![]), "Ok(0)");
    assert_eq!(
        call(
            "put",
            vec![s("a"), s("queued"), Value::Int(1), Value::none()]
        ),
        "Ok(1)"
    );
    assert_eq!(
        call(
            "put",
            vec![s("b"), s("queued"), Value::Int(2), Value::some(s("hello"))]
        ),
        "Ok(1)"
    );
    // The same id twice is the server's error, as text.
    let dup = call(
        "put",
        vec![s("a"), s("queued"), Value::Int(1), Value::none()],
    );
    assert!(
        dup.starts_with("Err(") && dup.contains("duplicate key"),
        "{dup}"
    );

    let queued = call("by_status", vec![s("queued")]);
    assert!(
        queued.contains("attempts: 2") || queued.contains("2"),
        "{queued}"
    );
    assert!(
        queued.contains("hello") && queued.contains("None"),
        "{queued}"
    );
    assert!(queued.contains("{\\\"k\\\":[1,2]}"), "{queued}");

    assert_eq!(call("finish", vec![s("a")]), "Ok(1)");
    assert_eq!(call("finish", vec![s("missing")]), "Ok(0)");
    let done = call("by_status", vec![s("done")]);
    assert!(done.contains("true") || done.contains("True"), "{done}");
    assert_eq!(call("count_all", vec![]), "Ok(2)");

    // Parameters are bound, never pasted into the SQL.
    let injected = call("by_status", vec![s("x' OR '1'='1")]);
    assert_eq!(injected, "Ok([])");

    let bad = call("bad_sql", vec![]);
    assert!(
        bad.starts_with("Err(") && bad.contains("null value"),
        "{bad}"
    );
    let ts = call("timestamp_column", vec![]);
    assert!(ts.contains("created::text"), "{ts}");
    let wrong = call("wrong_type", vec![]);
    assert!(wrong.contains("cannot use a Text"), "{wrong}");
}
