use serde_json::json;
use std::sync::Arc;
use telvax_runtime::{Interp, Value, embed_text};

fn load(src: &str) -> Interp {
    let (ast, perrs) = telvax_parser::parse(src);
    assert!(perrs.is_empty(), "parse: {perrs:?}");
    let (hir, diags) = telvax_hir::lower(&ast.unwrap());
    assert!(
        diags.is_empty(),
        "check: {:?}",
        diags.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
    Interp::new(hir).expect("globals should initialise")
}

fn run(src: &str, f: &str, args: Vec<Value>) -> Value {
    load(src)
        .call(f, args)
        .unwrap_or_else(|e| panic!("runtime error: {e}"))
}

fn int(v: Value) -> i64 {
    match v {
        Value::Int(i) => i,
        other => panic!("expected int, got {other:?}"),
    }
}

fn float(v: Value) -> f64 {
    match v {
        Value::Float(x) => x,
        other => panic!("expected float, got {other:?}"),
    }
}

fn text(v: Value) -> String {
    match v {
        Value::Str(s) => s.to_string(),
        other => panic!("expected string, got {other:?}"),
    }
}

#[test]
fn arithmetic_and_recursion() {
    let src = r#"
fn fib(n: i32) -> i32:
    if n < 2:
        return n
    fib(n - 1) + fib(n - 2)
"#;
    assert_eq!(int(run(src, "fib", vec![Value::Int(15)])), 610);
    assert_eq!(
        int(run(
            r#"
fn f() -> i32:
    2 + 3 * 4 - 10 / 5 % 3
"#,
            "f",
            vec![]
        )),
        12
    );
    assert_eq!(
        int(run(
            r#"
fn f() -> i32:
    -3
"#,
            "f",
            vec![]
        )),
        -3
    );
}

#[test]
fn floats_and_literal_adaptation() {
    let v = run(
        r#"
fn f(a: f32) -> f32:
    a * 2 + 0.5
"#,
        "f",
        vec![Value::Float(1.25)],
    );
    assert!(
        matches!(v, Value::Float(x) if (x - 3.0).abs() < 1e-9),
        "{v:?}"
    );
}

#[test]
fn let_blocks_and_early_return() {
    let src = r#"
fn f(x: i32) -> i32:
    let y = x + 1
    if y > 5:
        return 100
    let w = y * 2
    w + 1
"#;
    assert_eq!(int(run(src, "f", vec![Value::Int(1)])), 5);
    assert_eq!(int(run(src, "f", vec![Value::Int(9)])), 100);
}

#[test]
fn pipes_and_strings() {
    let src = r#"
fn shout(s: Text) -> Text:
    s + "!"

fn f(s: Text) -> Text:
    s |> std.text.clean_whitespace() |> std.text.truncate(11) |> shout() |> std.text.uppercase()
"#;
    assert_eq!(
        text(run(src, "f", vec![Value::str("  hello    big   world  ")])),
        "HELLO BIG W!"
    );
}

#[test]
fn fstrings() {
    let src = r#"
fn f(n: i32, s: Text) -> Text:
    f"{s}: {n + 1} \"ok\""
"#;
    assert_eq!(
        text(run(src, "f", vec![Value::Int(1), Value::str("n")])),
        "n: 2 \"ok\""
    );
}

#[test]
fn iterators_and_closures() {
    let src = r#"
fn f(v: List[i32], k: i32) -> Text:
    v |> std.iter.filter(x => x > 1) |> std.iter.map(x => f"{x * k}") |> std.string.join("-")
"#;
    let v = Value::vec(vec![Value::Int(1), Value::Int(2), Value::Int(3)]);
    assert_eq!(text(run(src, "f", vec![v, Value::Int(10)])), "20-30");
}

#[test]
fn structs_and_fields() {
    let src = r#"
type P:
    x: i32
    name: Text

fn f() -> Text:
    let p = P(name: "a", x: 4)
    f"{p.name}{p.x}"
"#;
    assert_eq!(text(run(src, "f", vec![])), "a4");
}

#[test]
fn short_circuit_avoids_errors() {
    let src = r#"
fn f(d: i32) -> Bool:
    d != 0 and 10 / d > 1
"#;
    assert!(matches!(
        run(src, "f", vec![Value::Int(0)]),
        Value::Bool(false)
    ));
    assert!(matches!(
        run(src, "f", vec![Value::Int(2)]),
        Value::Bool(true)
    ));
}

#[test]
fn runtime_errors_are_reported_not_panics() {
    let div = load(
        r#"
fn f(d: i32) -> i32:
    1 / d
"#,
    );
    assert!(
        div.call("f", vec![Value::Int(0)])
            .unwrap_err()
            .message
            .contains("division by zero")
    );
    let ovf = load(
        r#"
fn f(a: Int) -> Int:
    a * a
"#,
    );
    assert!(
        ovf.call("f", vec![Value::Int(i64::MAX)])
            .unwrap_err()
            .message
            .contains("overflow")
    );
    let rec = load(
        r#"
fn f(n: i32) -> i32:
    f(n + 1)
"#,
    );
    assert!(
        rec.call("f", vec![Value::Int(0)])
            .unwrap_err()
            .message
            .contains("stack overflow")
    );
    assert!(div.call("nope", vec![]).is_err());
}

#[test]
fn globals_are_evaluated_once_in_order() {
    let src = r#"
let a = 2
let b = a * 21

fn f() -> i32:
    b
"#;
    assert_eq!(int(run(src, "f", vec![])), 42);
}

#[test]
fn endpoint_results_and_errors() {
    let src = r#"
import std.http.Status

type In:
    n: i32

type Out:
    double: i32

@endpoint(POST, "/d")
fn d(p: In) -> Result[Out, Status]:
    if p.n < 0:
        return Err(Status.BadRequest("negative"))
    Ok(Out(double: p.n * 2))
"#;
    let i = load(src);
    assert_eq!(i.endpoint_names(), ["d"]);
    assert_eq!(
        i.call_endpoint("d", &json!({"n": 4})).unwrap(),
        json!({"double": 8})
    );
    let e = i.call_endpoint("d", &json!({"n": -1})).unwrap_err();
    assert_eq!((e.status, e.message.as_str()), (400, "negative"));
    assert_eq!(
        i.call_endpoint("d", &json!({"n": "x"})).unwrap_err().status,
        400
    );
    assert_eq!(i.call_endpoint("nope", &json!({})).unwrap_err().status, 500);
}

#[test]
fn a_runtime_error_in_an_endpoint_is_for_the_log_not_for_the_client() {
    let src = r#"
import std.http.Status

@endpoint(GET, "/boom")
fn boom() -> Result[Int, Status]:
    let xs: List[Int] = []
    Ok(xs[3])

@endpoint(GET, "/chosen")
fn chosen() -> Result[Int, Status]:
    Err(Status.Internal("the cache is down"))
"#;
    let i = load(src);
    let none = Default::default();
    let req = telvax_runtime::RequestInfo::default();
    let e = i
        .call_endpoint_reply("boom", &none, &json!(null), &req)
        .unwrap_err();
    assert_eq!(
        (e.status, e.message.as_str()),
        (500, "Internal server error")
    );
    assert!(
        e.log.unwrap().contains("index out of bounds"),
        "the reason goes to the log"
    );
    let e = i
        .call_endpoint_reply("chosen", &none, &json!(null), &req)
        .unwrap_err();
    assert_eq!((e.status, e.message.as_str()), (500, "the cache is down"));
    assert!(e.log.is_none());
}

#[test]
fn endpoint_path_parameters_reach_their_arguments() {
    let src = r#"
import std.http.Status

type Note:
    text: Text

type Out:
    line: Text

@endpoint(POST, "/jobs/{id}/notes/{n}")
fn add(id: Text, n: i32, p: Note) -> Result[Out, Status]:
    if n < 0:
        return Err(Status.BadRequest("negative"))
    Ok(Out(line: id + ":" + std.text.to_text(n) + ":" + p.text))
"#;
    let i = load(src);
    let params = std::collections::BTreeMap::from([
        ("id".to_string(), json!("j7")),
        ("n".to_string(), json!(3)),
    ]);
    assert_eq!(
        i.call_endpoint_with("add", &params, &json!({"text": "hi"}))
            .unwrap(),
        json!({"line": "j7:3:hi"})
    );
    let mut bad = params.clone();
    bad.insert("n".to_string(), json!(-1));
    let e = i
        .call_endpoint_with("add", &bad, &json!({"text": "hi"}))
        .unwrap_err();
    assert_eq!((e.status, e.message.as_str()), (400, "negative"));
    // A missing path value is a server error, not a bad request.
    let e = i
        .call_endpoint_with("add", &Default::default(), &json!({"text": "hi"}))
        .unwrap_err();
    assert_eq!(e.status, 500);
}

#[test]
fn an_endpoint_can_read_the_request() {
    let src = r#"
import std.http.Request
import std.http.Status

type Out:
    who: Text

@endpoint(DELETE, "/v1/cache")
fn clear(req: Request) -> Result[Out, Status]:
    match req.header("X-Admin-Token"):
        case Some(t):
            Ok(Out(who: req.method() + " " + req.path() + " " + t))
        case None:
            Err(Status.Unauthorized("invalid token"))
"#;
    let i = load(src);
    let none = std::collections::BTreeMap::new();
    let req = telvax_runtime::RequestInfo {
        method: "DELETE".into(),
        path: "/v1/cache".into(),
        headers: vec![("x-admin-token".into(), "s3".into())],
    };
    assert_eq!(
        i.call_endpoint_as("clear", &none, &json!(null), &req)
            .unwrap(),
        json!({"who": "DELETE /v1/cache s3"})
    );
    let e = i
        .call_endpoint_as(
            "clear",
            &none,
            &json!(null),
            &telvax_runtime::RequestInfo::default(),
        )
        .unwrap_err();
    assert_eq!((e.status, e.message.as_str()), (401, "invalid token"));
}

#[test]
fn an_endpoint_can_choose_the_status_and_headers() {
    let src = r#"
import std.http.Request
import std.http.Response
import std.http.Status

type Accepted:
    job_id: Text

type Code:
    n: i32

@endpoint(POST, "/jobs")
fn submit(c: Code) -> Result[Response, Status]:
    Ok(Response.new(Accepted(job_id: "1"))
        .status(c.n)
        .header("Location", "/v1/results/1")
        .header("location", "/v2/results/1"))

@endpoint(GET, "/plain")
fn plain() -> Result[Accepted, Status]:
    Ok(Accepted(job_id: "2"))

@endpoint(GET, "/bare")
fn bare() -> Result[Response, Status]:
    Ok(Response.new(Accepted(job_id: "3")))
"#;
    let i = load(src);
    let none = std::collections::BTreeMap::new();
    let req = telvax_runtime::RequestInfo::default();
    let r = i
        .call_endpoint_reply("submit", &none, &json!({"n": 202}), &req)
        .unwrap();
    assert_eq!(r.status, 202);
    assert_eq!(r.headers, [("location".into(), "/v2/results/1".into())]);
    assert_eq!(r.body, json!({"job_id": "1"}));

    let r = i
        .call_endpoint_reply("plain", &none, &json!(null), &req)
        .unwrap();
    assert_eq!((r.status, r.headers.len()), (200, 0));
    let r = i
        .call_endpoint_reply("bare", &none, &json!(null), &req)
        .unwrap();
    assert_eq!((r.status, r.body), (200, json!({"job_id": "3"})));

    // The old entry points keep giving only the body.
    assert_eq!(
        i.call_endpoint("submit", &json!({"n": 201})).unwrap(),
        json!({"job_id": "1"})
    );
    for bad in [99, 600, -1] {
        let e = i.call_endpoint("submit", &json!({ "n": bad })).unwrap_err();
        assert_eq!(e.status, 500, "{bad}");
        assert!(e.message.contains("from 100 to 599"), "{}", e.message);
    }
}

#[test]
fn a_top_level_list_and_map_are_shared_by_concurrent_requests() {
    let src = r#"
let hits: List[Text] = []
let last_seen: Map[Text, f64] = {}

type Who:
    name: Text

type Count:
    n: u64

@endpoint(POST, "/hit")
fn hit(w: Who) -> Result[Count, Text]:
    hits.push(w.name)
    last_seen.insert(w.name, std.time.now())
    Ok(Count(n: last_seen.len()))

@endpoint(GET, "/total")
fn total() -> Result[Count, Text]:
    Ok(Count(n: hits.len()))
"#;
    let i = Arc::new(load(src));
    let threads: Vec<_> = (0..8)
        .map(|t| {
            let i = i.clone();
            std::thread::spawn(move || {
                for k in 0..50 {
                    let name = format!("u{}", (t * 50 + k) % 100);
                    i.call_endpoint("hit", &json!({ "name": name })).unwrap();
                }
            })
        })
        .collect();
    for t in threads {
        t.join().unwrap();
    }
    // Every push and every insert landed: 400 calls over 100 distinct names.
    let n = i.call_endpoint("hit", &json!({"name": "u0"})).unwrap();
    assert_eq!(n, json!({"n": 100}));
    let total = i.call_endpoint("total", &json!(null)).unwrap();
    assert_eq!(total, json!({"n": 401}));
}

#[test]
fn text_is_normalized_and_cleaned() {
    let src = r##"
fn nfd_len(s: Text) -> u64:
    std.text.normalize(s, "NFD").chars().len()

fn nfc_len(s: Text) -> u64:
    std.text.normalize(s, "NFC").chars().len()

fn compat(s: Text) -> Text:
    std.text.normalize(s, "NFKC")

fn plain(s: Text) -> Text:
    std.text.strip_accents(s)

fn punctuation_free(s: Text) -> Text:
    std.text.remove_chars(s, "!\"#$%&'()*+,-./:;<=>?@[]^_`{|}~")

fn bad_form(s: Text) -> Text:
    std.text.normalize(s, "NFX")
"##;
    let i = load(src);
    let call = |f: &str, s: &str| i.call(f, vec![Value::str(s)]).unwrap();
    assert_eq!(call("nfd_len", "ação").to_string(), "6");
    assert_eq!(call("nfc_len", "a\u{0303}").to_string(), "1");
    assert_eq!(call("compat", "ﬁ ①").to_string(), "fi 1");
    assert_eq!(call("plain", "ação, é à").to_string(), "acao, e a");
    assert_eq!(
        call("punctuation_free", "Good, really-good! (yes)").to_string(),
        "Good reallygood yes"
    );
    let e = i.call("bad_form", vec![Value::str("x")]).unwrap_err();
    assert!(
        e.to_string().contains("unknown normalization form `NFX`"),
        "{e}"
    );
}

#[test]
fn tensors_round_trip_through_json() {
    let src = r#"
type T:
    v: Tensor[f32, 2, 2]

@endpoint(POST, "/t")
fn t(p: T) -> Result[T, Text]:
    Ok(p)
"#;
    let i = load(src);
    let body = json!({"v": [[1.0, 2.0], [3.0, 4.0]]});
    assert_eq!(i.call_endpoint("t", &body).unwrap(), body);
    assert_eq!(
        i.call_endpoint("t", &json!({"v": [[1.0], [3.0, 4.0]]}))
            .unwrap_err()
            .status,
        400
    );
}

#[test]
fn embeddings_are_deterministic_and_similarity_aware() {
    let a = embed_text("tensors have a static shape");
    let b = embed_text("a tensor has static shapes and tensors");
    let c = embed_text("completely unrelated banana bread");
    assert_eq!(a.len(), 768);
    assert_eq!(a, embed_text("tensors have a static shape"));
    let dot = |x: &[f32], y: &[f32]| x.iter().zip(y).map(|(p, q)| p * q).sum::<f32>();
    assert!((dot(&a, &a) - 1.0).abs() < 1e-4);
    assert!(dot(&a, &b) > dot(&a, &c));
}

#[test]
fn rag_example_end_to_end() {
    let src = include_str!("../../examples/rag-service.tvx");
    let i = load(src);
    let db = i.db_for("postgresql://localhost:5432/vectors").unwrap();
    for (id, text) in [
        (
            "t1",
            "Tensors in Telvax carry a static shape checked at compile time",
        ),
        (
            "p1",
            "The pipeline operator composes functions left to right",
        ),
        ("b1", "Banana bread needs ripe bananas"),
    ] {
        db.insert("knowledge_base", id, text, embed_text(text))
            .unwrap();
    }

    let out = i
        .call_endpoint(
            "handle_rag",
            &json!({"query": "how are tensor shapes checked", "top_k": 2}),
        )
        .unwrap();
    let ctx = out["retrieved_context"].as_array().unwrap();
    assert_eq!(ctx.len(), 2);
    assert!(
        ctx[0].as_str().unwrap().starts_with("Tensors in Telvax"),
        "{ctx:?}"
    );
    assert_eq!(out["query"], "how are tensor shapes checked");
    assert!(
        out["answer"]
            .as_str()
            .unwrap()
            .contains("how are tensor shapes checked"),
        "{out}"
    );

    let e = i
        .call_endpoint("handle_rag", &json!({"query": "", "top_k": 1}))
        .unwrap_err();
    assert_eq!(
        (e.status, e.message.as_str()),
        (400, "Query string cannot be empty.")
    );
}

#[test]
fn print_goes_to_the_configured_output() {
    use std::sync::{Arc, Mutex};
    struct Sink(Arc<Mutex<Vec<u8>>>);
    impl std::io::Write for Sink {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let buf = Arc::new(Mutex::new(Vec::new()));
    let i = load(
        r#"
fn main() -> i32:
    std.io.print("a")
    print("b")
    0
"#,
    );
    i.set_output(Sink(buf.clone()));
    i.call("main", vec![]).unwrap();
    assert_eq!(
        String::from_utf8(buf.lock().unwrap().clone()).unwrap(),
        "ab\n"
    );
}

#[test]
fn mutation_and_while() {
    let src = r#"
fn sum(n: i32) -> i32:
    var i = 0
    var s = 0
    while i < n:
        s += i
        i += 1
    s
"#;
    assert_eq!(int(run(src, "sum", vec![Value::Int(10)])), 45);
    assert_eq!(int(run(src, "sum", vec![Value::Int(0)])), 0);
    let early = r#"
fn f() -> i32:
    var i = 0
    while True:
        if i == 7:
            return i * 2
        i += 1
    0
"#;
    assert_eq!(int(run(early, "f", vec![])), 14);
    let strs = r#"
fn f() -> Text:
    var s = ""
    var i = 0
    while i < 3:
        s = s + f"{i},"
        i += 1
    s
"#;
    assert_eq!(text(run(strs, "f", vec![])), "0,1,2,");
}

#[test]
fn for_loops_over_ranges_and_vectors() {
    let src = r#"
fn tri(n: i32) -> i32:
    var s = 0

    for i in range(n):
        s += i

    s

fn incl(n: i32) -> i32:
    var s = 0
    for i in range(1, n + 1):
        s += i
    s

fn dot(a: List[i32], b: List[i32]) -> i32:
    var s = 0
    for i in range(std.iter.count(a)):
        s += a[i] * b[i]
    s

fn total(v: List[Float]) -> Float:
    var s = 0.0
    for x in v:
        s += x
    s

fn find(v: List[i32], t: i32) -> i32:
    var idx = 0
    for x in v:
        if x == t:
            return idx
        idx += 1
    0 - 1

fn nested(n: i32) -> i32:
    var c = 0
    for i in range(n):
        for j in range(i):
            c += j
    c

fn empty(n: i32) -> i32:
    var c = 0
    for i in range(5, n):
        c += 1
    c
"#;
    let ints = |v: &[i64]| Value::vec(v.iter().map(|&i| Value::Int(i)).collect());
    assert_eq!(int(run(src, "tri", vec![Value::Int(5)])), 10);
    assert_eq!(int(run(src, "incl", vec![Value::Int(5)])), 15);
    assert_eq!(
        int(run(src, "dot", vec![ints(&[1, 2, 3]), ints(&[4, 5, 6])])),
        32
    );
    assert_eq!(
        int(run(src, "find", vec![ints(&[5, 6, 7]), Value::Int(7)])),
        2
    );
    assert_eq!(
        int(run(src, "find", vec![ints(&[5, 6, 7]), Value::Int(9)])),
        -1
    );
    assert_eq!(int(run(src, "nested", vec![Value::Int(4)])), 4);
    assert_eq!(int(run(src, "empty", vec![Value::Int(3)])), 0);
    let floats = Value::vec(vec![Value::Float(1.5), Value::Float(2.0)]);
    assert!(matches!(run(src, "total", vec![floats]), Value::Float(x) if x == 3.5));
}

#[test]
fn arrays_indexing_and_bounds_errors() {
    let src = r#"
fn f(i: i32) -> i32:
    let v = [10, 20, 30]
    v[i]

fn g() -> i32:
    [[1, 2], [3, 4]][1][0]
"#;
    assert_eq!(int(run(src, "f", vec![Value::Int(2)])), 30);
    assert_eq!(int(run(src, "g", vec![])), 3);
    let i = load(src);
    let e = i.call("f", vec![Value::Int(3)]).unwrap_err();
    assert!(
        e.message
            .contains("index out of bounds: the length is 3 but the index is 3"),
        "{e}"
    );
    assert!(
        i.call("f", vec![Value::Int(-4)])
            .unwrap_err()
            .message
            .contains("out of bounds")
    );
    // A negative index counts from the end.
    assert_eq!(int(i.call("f", vec![Value::Int(-1)]).unwrap()), 30);
}

#[test]
fn a_range_ending_at_the_integer_limit_does_not_overflow() {
    let src = r#"
fn f() -> Int:
    let hi: Int = 9223372036854775807
    var c: Int = 0
    for i in range(hi - 2, hi):
        c += 1
    c
"#;
    assert_eq!(int(run(src, "f", vec![])), 2);
}

#[test]
fn assignment_to_vector_elements_and_struct_fields() {
    let src = r#"
type P:
    x: i32
    name: Text
    tags: List[i32]

type W:
    p: P
    n: i32

fn elems() -> i32:
    var v = [1, 2, 3]
    v[0] = 10
    v[2] += v[0]
    v[1] *= 5
    v[0] + v[1] + v[2]

fn fields() -> Text:
    var p = P(x: 1, name: "a", tags: [7, 8])
    p.x = 5
    p.x += 1
    p.name = p.name + "!"
    p.tags[1] = p.x
    f"{p.name}{p.x}{p.tags[0]}{p.tags[1]}"

fn nested() -> i32:
    var w = W(p: P(x: 1, name: "n", tags: [0, 0]), n: 2)
    w.p.x = 40
    w.p.tags[0] = 2
    w.n = 7
    w.p.x + w.p.tags[0] + w.n

fn grid() -> i32:
    var g = [[1, 2], [3, 4]]
    g[1][0] = 9
    g[0][1] += g[1][0]
    g[1][0] * 100 + g[0][1]

fn fill(v: List[i32], x: i32):
    v[0] = x
    v[1] = x * 2

fn via_param() -> i32:
    let v = [0, 0]
    fill(v, 4)
    v[0] + v[1]

fn value_semantics() -> i32:
    var a = P(x: 1, name: "a", tags: [])
    let b = a
    a.x = 99
    b.x

fn reference_semantics() -> i32:
    var a = [1, 2]
    let b = a
    a[0] = 50
    b[0]

fn loop_writes() -> i32:
    var v = [0, 0, 0, 0]
    for i in range(4):
        v[i] = i * i
    var s = 0
    for x in v:
        s += x
    s

fn mut_param_scalar(n: i32) -> i32:
    n += 1
    n * 2
"#;
    assert_eq!(int(run(src, "elems", vec![])), 10 + 10 + 13);
    assert_eq!(
        text(run(src, "fields", vec![])),
        "a!6".to_string() + "7" + "6"
    );
    assert_eq!(int(run(src, "nested", vec![])), 49);
    assert_eq!(int(run(src, "grid", vec![])), 900 + 11);
    assert_eq!(
        int(run(src, "via_param", vec![])),
        12,
        "Vec is passed by reference"
    );
    assert_eq!(
        int(run(src, "value_semantics", vec![])),
        1,
        "structs are copied"
    );
    assert_eq!(
        int(run(src, "reference_semantics", vec![])),
        50,
        "Vec copies share elements"
    );
    assert_eq!(int(run(src, "loop_writes", vec![])), 14);
    assert_eq!(int(run(src, "mut_param_scalar", vec![Value::Int(3)])), 8);
}

#[test]
fn assignment_bounds_errors() {
    let i = load(
        r#"
fn f(k: i32):
    var v = [1, 2]
    v[k] = 5

fn g():
    var v = [[1], [2]]
    v[1][3] = 0
"#,
    );
    assert!(
        i.call("f", vec![Value::Int(2)])
            .unwrap_err()
            .message
            .contains("out of bounds: the length is 2 but the index is 2")
    );
    assert!(i.call("f", vec![Value::Int(-3)]).is_err());
    assert!(
        i.call("g", vec![])
            .unwrap_err()
            .message
            .contains("out of bounds")
    );
    assert!(i.call("f", vec![Value::Int(1)]).is_ok());
}

#[test]
fn a_step_limit_stops_runaway_programs() {
    let i = load(
        r#"
fn spin() -> i32:
    var n = 0
    while True:
        n += 1
    n

fn quick() -> i32:
    1 + 1
"#,
    );
    i.set_step_limit(10_000);
    let e = i.call("spin", vec![]).unwrap_err();
    assert!(e.message.contains("step limit"), "{e}");
    i.set_step_limit(10_000);
    assert_eq!(
        int(i.call("quick", vec![]).unwrap()),
        2,
        "a fresh budget runs normal code"
    );
}

#[test]
fn deep_recursion_works_on_a_small_thread_stack_and_runaway_recursion_is_an_error() {
    let src = r#"
fn sum(n: Int) -> Int:
    if n == 0:
        0
    else:
        n + sum(n - 1)

fn forever(n: Int) -> Int:
    forever(n + 1)
"#;
    let i = std::sync::Arc::new(load(src));
    let worker = i.clone();
    // A 256 KiB thread would overflow after a few hundred native frames without stack growth.
    let handle = std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(move || {
            let deep = worker.call("sum", vec![Value::Int(1500)]).unwrap();
            let runaway = worker.call("forever", vec![Value::Int(0)]).unwrap_err();
            (int(deep), runaway.message)
        })
        .unwrap();
    let (deep, runaway) = handle.join().expect("no native stack overflow");
    assert_eq!(deep, 1500 * 1501 / 2);
    assert!(runaway.contains("stack overflow"), "{runaway}");
}

#[test]
fn top_level_functions_are_values() {
    let src = r#"
fn double(x: Int) -> Int:
    x * 2

fn apply_all(v: List[Int]) -> List[Int]:
    std.iter.map(v, double)

fn total(v: List[Int]) -> Int:
    var s = 0
    for x in apply_all(v):
        s += x
    s
"#;
    let v = Value::vec((1..=4).map(Value::Int).collect());
    assert_eq!(int(run(src, "total", vec![v])), 20);
}

#[test]
fn dist_map_spreads_work_over_threads_and_keeps_order() {
    let src = r#"
fn square(x: Int) -> Int:
    x * x

fn all(v: List[Int]) -> List[Int]:
    std.dist.map(v, square)

fn fails(x: Int) -> Int:
    10 / x

fn bad(v: List[Int]) -> List[Int]:
    std.dist.map(v, fails)
"#;
    let i = load(src);
    let input = Value::vec((0..200).map(Value::Int).collect());
    let Value::Vec(out) = i.call("all", vec![input]).unwrap() else {
        panic!()
    };
    let squares: Vec<i64> = out.snapshot().iter().map(|v| int(v.clone())).collect();
    assert_eq!(squares, (0..200).map(|x| x * x).collect::<Vec<_>>());
    // An error in any call fails the whole map.
    let e = i
        .call("bad", vec![Value::vec(vec![Value::Int(5), Value::Int(0)])])
        .unwrap_err();
    assert!(e.message.contains("division by zero"), "{e}");
}

#[test]
fn dist_map_can_use_a_custom_executor() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct Counting(AtomicUsize);
    impl telvax_runtime::Executor for Counting {
        fn map(
            &self,
            function: &str,
            items: Vec<serde_json::Value>,
        ) -> Result<Vec<serde_json::Value>, String> {
            self.0.fetch_add(items.len(), Ordering::SeqCst);
            Ok(items
                .iter()
                .map(|j| serde_json::json!(j.as_i64().unwrap() + 1000 + function.len() as i64))
                .collect())
        }
    }
    let i = load(
        r#"
fn work(x: Int) -> Int:
    x

fn all(v: List[Int]) -> List[Int]:
    std.dist.map(v, work)
"#,
    );
    let exec = std::sync::Arc::new(Counting(AtomicUsize::new(0)));
    i.set_executor(exec.clone());
    let Value::Vec(out) = i
        .call("all", vec![Value::vec(vec![Value::Int(1), Value::Int(2)])])
        .unwrap()
    else {
        panic!()
    };
    let got: Vec<i64> = out.snapshot().into_iter().map(int).collect();
    assert_eq!(got, [1005, 1006], "the executor decided the results");
    assert_eq!(exec.0.load(Ordering::SeqCst), 2);
}

#[test]
fn call_json_converts_by_the_parameter_types() {
    let i = load(
        r#"
type P:
    x: Int
    y: Int

fn add(p: P, k: Int) -> Int:
    p.x + p.y + k

fn boom() -> Result[Int, Text]:
    Err("no")
"#,
    );
    assert_eq!(
        i.call_json("add", &[json!({"x": 1, "y": 2}), json!(10)])
            .unwrap(),
        json!(13)
    );
    assert!(
        i.call_json("add", &[json!({"x": 1}), json!(1)])
            .unwrap_err()
            .contains("argument `p`")
    );
    assert!(
        i.call_json("add", &[json!(1)])
            .unwrap_err()
            .contains("takes 2")
    );
    assert!(i.call_json("boom", &[]).unwrap_err().contains("Err(no)"));
}

#[test]
fn break_and_continue_in_every_kind_of_loop() {
    let src = r#"
fn range_break(n: Int) -> Int:
    var s = 0

    for i in range(n):
        if i == 5:
            break

        s += i

    s

fn range_continue(n: Int) -> Int:
    var s = 0
    for i in range(n):
        if i % 2 == 0:
            continue
        s += i
    s

fn while_loop() -> Int:
    var i = 0
    var s = 0
    while True:
        i += 1
        if i > 10:
            break
        if i % 3 == 0:
            continue
        s += i
    s

fn nested() -> Int:
    var c = 0
    for a in range(5):
        for b in range(5):
            if b > a:
                break
            c += 1
    c

fn return_inside(n: Int) -> Int:
    for i in range(n):
        if i == 3:
            return i * 100
    0 - 1
"#;
    assert_eq!(int(run(src, "range_break", vec![Value::Int(100)])), 10);
    assert_eq!(int(run(src, "range_continue", vec![Value::Int(10)])), 25);
    assert_eq!(int(run(src, "while_loop", vec![])), 37);
    assert_eq!(int(run(src, "nested", vec![])), 15);
    assert_eq!(int(run(src, "return_inside", vec![Value::Int(10)])), 300);
}

#[test]
fn enums_and_match() {
    let src = r#"
enum Shape:
    Circle(Float)
    Rect(Float, Float)
    Empty

fn area(s: Shape) -> Float:
    match s:
        case Shape.Circle(r):
            r * r * 3.0
        case Shape.Rect(w, h):
            w * h
        case Shape.Empty:
            0.0

fn total() -> Float:
    area(Shape.Circle(2.0)) + area(Shape.Rect(2.0, 3.5)) + area(Shape.Empty)

fn describe(n: Int) -> Text:
    match n:
        case 0:
            "zero"
        case 1:
            "one"
        case _:
            "many"

fn safe_div(a: Int, b: Int) -> Result[Int, Text]:
    if b == 0:
        return Err("divide by zero")
    Ok(a / b)

fn show(a: Int, b: Int) -> Text:
    match safe_div(a, b):
        case Ok(v):
            f"ok {v}"
        case Err(e):
            f"error: {e}"

fn same(a: Shape, b: Shape) -> Bool:
    a == b

fn pick(flag: Bool) -> Shape:
    if flag:
        Shape.Rect(1.0, 2.0)
    else:
        Shape.Empty

enum Op:
    Add(Int)
    Neg
    Twice(Shape)

fn nested(o: Op) -> Float:
    match o:
        case Op.Add(n):
            1.0
        case Op.Neg:
            2.0
        case Op.Twice(Shape.Circle(r)):
            r
        case Op.Twice(_):
            9.0
"#;
    assert_eq!(float(run(src, "total", vec![])), 12.0 + 7.0);
    assert_eq!(text(run(src, "describe", vec![Value::Int(1)])), "one");
    assert_eq!(text(run(src, "describe", vec![Value::Int(42)])), "many");
    assert_eq!(
        text(run(src, "show", vec![Value::Int(9), Value::Int(3)])),
        "ok 3"
    );
    assert_eq!(
        text(run(src, "show", vec![Value::Int(9), Value::Int(0)])),
        "error: divide by zero"
    );
    let interp = load(src);
    let a = interp.call("pick", vec![Value::Bool(true)]).unwrap();
    let b = interp.call("pick", vec![Value::Bool(true)]).unwrap();
    let c = interp.call("pick", vec![Value::Bool(false)]).unwrap();
    assert!(matches!(
        interp.call("same", vec![a.clone(), b]),
        Ok(Value::Bool(true))
    ));
    assert!(matches!(
        interp.call("same", vec![a.clone(), c]),
        Ok(Value::Bool(false))
    ));
    assert_eq!(a.to_json(), json!({ "Rect": [1.0, 2.0] }));
    assert_eq!(a.to_string(), "Shape.Rect(1.0, 2.0)");
    // JSON in, JSON out (this is how endpoints and workers talk).
    let area = interp
        .call_json("area", &[json!({ "Circle": 3.0 })])
        .unwrap();
    assert_eq!(area, json!(27.0));
    assert_eq!(
        interp.call_json("area", &[json!("Empty")]).unwrap(),
        json!(0.0)
    );
    assert!(interp.call_json("area", &[json!("Triangle")]).is_err());
    assert_eq!(
        interp
            .call_json("nested", &[json!({ "Twice": { "Circle": 4.0 } })])
            .unwrap(),
        json!(4.0)
    );
}

#[test]
fn string_and_vec_methods() {
    let src = r#"
fn has(s: Text, t: Text) -> Bool:
    s.contains(t)

fn edges(s: Text) -> Bool:
    s.starts_with("ab") and s.ends_with("yz")

fn fix(s: Text) -> Text:
    s.replace("cat", "dog")

fn words(s: Text) -> u64:
    s.split(" ").len()

fn second(s: Text) -> Text:
    let parts = s.split(",")
    parts[1]

fn collect(n: Int) -> u64:
    let v = [0]
    for i in range(1, n):
        v.push(i * i)
    v.len()

fn shares(n: Int) -> Bool:
    let v = [1, 2]
    v.push(n)
    v.contains(n)
"#;
    assert!(matches!(
        run(
            src,
            "has",
            vec![Value::str("hello world"), Value::str("o w")]
        ),
        Value::Bool(true)
    ));
    assert!(matches!(
        run(src, "has", vec![Value::str("hello"), Value::str("xyz")]),
        Value::Bool(false)
    ));
    assert!(matches!(
        run(src, "edges", vec![Value::str("ab...yz")]),
        Value::Bool(true)
    ));
    assert!(matches!(
        run(src, "edges", vec![Value::str("ab...")]),
        Value::Bool(false)
    ));
    assert_eq!(
        text(run(src, "fix", vec![Value::str("a cat and a cat")])),
        "a dog and a dog"
    );
    assert_eq!(int(run(src, "words", vec![Value::str("one two three")])), 3);
    assert_eq!(text(run(src, "second", vec![Value::str("a,b,c")])), "b");
    assert_eq!(int(run(src, "collect", vec![Value::Int(5)])), 5);
    assert!(matches!(
        run(src, "shares", vec![Value::Int(9)]),
        Value::Bool(true)
    ));
}

#[test]
fn option_values_and_matching() {
    let src = r#"
fn parse(s: Text) -> Int?:
    s.to_int()

fn describe(s: Text) -> Text:
    match parse(s):
        case Some(n):
            f"number {n}"
        case None:
            "not a number"

fn pick(flag: Bool) -> Text?:
    if flag:
        Some("yes")
    else:
        None

fn first_or(v: List[Int], d: Int) -> Int:
    v.first().unwrap_or(d)

fn last_plus(v: List[Int]) -> Int:
    match v.last():
        case Some(x):
            x + 1
        case None:
            0

fn counts(v: List[Int]) -> Bool:
    v.get(1).is_some() and v.get(9).is_none()

fn drain() -> Int:
    let v = [1, 2, 3]
    var total = 0
    while v.len() > 0:
        match v.pop():
            case Some(x):
                total += x
            case None:
                break
    total

fn where_is(s: Text) -> u64?:
    s.find("lo")

fn sorted() -> List[Int]:
    let v = [3, 1, 2]
    v.sort()
    v

fn reversed() -> List[Text]:
    let v = "c b a".split(" ")
    v.sort()
    v.reverse()
    v
"#;
    assert_eq!(
        text(run(src, "describe", vec![Value::str("42")])),
        "number 42"
    );
    assert_eq!(
        text(run(src, "describe", vec![Value::str("x1")])),
        "not a number"
    );
    assert_eq!(
        run(src, "pick", vec![Value::Bool(true)]).to_json(),
        json!("yes")
    );
    assert_eq!(
        run(src, "pick", vec![Value::Bool(false)]).to_json(),
        json!(null)
    );
    assert_eq!(
        run(src, "pick", vec![Value::Bool(true)]).to_string(),
        "Some(\"yes\")"
    );
    let ints = |xs: &[i64]| Value::vec(xs.iter().map(|x| Value::Int(*x)).collect());
    assert_eq!(int(run(src, "first_or", vec![ints(&[]), Value::Int(7)])), 7);
    assert_eq!(
        int(run(src, "first_or", vec![ints(&[5, 6]), Value::Int(7)])),
        5
    );
    assert_eq!(int(run(src, "last_plus", vec![ints(&[5, 6])])), 7);
    assert!(matches!(
        run(src, "counts", vec![ints(&[1, 2])]),
        Value::Bool(true)
    ));
    assert_eq!(int(run(src, "drain", vec![])), 6);
    assert_eq!(
        run(src, "where_is", vec![Value::str("hello")]).to_json(),
        json!(3)
    );
    assert_eq!(run(src, "sorted", vec![]).to_json(), json!([1, 2, 3]));
    assert_eq!(
        run(src, "reversed", vec![]).to_json(),
        json!(["c", "b", "a"])
    );
    let interp = load(src);
    assert_eq!(
        interp.call_json("pick", &[json!(true)]).unwrap(),
        json!("yes")
    );
    assert_eq!(interp.call_json("parse", &[json!("9")]).unwrap(), json!(9));
}

#[test]
fn maps() {
    let src = r#"
fn word_counts(text: Text) -> Map[Text, Int]:
    let counts: Map[Text, Int] = std.map.new()
    for w in text.split(" "):
        let n = counts.get(w).unwrap_or(0)
        counts.insert(w, n + 1)
    counts

fn lookup(text: Text, w: Text) -> Int:
    match word_counts(text).get(w):
        case Some(n):
            n
        case None:
            0 - 1

fn size(text: Text) -> u64:
    word_counts(text).len()

fn keys_of(text: Text) -> List[Text]:
    word_counts(text).keys()

fn ids() -> Map[Int, Text]:
    let m: Map[Int, Text] = std.map.new()
    m.insert(2, "two")
    m.insert(1, "one")
    m.remove(2)
    m

fn has(k: Int, m: Map[Int, Text]) -> Bool:
    m.contains_key(k)
"#;
    let t = Value::str("a b a c a b");
    assert_eq!(int(run(src, "lookup", vec![t.clone(), Value::str("a")])), 3);
    assert_eq!(
        int(run(src, "lookup", vec![t.clone(), Value::str("z")])),
        -1
    );
    assert_eq!(int(run(src, "size", vec![t.clone()])), 3);
    assert_eq!(
        run(src, "keys_of", vec![t.clone()]).to_json(),
        json!(["a", "b", "c"])
    );
    assert_eq!(
        run(src, "word_counts", vec![t]).to_json(),
        json!({"a": 3, "b": 2, "c": 1})
    );
    assert_eq!(run(src, "ids", vec![]).to_json(), json!({"1": "one"}));
    let interp = load(src);
    assert_eq!(
        interp
            .call_json("has", &[json!(5), json!({"5": "five"})])
            .unwrap(),
        json!(true)
    );
    assert_eq!(
        interp
            .call_json("has", &[json!(6), json!({"5": "five"})])
            .unwrap(),
        json!(false)
    );
}

#[test]
fn closures_capture_variables() {
    let src = r#"
fn shifted(v: List[Int], k: Int) -> List[Int]:
    v |> std.iter.map(x => x + k) |> std.collect()

fn named(k: Int) -> Int:
    let add = a => a + k
    add(2) + add(3)

fn counter() -> Int:
    var n = 0
    let bump = d => d + 1
    n = bump(n)
    n = bump(n)
    n

fn nested(k: Int) -> Int:
    fn outer(a: Int):
        let inner = b => a + b + k
        inner(1)
    outer(10)

fn filtered(v: List[Int], min: Int) -> List[Int]:
    v |> std.iter.filter(x => x >= min) |> std.collect()
"#;
    let ints = |xs: &[i64]| Value::vec(xs.iter().map(|x| Value::Int(*x)).collect());
    assert_eq!(
        run(src, "shifted", vec![ints(&[1, 2]), Value::Int(10)]).to_json(),
        json!([11, 12])
    );
    assert_eq!(int(run(src, "named", vec![Value::Int(5)])), 7 + 8);
    assert_eq!(int(run(src, "counter", vec![])), 2);
    assert_eq!(int(run(src, "nested", vec![Value::Int(100)])), 111);
    assert_eq!(
        run(src, "filtered", vec![ints(&[1, 5, 9]), Value::Int(5)]).to_json(),
        json!([5, 9])
    );
}

#[test]
fn question_mark_propagates_errors_and_none() {
    let src = r#"
fn parse(s: Text) -> Result[Int, Text]:
    match s.to_int():
        case Some(n):
            Ok(n)
        case None:
            Err(f"not a number: {s}")

fn double(s: Text) -> Result[Int, Text]:
    let n = parse(s)?
    Ok(n * 2)

fn sum(a: Text, b: Text) -> Result[Int, Text]:
    Ok(parse(a)? + parse(b)?)

fn first_even(v: List[Int]) -> Int?:
    let first = v.first()?
    if first % 2 == 0:
        Some(first)
    else:
        None

fn describe(s: Text) -> Text:
    match double(s):
        case Ok(n):
            f"ok {n}"
        case Err(e):
            f"error: {e}"
"#;
    assert_eq!(text(run(src, "describe", vec![Value::str("21")])), "ok 42");
    assert_eq!(
        text(run(src, "describe", vec![Value::str("x")])),
        "error: not a number: x"
    );
    let both = run(src, "sum", vec![Value::str("2"), Value::str("3")]);
    assert_eq!(both.to_json(), json!(5));
    let bad = run(src, "sum", vec![Value::str("2"), Value::str("z")]);
    assert!(matches!(bad, Value::Err(_)), "{bad:?}");
    let ints = |xs: &[i64]| Value::vec(xs.iter().map(|x| Value::Int(*x)).collect());
    assert_eq!(
        run(src, "first_even", vec![ints(&[4, 1])]).to_json(),
        json!(4)
    );
    assert_eq!(
        run(src, "first_even", vec![ints(&[3])]).to_json(),
        json!(null)
    );
    assert_eq!(
        run(src, "first_even", vec![ints(&[])]).to_json(),
        json!(null)
    );
}

#[test]
fn if_let_and_while_let() {
    let src = r#"
fn describe(v: List[Int]) -> Text:
    if v.first() is Some(x):
        f"first {x}"
    else:
        "empty"

fn chained(a: Int?, b: Int?) -> Int:
    if a is Some(x):
        x
    else:
        if b is Some(y):
            y * 10
        else:
            0

fn drain(v: List[Int]) -> Int:
    var total = 0
    while v.pop() is Some(x):
        total += x
    total

fn until_big(v: List[Int]) -> Int:
    var total = 0
    while v.pop() is Some(x):
        if x > 100:
            break
        if x % 2 == 1:
            continue
        total += x
    total

fn set_only(r: Result[Int, Text]) -> Int:
    var out = 0
    if r is Ok(n):
        out = n
    out
"#;
    let ints = |xs: &[i64]| Value::vec(xs.iter().map(|x| Value::Int(*x)).collect());
    assert_eq!(text(run(src, "describe", vec![ints(&[7, 8])])), "first 7");
    assert_eq!(text(run(src, "describe", vec![ints(&[])])), "empty");
    let some = |n| Value::some(Value::Int(n));
    assert_eq!(int(run(src, "chained", vec![some(4), Value::none()])), 4);
    assert_eq!(int(run(src, "chained", vec![Value::none(), some(4)])), 40);
    assert_eq!(
        int(run(src, "chained", vec![Value::none(), Value::none()])),
        0
    );
    assert_eq!(int(run(src, "drain", vec![ints(&[1, 2, 3, 4])])), 10);
    assert_eq!(
        int(run(src, "until_big", vec![ints(&[2, 500, 4, 5, 6])])),
        6 + 4
    );
    assert_eq!(
        int(run(
            src,
            "set_only",
            vec![Value::Ok(Box::new(Value::Int(9)))]
        )),
        9
    );
    assert_eq!(
        int(run(
            src,
            "set_only",
            vec![Value::Err(Box::new(Value::str("x")))]
        )),
        0
    );
}

#[test]
fn tuples_and_iterator_methods() {
    let src = r#"
fn swap(p: (Int, Text)) -> (Text, Int):
    (p.1, p.0)

fn destructure(a: Int) -> Int:
    let x, y = (a, a * 2)
    let _, z, w = (1, 2, 3)
    x + y + z + w

fn labelled(v: List[Text]) -> List[Text]:
    v.enumerate().map(((i, s)) => f"{i}:{s}")

fn pairs() -> List[(Int, Text)]:
    [1, 2].zip(["a", "b"])

fn total_len(v: List[Text]) -> u64:
    v.map(s => s.len()).fold(0, (acc, n) => acc + n)

fn evens(v: List[Int]) -> List[Int]:
    v.filter(x => x % 2 == 0)

fn stats(v: List[Int]) -> (Int, Int?, Int?):
    (v.sum(), v.min(), v.max())

fn has_big(v: List[Int]) -> Bool:
    v.any(x => x > 10) and not v.all(x => x > 10)

fn first_big(v: List[Int]) -> Int?:
    v.find(x => x > 10)

fn counts(text: Text) -> List[Text]:
    let m: Map[Text, Int] = std.map.new()
    for w in text.split(" "):
        m.insert(w, m.get(w).unwrap_or(0) + 1)
    let out = [""]
    out.pop()
    for word, n in m.entries():
        out.push(f"{word}={n}")
    out

fn joined(v: List[Text]) -> Text:
    v.join("+")

fn skipped(v: List[Int]) -> List[Int]:
    v.skip(2)

fn tuple_match(p: (Bool, Int)) -> Text:
    match p:
        case (True, n):
            f"yes {n}"
        case (False, 0):
            "zero"
        case (False, n):
            f"no {n}"
"#;
    let ints = |xs: &[i64]| Value::vec(xs.iter().map(|x| Value::Int(*x)).collect());
    let strs = |xs: &[&str]| Value::vec(xs.iter().map(|x| Value::str(*x)).collect());
    let pair = Value::Tuple(Arc::new(vec![Value::Int(7), Value::str("seven")]));
    assert_eq!(run(src, "swap", vec![pair]).to_json(), json!(["seven", 7]));
    assert_eq!(
        int(run(src, "destructure", vec![Value::Int(5)])),
        5 + 10 + 2 + 3
    );
    assert_eq!(
        run(src, "labelled", vec![strs(&["a", "b"])]).to_json(),
        json!(["0:a", "1:b"])
    );
    assert_eq!(
        run(src, "pairs", vec![]).to_json(),
        json!([[1, "a"], [2, "b"]])
    );
    assert_eq!(int(run(src, "total_len", vec![strs(&["ab", "cde"])])), 5);
    assert_eq!(
        run(src, "evens", vec![ints(&[1, 2, 3, 4])]).to_json(),
        json!([2, 4])
    );
    assert_eq!(
        run(src, "stats", vec![ints(&[3, 9, 1])]).to_json(),
        json!([13, 1, 9])
    );
    assert_eq!(
        run(src, "stats", vec![ints(&[])]).to_json(),
        json!([0, null, null])
    );
    assert!(matches!(
        run(src, "has_big", vec![ints(&[1, 20])]),
        Value::Bool(true)
    ));
    assert_eq!(
        run(src, "first_big", vec![ints(&[1, 20, 30])]).to_json(),
        json!(20)
    );
    assert_eq!(
        run(src, "counts", vec![Value::str("b a b")]).to_json(),
        json!(["a=1", "b=2"])
    );
    assert_eq!(
        text(run(src, "joined", vec![strs(&["x", "y", "z"])])),
        "x+y+z"
    );
    assert_eq!(
        run(src, "skipped", vec![ints(&[1, 2, 3, 4])]).to_json(),
        json!([3, 4])
    );
    let tup = |b, n| Value::Tuple(Arc::new(vec![Value::Bool(b), Value::Int(n)]));
    assert_eq!(text(run(src, "tuple_match", vec![tup(true, 4)])), "yes 4");
    assert_eq!(text(run(src, "tuple_match", vec![tup(false, 0)])), "zero");
    assert_eq!(text(run(src, "tuple_match", vec![tup(false, 5)])), "no 5");
    // Tuples are JSON arrays on the way in and out.
    let interp = load(src);
    assert_eq!(
        interp.call_json("swap", &[json!([3, "x"])]).unwrap(),
        json!(["x", 3])
    );
}

#[test]
fn impl_methods_and_associated_functions() {
    let src = r#"
type Point:
    x: Float
    y: Float

    fn new(x: Float, y: Float) -> Point:
        Point(x: x, y: y)

    fn origin() -> Self:
        Self(x: 0.0, y: 0.0)

    fn norm2(self) -> Float:
        self.x * self.x + self.y * self.y

    fn plus(self, other: Point) -> Point:
        Point.new(self.x + other.x, self.y + other.y)

    fn scaled(self, k: Float) -> Self:
        Self.new(self.x * k, self.y * k)

enum Shape:
    Circle(Float)
    Square(Float)

extend Shape:
    fn area(self) -> Float:
        match self:
            case Shape.Circle(r):
                r * r * 3.0
            case Shape.Square(s):
                s * s

    fn unit_square() -> Shape:
        Self.Square(1.0)

    fn describe(self) -> Text:
        f"area {self.area()}"

fn run() -> Float:
    let p = Point.new(3.0, 4.0)
    let q = p.plus(Point.origin()).scaled(2.0)
    q.norm2() + Shape.unit_square().area() + Shape.Circle(1.0).area()

fn text() -> Text:
    Shape.Square(2.0).describe()

fn chain() -> Float:
    Point.new(1.0, 1.0).plus(Point.new(2.0, 3.0)).norm2()
"#;
    assert_eq!(float(run(src, "run", vec![])), 100.0 + 1.0 + 3.0);
    assert_eq!(text(run(src, "text", vec![])), "area 4.0");
    assert_eq!(float(run(src, "chain", vec![])), 9.0 + 16.0);
}

#[test]
fn traits_with_dynamic_dispatch_and_dependency_injection() {
    let src = r#"
trait Shape:
    fn area(self) -> Float: ...

    fn name(self) -> Text: ...

    fn describe(self) -> Text:
        f"{self.name()} with area {self.area()}"

type Circle:
    r: Float

type Square:
    s: Float

extend Circle(Shape):
    fn area(self) -> Float:
        self.r * self.r * 3.0

    fn name(self) -> Text:
        "circle"

extend Square(Shape):
    fn area(self) -> Float:
        self.s * self.s

    fn name(self) -> Text:
        "square"

    fn describe(self) -> Text:
        "a square!"

fn total(shapes: List[Shape]) -> Float:
    shapes.map(s => s.area()).sum()

fn labels(shapes: List[Shape]) -> List[Text]:
    shapes.map(s => s.describe())

fn mixed() -> List[Shape]:
    [Circle(r: 1.0), Square(s: 2.0)]

fn direct() -> Text:
    Circle(r: 2.0).describe()

# An injected dependency: the service does not know which store it talks to.
trait Store:
    fn load(self, key: Text) -> Text?: ...

    fn save(self, key: Text, value: Text): ...

type MemoryStore(Store):
    data: Map[Text, Text]

    fn load(self, key: Text) -> Text?:
        self.data.get(key)

    fn save(self, key: Text, value: Text):
        self.data.insert(key, value)

type Greeter:
    store: Store

    fn new(store: Store) -> Greeter:
        Greeter(store: store)

    fn greet(self, name: Text) -> Text:
        match self.store.load(name):
            case Some(g):
                g
            case None:
                self.store.save(name, f"hello, {name}")
                f"first time, {name}"

fn visits() -> List[Text]:
    let g = Greeter.new(MemoryStore(data: std.map.new()))
    [g.greet("ana"), g.greet("ana"), g.greet("bo")]
"#;
    assert_eq!(
        float(run(src, "total", vec![run(src, "mixed", vec![])])),
        3.0 + 4.0
    );
    assert_eq!(
        run(src, "labels", vec![run(src, "mixed", vec![])]).to_json(),
        json!(["circle with area 3.0", "a square!"])
    );
    assert_eq!(text(run(src, "direct", vec![])), "circle with area 12.0");
    assert_eq!(
        run(src, "visits", vec![]).to_json(),
        json!(["first time, ana", "hello, ana", "first time, bo"])
    );
}

#[test]
fn generic_functions_structs_and_impls_are_monomorphized() {
    let src = r#"
trait Named:
    fn name(self) -> Text: ...

type Dog(Named):
    n: Text

    fn name(self) -> Text:
        self.n

fn first[T](xs: List[T]) -> T:
    xs[0]

fn id[T](x: T) -> T:
    x

fn greet[T: Named](x: T) -> Text:
    f"hi {x.name()}"

type Pair[A, B]:
    a: A
    b: B

    fn new(a: A, b: B) -> Pair[A, B]:
        Pair(a: a, b: b)

    fn first(self) -> A:
        self.a

    fn second(self) -> B:
        self.b

fn ints() -> Int:
    let xs: List[Int] = [4, 5, 6]
    let one: Int = 1
    first(xs) + id(one)

fn texts() -> Text:
    first(["x", "y"])

fn named() -> Text:
    greet(Dog(n: "rex"))

fn pairs() -> Text:
    let p = Pair.new(7, "seven")
    f"{p.first()} {p.second()}"
"#;
    assert_eq!(run(src, "ints", vec![]).to_json(), json!(5));
    assert_eq!(text(run(src, "texts", vec![])), "x");
    assert_eq!(text(run(src, "named", vec![])), "hi rex");
    assert_eq!(text(run(src, "pairs", vec![])), "7 seven");
}

#[test]
fn generic_enums_and_methods_are_monomorphized() {
    let src = r#"
enum Tree[T]:
    Leaf
    Node(T, List[T])

extend Tree[T]:
    fn size(self) -> u64:
        match self:
            case Tree.Leaf:
                0
            case Tree.Node(_, rest):
                1 + rest.len()

type Stack[T]:
    items: List[T]

    fn new() -> Stack[T]:
        Stack(items: [])

    fn count(self) -> u64:
        self.items.len()

fn sizes() -> u64:
    let t: Tree[Int] = Tree.Node(1, [2, 3])
    let e: Tree[Text] = Tree.Leaf
    t.size() * 10 + e.size()

fn empty() -> u64:
    let s: Stack[Int] = Stack.new()
    s.count()
"#;
    assert_eq!(run(src, "sizes", vec![]).to_json(), json!(30));
    assert_eq!(run(src, "empty", vec![]).to_json(), json!(0));
}

#[test]
fn json_parses_into_a_type_and_writes_back() {
    let src = r#"
type User:
    name: Text
    age: Int

fn age_of(body: Text) -> Text:
    let r: Result[User, Text] = std.json.parse(body)
    match r:
        case Ok(u):
            f"{u.name}:{u.age}"
        case Err(e):
            e

fn dump(n: Text, a: Int) -> Text:
    std.json.stringify(User(name: n, age: a))
"#;
    let i = load(src);
    let ask = |b: &str| text(i.call("age_of", vec![Value::str(b)]).unwrap());
    assert_eq!(ask(r#"{"name":"Ana","age":30}"#), "Ana:30");
    assert!(ask(r#"{"name":"Ana"}"#).contains("missing field `age`"));
    assert!(ask("nope").contains("not valid JSON"));
    assert_eq!(
        text(
            i.call("dump", vec![Value::str("Bo"), Value::Int(5)])
                .unwrap()
        ),
        r#"{"age":5,"name":"Bo"}"#
    );
}

#[test]
fn files_and_environment_variables() {
    let dir = std::env::temp_dir().join(format!("telvax-fs-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("note.txt");
    let src = r#"
fn roundtrip(path: Text, body: Text) -> Text:
    match std.fs.write(path, body):
        case Err(e):
            e
        case Ok(_):
            match std.fs.read(path):
                case Ok(t):
                    t
                case Err(e):
                    e

fn missing(path: Text) -> Text:
    match std.fs.read(path):
        case Ok(t):
            t
        case Err(e):
            e

fn has(path: Text) -> Bool:
    std.fs.exists(path)

fn var_or(name: Text) -> Text:
    std.env.get(name).unwrap_or("unset")
"#;
    let i = load(src);
    let path = file.to_str().unwrap();
    assert_eq!(
        text(
            i.call("roundtrip", vec![Value::str(path), Value::str("hello")])
                .unwrap()
        ),
        "hello"
    );
    assert!(matches!(
        i.call("has", vec![Value::str(path)]).unwrap(),
        Value::Bool(true)
    ));
    let gone = dir.join("nope.txt");
    assert!(
        text(
            i.call("missing", vec![Value::str(gone.to_str().unwrap())])
                .unwrap()
        )
        .contains("cannot read")
    );
    unsafe { std::env::set_var("TELVAX_TEST_ENV_VAR", "set") };
    assert_eq!(
        text(
            i.call("var_or", vec![Value::str("TELVAX_TEST_ENV_VAR")])
                .unwrap()
        ),
        "set"
    );
    assert_eq!(
        text(
            i.call("var_or", vec![Value::str("TELVAX_NOT_SET_ANYWHERE")])
                .unwrap()
        ),
        "unset"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

const ERRORS: &str = r#"
fn parse(s: Text) -> Result[Int, Text]:
    match s.to_int():
        case Some(n):
            Ok(n)
        case None:
            Err(f"bad: {s}")

fn chain() -> Int:
    parse("20").map(n => n + 1).and_then(n => parse("2")).unwrap_or(0)

fn recover() -> Int:
    parse("x").unwrap_or_else(e => 6)

fn flags() -> Bool:
    parse("x").map_err(e => 0).is_err() and parse("1").is_ok()

fn kept() -> Int:
    let o = Some(4)
    o.map(n => n * 2).filter(n => n > 5).unwrap_or_else(() => 0)

fn dropped() -> Int:
    let o = Some(4)
    o.filter(n => n > 5).unwrap_or_else(() => -1)

fn to_result() -> Int:
    let o = Some(4)
    o.ok_or("nothing").and_then(n => parse("3")).unwrap()

fn boom() -> Int:
    parse("x").unwrap()

fn expected() -> Int:
    parse("x").expect("port")
"#;

#[test]
fn option_and_result_methods() {
    assert_eq!(int(run(ERRORS, "chain", vec![])), 2);
    assert_eq!(int(run(ERRORS, "recover", vec![])), 6);
    assert!(matches!(run(ERRORS, "flags", vec![]), Value::Bool(true)));
    assert_eq!(int(run(ERRORS, "kept", vec![])), 8);
    assert_eq!(int(run(ERRORS, "dropped", vec![])), -1);
    assert_eq!(int(run(ERRORS, "to_result", vec![])), 3);
    let e = load(ERRORS).call("boom", vec![]).unwrap_err().to_string();
    assert!(e.contains("called `unwrap` on `Err`: bad: x"), "{e}");
    let e = load(ERRORS)
        .call("expected", vec![])
        .unwrap_err()
        .to_string();
    assert!(e.contains("port: bad: x"), "{e}");
}

#[test]
fn functions_are_values() {
    let src = r#"
type Handler:
    run: (Int) -> Int

fn twice(x: Int) -> Int:
    x * 2

fn apply(f: (Int) -> Int, x: Int) -> Int:
    f(x)

fn adder(n: Int) -> (Int) -> Int:
    x => x + n

fn compose(f: (Int) -> Int, g: (Int) -> Int) -> (Int) -> Int:
    x => g(f(x))

fn pick(flag: Bool) -> (Int) -> Int:
    if flag:
        twice
    else:
        adder(100)

fn field() -> Int:
    let h = Handler(run: x => x + 7)
    h.run(1)

fn sum() -> Int:
    apply(twice, 3) + adder(5)(1) + compose(twice, adder(1))(4) + pick(True)(2) + pick(False)(2) + field()
"#;
    // 6 + 6 + 9 + 4 + 102 + 8
    assert_eq!(int(run(src, "sum", vec![])), 135);
}

#[test]
fn methods_with_type_parameters_of_their_own() {
    let src = r#"
type Stack[T]:
    items: List[T]

    fn map[U](self, f: (T) -> U) -> Stack[U]:
        let out: List[U] = []
        for x in self.items:
            out.push(f(x))
        Stack(items: out)

    fn fold[A](self, init: A, f: (A, T) -> A) -> A:
        var acc = init
        for x in self.items:
            acc = f(acc, x)
        acc

type Plain:
    n: Int

    fn apply[U](self, f: (Int) -> U) -> U:
        f(self.n)

fn run() -> Int:
    let s: Stack[Int] = Stack(items: [1, 2, 3])
    let doubled = s.map(x => x * 2)
    let labels = s.map(x => f"<{x}>")
    let total: Int = doubled.fold(0, (a, x) => a + x)
    let width: Int = labels.fold(0, (a, x) => a + 1)
    total * 10 + width + Plain(n: 4).apply(x => x + 100)
"#;
    // total 12, width 3, apply 104
    assert_eq!(int(run(src, "run", vec![])), 12 * 10 + 3 + 104);
}

#[test]
fn methods_without_an_arrow_return_their_last_expression() {
    let src = r#"
trait Shape:
    fn area(self) -> Int: ...

type Sq(Shape):
    s: Int

    fn area(self):
        self.s * self.s

type P:
    n: Int

    fn double(self):
        self.n * 2

    fn make(k: Int):
        P(n: k)

extend P:
    fn triple(self):
        self.n * 3

let base = twice(5)

fn twice(x: Int):
    x * 2

fn run() -> Int:
    let p = P.make(4)
    Sq(s: 3).area() + p.double() + p.triple() + base
"#;
    // 9 + 8 + 12 + 10
    assert_eq!(int(run(src, "run", vec![])), 39);
}

fn shown(src: &str) -> String {
    text(run(src, "f", vec![]))
}

#[test]
fn slices_and_negative_indices() {
    let src = r#"
fn f() -> Text:
    let xs = [1, 2, 3, 4, 5]
    let s = "hello"
    f"{xs[1:3]} {xs[:2]} {xs[3:]} {xs[-2:]} {xs[-1]} {xs[:]} {xs[4:2]} {xs[9:]} {xs[-9:2]}|{s[1:3]} {s[0]} {s[-1]} {s[:2]} {s[2:]}"
"#;
    assert_eq!(
        shown(src),
        "[2, 3] [1, 2] [4, 5] [4, 5] 5 [1, 2, 3, 4, 5] [] [] [1, 2]|el h o he llo"
    );
    // Slices count characters, not bytes.
    let src = "fn f() -> Text:\n    let s = \"añb\"\n    f\"{s[1:2]}{s[-1]}\"\n";
    assert_eq!(shown(src), "ñb");
    let src = "fn f() -> Text:\n    let s = \"ab\"\n    s[2]\n";
    let e = load(src).call("f", vec![]).unwrap_err();
    assert!(e.message.contains("index out of bounds"), "{e}");
}

#[test]
fn maps_are_indexed_assigned_and_iterated() {
    let src = r#"
fn f() -> Text:
    var m = {"a": 1, "b": 2}
    m["c"] = 3
    m["a"] = m["a"] + 10
    var keys = ""
    for k in m:
        keys = keys + k
    let n: Map[Int, Text] = {}
    var names = {1: "one"}
    names[2] = "two"
    f"{keys} {m["a"]} {m.len()} {n.len()} {names[2]}"
"#;
    assert_eq!(shown(src), "abc 11 3 0 two");
    let src = r#"
fn f() -> Int:
    let m = {"a": 1}
    m["zzz"]
"#;
    let e = load(src).call("f", vec![]).unwrap_err();
    assert!(e.message.contains("key not found: zzz"), "{e}");
    // An entry that holds a list is updated through the map.
    let src = r#"
fn f() -> Int:
    var m = {"a": [1, 2]}
    m["a"][0] = 9
    m["a"][0] + m["a"][1]
"#;
    assert_eq!(int(run(src, "f", vec![])), 11);
}

#[test]
fn comprehensions() {
    let src = r#"
fn f() -> Text:
    let xs = [1, 2, 3, 4, 5]
    let a = [x * 2 for x in xs if x > 2]
    let b = [i * i for i in range(4)]
    let c = [f"{i}:{x}" for i, x in xs.enumerate() if i < 2]
    let d = [x for x in xs if x % 2 == 0]
    let e = [1 for _ in xs]
    f"{a} {b} {c} {d} {e.len()}"
"#;
    assert_eq!(
        shown(src),
        "[6, 8, 10] [0, 1, 4, 9] [\"0:1\", \"1:2\"] [2, 4] 5"
    );
}

#[test]
fn power_floor_division_and_chained_comparisons() {
    let src = r#"
fn f() -> Text:
    let z = 0
    f"{2 ** 10} {2 ** 3 ** 2} {-2 ** 2} {2.0 ** -1} {7 // 2} {-7 // 2} {7 // -2} {7.5 // 2} {1 < 2 < 3} {3 < 2 < 5} {1 < 2 <= 2} {z < 1 < 2}"
"#;
    assert_eq!(
        shown(src),
        "1024 512 -4 0.5 3 -4 -4 3.0 True False True True"
    );
    for (body, message) in [
        ("2 ** -1", "negative exponent"),
        ("1 // z", "division by zero"),
        ("9223372036854775807 ** 2", "overflow"),
    ] {
        let src = format!("fn f() -> Int:\n    let z = 0\n    {body}\n");
        let e = load(&src).call("f", vec![]).unwrap_err();
        assert!(e.message.contains(message), "{body}: {e}");
    }
    // The middle operand of a chain is computed once.
    let src = r#"
fn bump(c: List[Int]) -> u64:
    c.push(1)
    c.len()

fn f() -> u64:
    let c: List[Int] = []
    let ok = 0 < bump(c) < 5
    c.len()
"#;
    assert_eq!(int(run(src, "f", vec![])), 1);
}

#[test]
fn a_missing_value_gets_a_default() {
    let src = r#"
fn boom() -> Int:
    assert(False)
    0

fn f() -> Text:
    let o: Int? = None
    let p: Int? = Some(3)
    let m = {"a": 1}
    f"{o ?? 7} {p ?? boom()} {m.get("a") ?? 0} {m.get("b") ?? -1}"
"#;
    assert_eq!(shown(src), "7 3 1 -1");
}

#[test]
fn texts_and_lists_with_operators() {
    let src = r#"
fn f() -> Text:
    let t = """line one
line two"""
    f"{"ab" * 3}|{"x" * 0}|{[1, 2] + [3]}|{t.lines().len()}"
"#;
    assert_eq!(shown(src), "ababab||[1, 2, 3]|2");
    let src = "fn f() -> Text:\n    \"ab\" * -1\n";
    let e = load(src).call("f", vec![]).unwrap_err();
    assert!(e.message.contains("not negative"), "{e}");
}

#[test]
fn f_string_formats() {
    let src = r#"
fn f() -> Text:
    let pi = 3.14159
    let n = 42
    let name = "bo"
    f"{pi:.2f}|{pi=}|{n:05d}|{n:>6}|{n:<6}|{name:^6}|{name:*>5}|{1234567:,}|{0.256:.1%}|{255:x}|{255:X}|{5:b}|{-7:04d}|{name:.1}|{n=}|{n == 42}|{pi:8.3f}"
"#;
    assert_eq!(
        shown(src),
        "3.14|pi=3.14159|00042|    42|42    |  bo  |***bo|1,234,567|25.6%|ff|FF|101|-007|b|n=42|True|   3.142"
    );
    for spec in ["q", ".f", "d"] {
        let src = format!("fn f() -> Text:\n    let s = \"a\"\n    f\"{{s:{spec}}}\"\n");
        assert!(load(&src).call("f", vec![]).is_err(), "{spec}");
    }
}

#[test]
fn numbers_and_conversions() {
    let src = r#"
fn f() -> Text:
    let x = -3
    f"{abs(x)} {abs(-2.5)} {max(2, 9)} {min(2, 9)} {max(1.5, 2)} {min("b", "a")} {int(3.9)} {int(-3.9)} {int(4)} {float(3) + 0.5} {str(12) + "!"} {std.math.sqrt(16.0)} {std.math.floor(2.7)} {std.math.ceil(2.1)} {std.math.round(2.5)} {range(3)} {range(2, 5)}"
"#;
    assert_eq!(
        shown(src),
        "3 2.5 9 2 2.0 a 3 -3 4 3.5 12! 4.0 2.0 3.0 3.0 [0, 1, 2] [2, 3, 4]"
    );
    // A name of the program wins over the short names.
    let src = r#"
fn max(a: Int, b: Int) -> Int:
    a - b

fn f() -> Int:
    max(5, 2)
"#;
    assert_eq!(int(run(src, "f", vec![])), 3);
}

const PATTERNS: &str = r#"
type P:
    x: Int
    y: Int

enum S:
    C(Int)
    R(Int, Int)
    E

fn sign(n: Int) -> Text:
    match n:
        case 0:
            "zero"
        case -1 | -2:
            "small negative"
        case x if x < 0:
            "negative"
        case 1 | 2 | 3:
            "small"
        case _:
            "big"

fn kind(s: S) -> Int:
    match s:
        case S.C(_) | S.R(_, _):
            0
        case S.E:
            1

fn describe(xs: List[Int]) -> Text:
    match xs:
        case []:
            "empty"
        case [one]:
            f"one {one}"
        case [first, *rest]:
            f"{first} then {rest.len()}"

fn ends(xs: List[Int]) -> Text:
    match xs:
        case [a, *_, z]:
            f"{a}..{z}"
        case _:
            "short"

fn where_is(p: P) -> Text:
    match p:
        case P(x: 0, y: 0):
            "origin"
        case P(x: 0):
            "on y"
        case P(y: yy, x: xx):
            f"{xx},{yy}"

fn nested(v: (S, List[P])) -> Text:
    match v:
        case (S.R(a, 2), [P(x: 1, y: b), *_]):
            f"{a}{b}"
        case (S.C(c), [_, P(y: d)]):
            f"{c}{d}"
        case _:
            "none"
"#;

fn patterns(body: &str) -> String {
    shown(&format!("{PATTERNS}\n{body}"))
}

#[test]
fn match_with_guards_alternatives_and_signs() {
    let all = patterns(
        r#"
fn f() -> Text:
    f"{sign(0)}|{sign(-2)}|{sign(-9)}|{sign(2)}|{sign(50)}|{kind(S.R(1, 2))}{kind(S.E)}"
"#,
    );
    assert_eq!(all, "zero|small negative|negative|small|big|01");
    // A negative float, and a guard that sees the names of its pattern.
    let src = r#"
fn f() -> Text:
    let a = match -2.5:
        case -2.5:
            "match"
        case _:
            "no"
    let b = match (3, 4):
        case (x, y) if x > y:
            "first"
        case (x, y) if x + y == 7:
            "sum"
        case _:
            "none"
    f"{a} {b}"
"#;
    assert_eq!(shown(src), "match sum");
}

#[test]
fn match_with_lists_and_types() {
    let out = patterns(
        r#"
fn f() -> Text:
    f"{describe([])}|{describe([4])}|{describe([1, 2, 3])}|{ends([1, 2, 3])}|{ends([1])}|{ends([5, 6])}"
"#,
    );
    assert_eq!(out, "empty|one 4|1 then 2|1..3|short|5..6");
    let out = patterns(
        r#"
fn f() -> Text:
    f"{where_is(P(x: 0, y: 0))}|{where_is(P(x: 0, y: 5))}|{where_is(P(x: 3, y: 4))}"
"#,
    );
    assert_eq!(out, "origin|on y|3,4");
    let out = patterns(
        r#"
fn f() -> Text:
    let a = nested((S.R(7, 2), [P(x: 1, y: 8), P(x: 0, y: 0)]))
    let b = nested((S.C(3), [P(x: 0, y: 0), P(x: 1, y: 9)]))
    let c = nested((S.E, []))
    f"{a} {b} {c}"
"#,
    );
    assert_eq!(out, "78 39 none");
    // `*rest` is a list that can be used, and a list of texts matches its literals.
    let src = r#"
fn total(xs: List[Int]) -> Int:
    match xs:
        case []:
            0
        case [head, *tail]:
            head + total(tail)

fn f() -> Text:
    let words = match ["a", "b"]:
        case ["a", second]:
            second
        case _:
            "?"
    f"{total([1, 2, 3, 4])} {words}"
"#;
    assert_eq!(shown(src), "10 b");
}

#[test]
fn match_is_a_value_and_if_is_has_a_condition() {
    let src = r#"
fn pick(n: Int) -> Int:
    let a = match n:
        case 1:
            10
        case _:
            let k = n * 2
            k + 1
    var b = 0
    b = match a:
        case 10:
            1
        case _:
            2
    return match b:
        case 1:
            100
        case _:
            200

fn f() -> Text:
    let o: Int? = Some(5)
    var hits = ""
    if o is Some(n) and n > 9:
        hits = hits + "big"
    elif o is Some(n) and n > 3:
        hits = hits + f"mid{n}"
    else:
        hits = hits + "none"
    let stack = [1, 2, 3, 4]
    var seen = 0
    while stack.pop() is Some(v) and v > 1:
        seen = seen + v
    f"{pick(1)} {pick(7)} {hits} {seen}"
"#;
    assert_eq!(shown(src), "100 200 mid5 9");
}

#[test]
fn tuples_are_taken_apart_and_swapped() {
    let src = r#"
fn add((a, b): (Int, Int), k: Int) -> Int:
    a + b + k

fn deep(t: ((Int, Int), (Int, (Int, Int)))) -> Int:
    let (a, b), (c, (d, e)) = t
    a * 10000 + b * 1000 + c * 100 + d * 10 + e

fn f() -> Text:
    var x = 1
    var y = 2
    x, y = y, x
    let q, r = 7 // 2, 7 % 2
    var out = ""
    for i, (a, b) in [(1, 2), (3, 4)].enumerate():
        out = out + f"{i}{a}{b},"
    let sum = [(1, (2, 3)), (4, (5, 6))].map(((a, (b, c))) => a + b + c)
    f"{add((1, 2), 3)} {deep(((1, 2), (3, (4, 5))))} {x}{y} {q}{r} {out} {sum}"
"#;
    assert_eq!(shown(src), "6 12345 21 31 012,134, [6, 15]");
}

#[test]
fn recursive_enums_and_methods() {
    let src = r#"
enum Expr:
    Num(Int)
    Add(Expr, Expr)
    Mul(Expr, Expr)
    Neg(Expr)

    fn eval(self) -> Int:
        match self:
            case Expr.Num(n):
                n
            case Expr.Add(a, b):
                a.eval() + b.eval()
            case Expr.Mul(a, b):
                a.eval() * b.eval()
            case Expr.Neg(a):
                0 - a.eval()

fn simplify(e: Expr) -> Expr:
    match e:
        case Expr.Add(Expr.Num(0), x):
            simplify(x)
        case Expr.Mul(Expr.Num(1), x):
            simplify(x)
        case Expr.Neg(Expr.Neg(x)):
            simplify(x)
        case _:
            e

fn f() -> Text:
    let e = Expr.Add(Expr.Num(2), Expr.Mul(Expr.Num(3), Expr.Neg(Expr.Num(4))))
    let s = simplify(Expr.Neg(Expr.Neg(Expr.Add(Expr.Num(0), Expr.Num(7)))))
    f"{e.eval()} {s.eval()}"
"#;
    assert_eq!(shown(src), "-10 7");
}

#[test]
fn enums_with_traits_and_named_values() {
    let src = r#"
trait Describe:
    fn describe(self) -> Text: ...

enum Shape(Describe):
    Circle(r: Float)
    Rect(w: Float, h: Float)
    Empty

    fn describe(self) -> Text:
        match self:
            case Shape.Circle(r: r):
                f"circle {r}"
            case Shape.Rect(h: h, w: w):
                f"rect {w}x{h}"
            case Shape.Empty:
                "empty"

extend Shape:
    fn unit() -> Shape:
        Shape.Rect(w: 1.0, h: 1.0)

enum Maybe[T]:
    Just(value: T)
    Nothing

    fn or_else(self, d: T) -> T:
        match self:
            case Maybe.Just(value: v):
                v
            case Maybe.Nothing:
                d

fn f() -> Text:
    let m: Maybe[Int] = Maybe.Just(value: 4)
    let n: Maybe[Int] = Maybe.Nothing
    f"{Shape.Rect(h: 2.0, w: 3.0).describe()}|{Shape.Circle(r: 1.5).describe()}|{Shape.Empty.describe()}|{Shape.unit().describe()}|{m.or_else(0)}{n.or_else(9)}|{Shape.Rect(1.0, 2.0)}"
"#;
    assert_eq!(
        shown(src),
        "rect 3.0x2.0|circle 1.5|empty|rect 1.0x1.0|49|Shape.Rect(1.0, 2.0)"
    );
}

#[test]
fn aliases_and_converted_errors() {
    let src = r#"
type Id = Int
type Pair[T] = (T, T)
type Names = Map[Id, Text]

enum AppError:
    from Parse(Text)
    from Range(Int)

fn parse(s: Text) -> Result[Id, Text]:
    match s.to_int():
        case Some(n):
            Ok(n)
        case None:
            Err(f"not a number: {s}")

fn check(n: Id) -> Result[Id, Int]:
    if n > 10:
        return Err(n)
    Ok(n)

fn run(s: Text) -> Result[Id, AppError]:
    let n = parse(s)?
    let m = check(n)?
    Ok(m * 2)

fn swap(p: Pair[Id]) -> Pair[Id]:
    (p.1, p.0)

fn show(r: Result[Id, AppError]) -> Text:
    match r:
        case Ok(v):
            f"ok {v}"
        case Err(AppError.Parse(t)):
            f"parse: {t}"
        case Err(AppError.Range(n)):
            f"range {n}"

fn f() -> Text:
    let names: Names = {1: "one"}
    f"{show(run("4"))}|{show(run("x"))}|{show(run("40"))}|{swap((1, 2))}|{names[1]}"
"#;
    assert_eq!(
        shown(src),
        "ok 8|parse: not a number: x|range 40|(2, 1)|one"
    );
}

#[test]
fn enums_as_map_keys_and_in_comparisons() {
    let src = r#"
enum Level:
    Low
    Mid
    High

fn f() -> Text:
    var counts = {Level.High: 1, Level.Low: 2}
    counts[Level.Mid] = 5
    counts[Level.High] = counts[Level.High] + 10
    var out = ""
    for k in counts:
        out = out + f"{k}={counts[k]},"
    f"{out} {Level.Low < Level.High} {Level.High <= Level.Mid} {Level.Mid > Level.Low} {Level.Low >= Level.Low} {counts.get(Level.Low) ?? 0} {counts.contains_key(Level.Mid)}"
"#;
    assert_eq!(
        shown(src),
        "Level.Low=2,Level.Mid=5,Level.High=11, True False True True 2 True"
    );
    let src = r#"
enum Shape:
    Circle(Int)
    Empty

fn f() -> u64:
    let m = {Shape.Circle(1): 1}
    m.len()
"#;
    let e = load(src).call("f", vec![]).unwrap_err();
    assert!(
        e.message
            .contains("a Map key that is an enum cannot have values"),
        "{e}"
    );
}

#[test]
fn values_are_shown_the_same_way_everywhere() {
    let src = r#"
type Reading:
    name: Text
    value: f32

enum Mark:
    Level(f32)
    Label(Text)

fn f() -> Text:
    let a: f32 = 0.1
    let b: f64 = 2.0
    let xs: List[f32] = [0.1, 0.25]
    let o: Option[f32] = Some(0.3)
    let r = Reading(name: "t", value: 0.1)
    let m = {"k": a}
    let marks = [Mark.Level(0.1), Mark.Label("x")]
    f"{a} {b} {xs} {o} {r} {m} {marks}|{str(a)} {str(xs)}|{[1.0, 2.5]} {("a", 1.0)} {["a, b", "c"]}|{1.0e16} {1.0e-5} {2.0 / 3.0}"
"#;
    assert_eq!(
        shown(src),
        "0.1 2.0 [0.1, 0.25] Some(0.3) Reading(name=\"t\", value=0.1) {\"k\": 0.1} [Mark.Level(0.1), Mark.Label(\"x\")]\
         |0.1 [0.1, 0.25]|[1.0, 2.5] (\"a\", 1.0) [\"a, b\", \"c\"]|1e+16 1e-05 0.6666666666666666"
    );
}

#[test]
fn f_string_formats_in_full() {
    let src = r#"
fn f() -> Text:
    let w = 6
    let n = 42
    let s = "hi"
    let x = 3.14159
    let a = f"{n:>{w}}|{n:<{w}}|{n:^{w}}|{s:>{w}}|{x:{w}.2f}|"
    let b = f"{n:+d} {-n:+d} {n: d} {n:#x} {n:#b} {n:#o} {255:08b} {n:+08d} {-n:=8d}|"
    let c = f"{12345.678:e} {12345.678:.2e} {0.00012:g} {1234567.0:g} {0.1:.3} {3.0:.3} {100000000.0:g}"
    let d = f"{1234567.891:,.2f} {1234567:_} {255:#_x} {255:_b} {65:c} {1.5:%} {0.256:.1%}"
    let e = f"{s!r} {s!s} {x!r} {s!r:>6}|{s=} {n=} {x = } {s=!s} {n=:>5} {n:5} {s:5}|{x:10}|"
    f"{a}\n{b}\n{c}\n{d}\n{e}"
"#;
    assert_eq!(
        shown(src),
        "    42|42    |  42  |    hi|  3.14|\n\
         +42 -42  42 0x2a 0b101010 0o52 11111111 +0000042 -     42|\n\
         1.234568e+04 1.23e+04 0.00012 1.23457e+06 0.1 3.0 1e+08\n\
         1,234,567.89 1_234_567 0xff 1111_1111 A 150.000000% 25.6%\n\
         \"hi\" hi 3.14159   \"hi\"|s=\"hi\" n=42 x = 3.14159 s=hi n=   42    42 hi   |   3.14159|"
    );
    for spec in ["q", ".f", "d", "x"] {
        let src = format!("fn f() -> Text:\n    let s = \"a\"\n    f\"{{s:{spec}}}\"\n");
        assert!(load(&src).call("f", vec![]).is_err(), "{spec}");
    }
}

#[test]
fn braces_raw_text_and_lines_in_f_strings() {
    let src = r#"
fn f() -> Text:
    let n = 3
    let name = "bo"
    let s = f"""Dear {name},
  you have {n:>3} items {{kept}}
  total: {n * 2:05d}"""
    let pattern = r"^\d{3}-\d{4}$"
    f"{{x}} {{{n}}} }}|{s}|{r"a\nb"}|{pattern} {pattern.len()}"
"#;
    assert_eq!(
        shown(src),
        "{x} {3} }|Dear bo,\n  you have   3 items {kept}\n  total: 00006|a\\nb|^\\d{3}-\\d{4}$ 13"
    );
}

#[test]
fn async_functions_run_when_awaited() {
    let src = r#"
async fn fetch(id: Int) -> Result[Text, Text]:
    await std.task.sleep(1)
    if id < 0:
        return Err(f"bad id {id}")
    Ok(f"item {id}")

async fn twice(id: Int) -> Result[Text, Text]:
    let a = await fetch(id)?
    let b = await fetch(id + 1)?
    Ok(f"{a}+{b}")

async fn fact(n: Int) -> Int:
    if n <= 1:
        return 1
    n * await fact(n - 1)

async fn run() -> Text:
    let one = await fetch(1)
    let both = await twice(5)
    let bad = await twice(-1)
    f"{one} {both} {bad} {await fact(10)}"

fn sync_run() -> Int:
    std.task.run(fact(5))
"#;
    assert_eq!(
        shown(&src.replace("async fn run()", "async fn f()")),
        "Ok(\"item 1\") Ok(\"item 5+item 6\") Err(\"bad id -1\") 3628800"
    );
    assert_eq!(int(run(src, "sync_run", vec![])), 120);
    // A call that is not awaited does not run.
    let src = r#"
async fn boom() -> Int:
    assert(False)
    1

fn f() -> Int:
    let t = boom()
    7
"#;
    assert_eq!(int(run(src, "f", vec![])), 7);
}

#[test]
fn gather_runs_tasks_at_the_same_time() {
    // The task that sleeps longest is first in the list and last to finish: they overlap.
    let src = r#"
async fn work(name: Text, ms: Int, log: List[Text]) -> Text:
    await std.task.sleep(ms)
    log.push(name)
    name

async fn f() -> Text:
    let log: List[Text] = []
    let out = await std.task.gather([work("slow", 400, log), work("mid", 200, log), work("fast", 10, log)])
    f"{out} {log}"
"#;
    assert_eq!(
        shown(src),
        "[\"slow\", \"mid\", \"fast\"] [\"fast\", \"mid\", \"slow\"]"
    );
    // The first failure, by position, is the result; an empty list is fine.
    let src = r#"
async fn ok(n: Int) -> Int:
    n

async fn bad(n: Int) -> Int:
    assert_eq(n, 0)
    n

async fn f() -> Text:
    let none: List[Int] = await std.task.gather([])
    let some = await std.task.gather([ok(1), ok(2), ok(3)])
    f"{none} {some}"

async fn g() -> List[Int]:
    await std.task.gather([ok(1), bad(5), ok(3)])
"#;
    assert_eq!(shown(src), "[] [1, 2, 3]");
    let e = load(src).call("g", vec![]).unwrap_err();
    assert!(e.message.contains("assert"), "{e}");
}

#[test]
fn async_endpoints_and_methods() {
    let src = r#"
from std.http import Status

type In:
    a: Int

type Out:
    b: Int

trait Source:
    async fn load(self) -> Int: ...

type Slow(Source):
    n: Int

    async fn load(self) -> Int:
        await std.task.sleep(1)
        self.n * 10

async fn total(sources: List[Source]) -> Int:
    var sum = 0
    for s in sources:
        sum = sum + await s.load()
    sum

@endpoint(POST, "/double")
async fn double(i: In) -> Result[Out, Status]:
    let t = await total([Slow(n: i.a), Slow(n: 1)])
    if t < 0:
        return Err(Status.BadRequest("negative"))
    Ok(Out(b: t))
"#;
    let i = load(src);
    let out = i.call_endpoint("double", &json!({"a": 4})).unwrap();
    assert_eq!(out, json!({"b": 50}));
    // `call` runs an `async fn` to the end.
    assert_eq!(int(run(src, "total", vec![Value::vec(vec![])])), 0);
    // The time functions.
    let src = r#"
fn f() -> Bool:
    let t0 = std.time.now()
    std.time.sleep(20)
    std.time.now() - t0 >= 0.015
"#;
    assert_eq!(run(src, "f", vec![]).to_string(), "True");
}

#[test]
fn a_spawned_task_goes_on_after_the_call_returns() {
    let src = r#"
let jobs: Map[Text, Text] = {}

async fn work(id: Text):
    await std.task.sleep(200)
    jobs.insert(id, "done")

async fn failing():
    await std.task.sleep(1)
    std.test.fail("boom")

fn start(id: Text) -> Text:
    jobs.insert(id, "queued")
    std.task.spawn(work(id))
    "accepted"

fn start_failing():
    std.task.spawn(failing())

fn status(id: Text) -> Text:
    jobs.get(id).unwrap_or("unknown")
"#;
    let wait = |i: &Interp, id: &str, want: &str| {
        for _ in 0..100 {
            if i.call("status", vec![Value::str(id)]).unwrap().to_string() == want {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        false
    };

    // Shared (as `telvax serve` has it): the call returns first and the task finishes later.
    let shared = load(src).into_shared();
    let started = std::time::Instant::now();
    assert_eq!(
        shared
            .call("start", vec![Value::str("a")])
            .unwrap()
            .to_string(),
        "accepted"
    );
    assert!(started.elapsed() < std::time::Duration::from_millis(150));
    assert_eq!(
        shared
            .call("status", vec![Value::str("a")])
            .unwrap()
            .to_string(),
        "queued"
    );
    assert!(wait(&shared, "a", "done"));
    // A failing task does not reach the caller.
    assert!(shared.call("start_failing", vec![]).is_ok());

    // Not shared (`telvax run`): there is nothing to outlive the call, so it waits.
    let alone = load(src);
    alone.call("start", vec![Value::str("b")]).unwrap();
    assert_eq!(
        alone
            .call("status", vec![Value::str("b")])
            .unwrap()
            .to_string(),
        "done"
    );
}

#[test]
fn text_is_searched_with_patterns() {
    let src = r##"
fn has_digits(s: Text) -> Bool:
    std.text.matches(s, "[0-9]+")

fn first_word(s: Text) -> Text?:
    std.text.find(s, "[a-z]+")

fn words(s: Text) -> List[Text]:
    std.text.find_all(s, "[a-z]+")

fn mask(s: Text) -> Text:
    std.text.replace_all(s, "([0-9]{3})-([0-9]+)", "$1-xxxx")

fn broken(s: Text) -> Bool:
    std.text.matches(s, "(")
"##;
    let i = load(src);
    let call = |f: &str, s: &str| i.call(f, vec![Value::str(s)]).unwrap().to_string();
    assert_eq!(call("has_digits", "room 12"), "True");
    assert_eq!(call("has_digits", "no digits"), "False");
    assert_eq!(call("first_word", "42 abc def"), "Some(\"abc\")");
    assert_eq!(call("first_word", "42"), "None");
    assert_eq!(call("words", "ab 12 cd"), "[\"ab\", \"cd\"]");
    assert_eq!(call("mask", "call 555-1234 now"), "call 555-xxxx now");
    let e = i.call("broken", vec![Value::str("x")]).unwrap_err();
    assert!(e.to_string().contains("invalid pattern `(`"), "{e}");
}

#[test]
fn sql_needs_live_and_a_row_type() {
    let src = r#"
import std.db.postgres.PostgresClient

type Job:
    id: Text

let db = PostgresClient.connect("postgresql://localhost/none")

fn run() -> Result[Int, Text]:
    db.execute("DELETE FROM jobs WHERE id = $1", ["a"])
"#;
    let e = load(src).call("run", vec![]).unwrap_err();
    assert!(e.to_string().contains("needs `--live`"), "{e}");

    // The rows are a list of a type of the program.
    let (ast, _) = telvax_parser::parse(
        r#"
import std.db.postgres.PostgresClient

let db = PostgresClient.connect("postgresql://localhost/none")

fn rows() -> Result[Int, Text]:
    let rows: Int = db.query("SELECT 1", [])?
    Ok(rows)
"#,
    );
    let (_, diags) = telvax_hir::lower(&ast.unwrap());
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("cannot infer the rows")),
        "{:?}",
        diags.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

#[test]
fn std_random_makes_ids_and_whole_numbers() {
    let src = r#"
fn id() -> Text:
    std.random.uuid()

fn same() -> Bool:
    std.random.uuid() == std.random.uuid()

fn roll() -> Int:
    var low = 0
    var high = 0
    for i in range(500):
        let n = std.random.int(1, 6)
        if n < 1 or n > 6:
            return -1
        if n == 1:
            low += 1
        if n == 6:
            high += 1
    low * 1000 + high

fn one() -> Int:
    std.random.int(-3, -3)

fn backwards() -> Int:
    std.random.int(2, 1)
"#;
    let id = text(run(src, "id", vec![]));
    assert_eq!(id.len(), 32, "{id}");
    assert!(
        id.chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
        "{id}"
    );
    // Version 4, variant 10xx.
    assert_eq!(&id[12..13], "4", "{id}");
    assert!("89ab".contains(&id[16..17]), "{id}");
    assert!(matches!(run(src, "same", vec![]), Value::Bool(false)));
    let counts = int(run(src, "roll", vec![]));
    assert!(
        counts > 0 && counts / 1000 > 0 && counts % 1000 > 0,
        "the ends were never drawn: {counts}"
    );
    assert_eq!(int(run(src, "one", vec![])), -3);
    let e = load(src).call("backwards", vec![]).unwrap_err();
    assert!(e.to_string().contains("low (2) is above high (1)"), "{e}");
}

#[test]
fn a_let_in_capitals_has_methods_and_fields() {
    let src = r#"
type Config:
    name: Text
    size: Int

let LIMITS: Map[Text, Int] = {"a": 1}
let NAMES: List[Text] = ["x"]
let TITLE = "Sentiment"
let CONFIG = Config(name: "api", size: 3)

fn run() -> Int:
    LIMITS.insert("b", 2)
    NAMES.push("y")
    var n = std.math.to_int(LIMITS.len()) + std.math.to_int(NAMES.len())
    if LIMITS.contains_key("b") and TITLE.starts_with("Sent") and not TITLE.is_empty():
        n += 10
    n + CONFIG.size + std.math.to_int(CONFIG.name.len())
"#;
    // 2 entries + 2 names + 10 + 3 + 3
    assert_eq!(int(run(src, "run", vec![])), 20);
}

#[test]
fn a_length_and_a_signed_number_can_be_compared_and_subtracted() {
    let src = r#"
let LIMIT = 3

fn over(xs: List[Int]) -> Bool:
    xs.len() > LIMIT

fn before_last(xs: List[Int], i: Int) -> Bool:
    xs.len() - 1 > i

fn spare(xs: List[Int]) -> Int:
    let n: Int = 2
    xs.len() - n

fn run() -> Int:
    var score = 0
    if over([1, 2, 3, 4]):
        score += 1
    if not over([1, 2]):
        score += 10
    if before_last([1, 2, 3], 1):
        score += 100
    score + spare([1, 2, 3, 4, 5])
"#;
    assert_eq!(int(run(src, "run", vec![])), 114);
}

#[test]
fn spawn_tells_on_error_when_the_task_fails() {
    let src = r#"
let seen: Map[Text, Text] = {}

async fn boom(id: Text):
    let nothing: Int? = None
    nothing.unwrap()

async fn fine(id: Text):
    seen.insert(id, "done")

fn note(message: Text):
    seen.insert("error", message)

fn run() -> Text:
    std.task.spawn(fine("a"), note)
    std.task.spawn(boom("b"), note)
    seen.get("a").unwrap_or("-") + "|" + seen.get("error").unwrap_or("none")
"#;
    let out = text(run(src, "run", vec![]));
    assert!(out.starts_with("done|"), "{out}");
    assert!(!out.ends_with("|none"), "{out}");
}

#[test]
fn a_cache_drops_the_least_recently_used_and_forgets_after_its_ttl() {
    let src = r#"
let cache = std.cache.new(2, 0.2)
let big = std.cache.new(1000, 60.0)

fn lru() -> Text:
    cache.clear()
    cache.set("a", 1)
    cache.set("b", 2)
    cache.get("a")
    cache.set("c", 3)
    let a = cache.get("a").unwrap_or(0)
    let b = cache.get("b").unwrap_or(0)
    let c = cache.get("c").unwrap_or(0)
    f"{a}{b}{c}|{cache.len()}"

fn ttl() -> Text:
    cache.clear()
    cache.set("k", 7)
    let fresh = cache.get("k").unwrap_or(0)
    std.time.sleep(300)
    f"{fresh}{cache.get("k").unwrap_or(0)}"

fn many() -> Int:
    for i in range(3000):
        big.set(f"key{i}", i)
    std.math.to_int(big.len())

fn removed() -> Text:
    big.clear()
    big.set("x", "v")
    let gone = big.remove("x").unwrap_or("none")
    let again = big.remove("x").unwrap_or("none")
    f"{gone}{again}{big.clear()}"

fn bad() -> Int:
    let c = std.cache.new(0, 5.0)
    0
"#;
    assert_eq!(text(run(src, "lru", vec![])), "103|2");
    assert_eq!(text(run(src, "ttl", vec![])), "70");
    // The shards each keep their share of the limit, so the whole stays at or under it.
    let n = int(run(src, "many", vec![]));
    assert!(n <= 1000 && n > 800, "{n}");
    assert_eq!(text(run(src, "removed", vec![])), "vnone0");
    let e = load(src).call("bad", vec![]).unwrap_err();
    assert!(e.to_string().contains("must be above 0"), "{e}");
}

#[test]
fn names_are_read_in_place_before_and_after_the_globals_are_ready() {
    let src = r#"
type Table:
    names: List[Text]
    scores: Map[Text, Int]

let NAMES: List[Text] = ["a", "b", "c"]
let TABLE = Table(names: ["x", "y"], scores: {"k": 7})
# Read while the globals are still being made.
let FIRST = NAMES[0]
let COUNT = std.math.to_int(NAMES.len()) + std.math.to_int(TABLE.names.len())
let SEVEN = TABLE.scores["k"]

fn run() -> Int:
    let local = TABLE
    var n = COUNT + SEVEN
    if FIRST == "a" and TABLE.names.contains("y") and local.scores.contains_key("k"):
        n += 100
    n + local.scores["k"] + std.math.to_int(local.names[1].len())
"#;
    // COUNT 5 + SEVEN 7 + 100 + 7 + 1
    assert_eq!(int(run(src, "run", vec![])), 120);
}

#[test]
fn a_pattern_is_compiled_once_and_used_many_times() {
    let src = r#"
fn run() -> Int:
    var hits = 0
    for i in range(2000):
        if std.text.matches("abc123", "^[a-z]+[0-9]+$"):
            hits += 1
        hits += std.math.to_int(std.text.find_all("a1 b2 c3", "[a-z][0-9]").len()) - 3
    hits
"#;
    assert_eq!(int(run(src, "run", vec![])), 2000);
}

#[test]
fn counters_can_have_labels() {
    let src = r#"
fn run() -> Int:
    std.metrics.count("lbl_requests_total", {"language": "en", "sentiment": "positive"})
    std.metrics.count("lbl_requests_total", {"sentiment": "positive", "language": "en"}, 4)
    std.metrics.count("lbl_requests_total", {"language": "pt", "sentiment": "a\"b"})
    std.metrics.count("lbl_plain")
    std.metrics.count("lbl_plain", 2)
    0

fn bad_name() -> Int:
    std.metrics.count("has space", {"a": "b"})
    0

fn bad_label() -> Int:
    std.metrics.count("lbl_x", {"1a": "b"})
    0

fn bad_plain_name() -> Int:
    std.metrics.count("cache hit")
    0
"#;
    run(src, "run", vec![]);
    let all = telvax_runtime::counters();
    let get = |k: &str| all.iter().find(|(n, _)| n == k).map(|(_, v)| *v);
    // Same labels in another order are the same counter.
    assert_eq!(
        get(r#"lbl_requests_total{language="en",sentiment="positive"}"#),
        Some(5)
    );
    assert_eq!(
        get(r#"lbl_requests_total{language="pt",sentiment="a\"b"}"#),
        Some(1)
    );
    assert_eq!(get("lbl_plain"), Some(3));
    let e = load(src).call("bad_name", vec![]).unwrap_err();
    assert!(e.to_string().contains("not a counter name"), "{e}");
    // A counter without labels is listed under its own name, so it needs a name Prometheus reads.
    let e = load(src).call("bad_plain_name", vec![]).unwrap_err();
    assert!(
        e.to_string().contains("`cache hit` is not a counter name"),
        "{e}"
    );
    let e = load(src).call("bad_label", vec![]).unwrap_err();
    assert!(e.to_string().contains("not a label name"), "{e}");
}

#[test]
fn a_gauge_holds_the_last_value() {
    let src = r#"
fn run() -> Int:
    std.metrics.set("gauge_cache_size", 10)
    std.metrics.set("gauge_cache_size", 7.5)
    std.metrics.set("gauge_jobs", 3, {"queue": "high"})
    std.metrics.set("gauge_jobs", 1, {"queue": "high"})
    std.metrics.set("gauge_jobs", 2, {"queue": "low"})
    0

fn bad_name() -> Int:
    std.metrics.set("has space", 1)
    0

fn bad_label() -> Int:
    std.metrics.set("gauge_x", 1, {"1a": "b"})
    0

fn set_a_counter() -> Int:
    std.metrics.count("gauge_is_a_counter")
    std.metrics.set("gauge_is_a_counter", 1)
    0

fn count_a_gauge() -> Int:
    std.metrics.set("gauge_is_a_gauge", 1)
    std.metrics.count("gauge_is_a_gauge")
    0

fn count_a_labelled_gauge() -> Int:
    std.metrics.set("gauge_is_a_labelled_gauge", 1, {"a": "b"})
    std.metrics.count("gauge_is_a_labelled_gauge", {"a": "c"})
    0
"#;
    run(src, "run", vec![]);
    let all = telvax_runtime::gauges();
    let get = |k: &str| all.iter().find(|(n, _)| n == k).map(|(_, v)| *v);
    // The last call wins, and an `Int` is read as a `Float`.
    assert_eq!(get("gauge_cache_size"), Some(7.5));
    assert_eq!(get(r#"gauge_jobs{queue="high"}"#), Some(1.0));
    assert_eq!(get(r#"gauge_jobs{queue="low"}"#), Some(2.0));

    let message = |f: &str| load(src).call(f, vec![]).unwrap_err().to_string();
    assert!(message("bad_name").contains("`has space` is not a gauge name"));
    assert!(message("bad_label").contains("not a label name"));
    // A name is a counter or a gauge, not both.
    assert!(
        message("set_a_counter")
            .contains("`gauge_is_a_counter` is a counter: use `std.metrics.count`")
    );
    assert!(
        message("count_a_gauge").contains("`gauge_is_a_gauge` is a gauge: use `std.metrics.set`")
    );
    assert!(message("count_a_labelled_gauge").contains("is a gauge"));
    // A refused call changes nothing.
    assert_eq!(get("gauge_is_a_counter"), None);
}

#[test]
fn a_gauge_name_has_a_limit_of_label_sets() {
    let src = r#"
fn run() -> Text:
    for i in range(1100):
        std.metrics.set("gauge_many", i, {"id": f"{i}"})
    "unreachable"
"#;
    let e = load(src).call("run", vec![]).unwrap_err();
    assert!(
        e.to_string()
            .contains("std.metrics.set: `gauge_many` has 1000 label sets"),
        "{e}"
    );
}

#[test]
fn a_counter_name_has_a_limit_of_label_sets() {
    let src = r#"
fn run() -> Text:
    for i in range(1100):
        std.metrics.count("lbl_many_total", {"id": f"{i}"})
    "unreachable"
"#;
    let e = load(src).call("run", vec![]).unwrap_err();
    assert!(e.to_string().contains("1000 label sets"), "{e}");
}

#[test]
fn case_conversion_keeps_ascii_and_unicode_right() {
    let src = r#"
fn run() -> Text:
    let words = ["abc", "ABC", "aBc", "ÀÉ", "àé", "ǅ", "straße"]
    var out = ""
    for w in words:
        out = out + w.to_lowercase() + "/" + w.to_uppercase() + " "
    out
"#;
    assert_eq!(
        text(run(src, "run", vec![])),
        "abc/ABC abc/ABC abc/ABC àé/ÀÉ àé/ÀÉ ǆ/Ǆ straße/STRASSE "
    );
}

#[test]
fn joined_text_is_the_same_short_or_long() {
    let src = r#"
fn run() -> Int:
    var long = ""
    for i in range(300):
        long = long + "é"
    let short = "ab" + "cd"
    let both = long + short
    std.math.to_int(both.len()) * 100 + std.math.to_int(short.len())
"#;
    // 300 characters of 2 bytes, then 4 bytes: 604 bytes; the short text has 4.
    assert_eq!(int(run(src, "run", vec![])), 604 * 100 + 4);
}

#[test]
fn contains_looks_at_the_list_in_place() {
    let src = r#"
fn run() -> Int:
    let xs = ["a", "b"]
    var n = 0
    if xs.contains("b"):
        n += 1
    xs.push("c")
    if xs.contains("c"):
        n += 10
    if not xs.contains("z"):
        n += 100
    n
"#;
    assert_eq!(int(run(src, "run", vec![])), 111);
}

#[test]
fn a_call_counts_once_for_a_mock_and_uses_its_arguments() {
    let src = r#"
fn double(x: Int) -> Int:
    x * 2

fn run() -> Int:
    double(double(3)) + double(1)

fn test_calls_are_counted_once():
    std.test.mock(double, x => 100 + x)
    assert_eq(run(), 203 + 101)
    assert_eq(std.test.calls(double), 3)
"#;
    let interp = load(src);
    assert_eq!(
        int(interp.call("run", vec![]).unwrap()),
        2 * (2 * 3) + 2,
        "without a mock"
    );
    interp
        .call("test_calls_are_counted_once", vec![])
        .unwrap_or_else(|e| panic!("runtime error: {e}"));
}

#[test]
fn comparing_and_joining_with_a_text_literal() {
    let src = r#"
type Box:
    label: Text

fn run() -> Int:
    let a = "never"
    let b = Box(label: "so")
    var n = 0
    if a == "never":
        n += 1
    if a != "never":
        n += 10
    if b.label == "so":
        n += 100
    if 5 == 5:
        n += 1000
    let joined = a + " " + b.label
    if joined == "never so":
        n += 10000
    n
"#;
    assert_eq!(int(run(src, "run", vec![])), 11101);
}

#[test]
fn a_value_stays_small() {
    // A `Value` is moved and copied at every step of the interpreter; keep it to a tag and two words.
    assert!(
        std::mem::size_of::<Value>() <= 24,
        "Value grew to {} bytes",
        std::mem::size_of::<Value>()
    );
}

/// Runs `run()` of `src` with compiled code and with the tree walker (a step limit turns compiled
/// code off) and gives both results.
fn both_ways(src: &str) -> (String, String) {
    let shown = |interp: &Interp| match interp.call("run", vec![]) {
        Ok(v) => format!("{v}"),
        Err(e) => format!("error: {e}"),
    };
    let compiled = load(src);
    let walked = load(src);
    walked.set_step_limit(u64::MAX - 1);
    (shown(&compiled), shown(&walked))
}

#[test]
fn compiled_code_and_the_tree_walker_agree() {
    let programs = [
        // Numbers, loops, early return and recursion.
        r#"
fn fib(n: Int) -> Int:
    if n < 2:
        return n
    fib(n - 1) + fib(n - 2)

fn first_over(limit: Int) -> Int:
    for i in range(1000):
        if i * i > limit:
            return i
    -1

fn run() -> Int:
    var total = 0
    var k = 0
    while k < 10:
        total += fib(k)
        k += 1
    total + first_over(500) * 1000
"#,
        // Structs, assignment through fields and indexes, methods on lists and maps.
        r#"
type Item:
    name: Text
    qty: Int
    price: Float

fn run() -> Text:
    var items = [Item(name: "a", qty: 1, price: 2.5), Item(name: "b", qty: 3, price: 1.25)]
    items[1].qty = items[1].qty + 4
    var stock: Map[Text, Int] = {"x": 1}
    stock["y"] = 5
    stock["x"] += 2
    var total = 0.0
    for it in items:
        total += it.price * 2.0 + 0.5
    f"{items[1].qty} {total} {stock.len()} {stock.get("x")} {stock.contains_key("z")}"
"#,
        // Option, Result, `?`, match with guards, enums and closures.
        r#"
enum Shape:
    Circle(Float)
    Square(Float)
    Empty

fn area(s: Shape) -> Float:
    match s:
        case Shape.Circle(r) if r > 10.0:
            -1.0
        case Shape.Circle(r):
            3.0 * r * r
        case Shape.Square(a):
            a * a
        case Shape.Empty:
            0.0

fn parse(s: Text) -> Result[Int, Text]:
    match s.to_int():
        case Some(n):
            Ok(n)
        case None:
            Err("bad " + s)

fn sum_two(a: Text, b: Text) -> Result[Int, Text]:
    let x = parse(a)?
    let y = parse(b)?
    Ok(x + y)

fn run() -> Text:
    let shapes = [Shape.Circle(2.0), Shape.Square(3.0), Shape.Empty, Shape.Circle(11.0)]
    let areas = shapes.map(s => area(s))
    let ok = sum_two("4", "5")
    let bad = sum_two("4", "x")
    f"{areas} {ok} {bad}"
"#,
        // Text: literals, joining, case, comparisons, f-string formats, indexing.
        r#"
fn run() -> Text:
    let words = ["Hello", "WORLD", "ünï", "abc"]
    var out = ""
    for w in words:
        if w == "abc":
            out = out + "!"
        elif w != "Hello":
            out = out + w.to_lowercase() + w.to_uppercase()
        else:
            out = out + w + " "
    f"{out} {words[0]} {words[-1]} {3.5:.2f} {out.len()}"
"#,
        // A failure: the same message either way.
        r#"
fn run() -> Int:
    let xs = [1, 2, 3]
    xs[5]
"#,
        r#"
fn run() -> Int:
    var n = 9223372036854775807
    n + 1
"#,
    ];
    for (i, src) in programs.iter().enumerate() {
        let (compiled, walked) = both_ways(src);
        assert_eq!(compiled, walked, "program {i}");
    }
}
