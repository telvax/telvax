use std::sync::Arc;

use serde_json::json;
use telvax_cluster::{ClusterExecutor, spawn};
use telvax_modules::Image;
use telvax_runtime::{Executor, Interp, Value};

const PROGRAM: &str = r#"
type Job:
    id: Int
    text: Text

type Done:
    id: Int
    words: u64
    shout: Text

fn square(x: Int) -> Int:
    x * x

fn heavy(n: Int) -> Int:
    var acc: Int = 0
    for i in range(n):
        acc += i * i % 7
    acc

fn process(j: Job) -> Done:
    Done(id: j.id, words: std.iter.count(std.text.split(j.text, " ")), shout: std.text.uppercase(j.text))

fn fails(x: Int) -> Int:
    100 / x

fn squares(v: List[Int]) -> List[Int]:
    std.dist.map(v, square)

fn crunch(v: List[Int]) -> List[Int]:
    std.dist.map(v, heavy)

fn jobs(v: List[Job]) -> List[Done]:
    std.dist.map(v, process)

fn risky(v: List[Int]) -> List[Int]:
    std.dist.map(v, fails)
"#;

fn image(src: &str) -> Image {
    Image {
        entry: "app.tvx".into(),
        files: vec![("app.tvx".into(), src.into())],
        deps: vec![],
        data: vec![],
    }
}

fn interp_with(workers: Vec<String>, token: Option<String>) -> Interp {
    let image = image(PROGRAM);
    let loaded = image.load();
    assert!(loaded.diagnostics.is_empty());
    let (hir, diags) = telvax_hir::lower(&loaded.program);
    assert!(
        diags.is_empty(),
        "{:?}",
        diags.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
    let interp = Interp::new(hir).unwrap();
    interp.set_executor(Arc::new(
        ClusterExecutor::new(workers, &image, token).unwrap(),
    ));
    interp
}

fn ints(v: Value) -> Vec<i64> {
    let Value::Vec(items) = v else {
        panic!("{v:?}")
    };
    items
        .snapshot()
        .into_iter()
        .map(|x| match x {
            Value::Int(i) => i,
            other => panic!("{other:?}"),
        })
        .collect()
}

fn range(n: i64) -> Value {
    Value::vec((0..n).map(Value::Int).collect())
}

fn health(addr: &str) -> serde_json::Value {
    let mut r = ureq::get(&format!("http://{addr}/health")).call().unwrap();
    r.body_mut().read_json().unwrap()
}

#[test]
fn work_is_spread_over_workers_and_results_keep_their_order() {
    let (a, b, c) = (
        spawn(None).unwrap(),
        spawn(None).unwrap(),
        spawn(None).unwrap(),
    );
    let interp = interp_with(vec![a.address(), b.address(), c.address()], None);
    // Each item takes long enough that one worker cannot drain the queue before the others start.
    let sizes = Value::vec((0..300).map(|i| Value::Int(5_000 + i)).collect());
    let out = ints(interp.call("crunch", vec![sizes.clone()]).unwrap());
    let heavy = |n: i64| (0..n).map(|i| i * i % 7).sum::<i64>();
    assert_eq!(out, (0..300).map(|i| heavy(5_000 + i)).collect::<Vec<_>>());
    let calls: Vec<u64> = [&a, &b, &c]
        .iter()
        .map(|w| health(&w.address())["calls"].as_u64().unwrap())
        .collect();
    assert_eq!(
        calls.iter().sum::<u64>(),
        300,
        "every item was computed exactly once: {calls:?}"
    );
    assert!(
        calls.iter().all(|&n| n > 0),
        "every worker took part: {calls:?}"
    );
}

#[test]
fn structs_travel_as_json_in_both_directions() {
    let w = spawn(None).unwrap();
    let interp = interp_with(vec![w.address()], None);
    let jobs = Value::vec(vec![
        interp
            .value_from_json(
                &telvax_hir::Ty::Struct("Job".into()),
                &json!({"id": 1, "text": "hello big world"}),
            )
            .unwrap(),
        interp
            .value_from_json(
                &telvax_hir::Ty::Struct("Job".into()),
                &json!({"id": 2, "text": "tiny"}),
            )
            .unwrap(),
    ]);
    let out = interp.call("jobs", vec![jobs]).unwrap();
    assert_eq!(
        out.to_json(),
        json!([{"id": 1, "words": 3, "shout": "HELLO BIG WORLD"}, {"id": 2, "words": 1, "shout": "TINY"}])
    );
}

#[test]
fn the_program_failing_on_a_worker_fails_the_map_with_its_message() {
    let (a, b) = (spawn(None).unwrap(), spawn(None).unwrap());
    let interp = interp_with(vec![a.address(), b.address()], None);
    let e = interp
        .call(
            "risky",
            vec![Value::vec(vec![
                Value::Int(5),
                Value::Int(0),
                Value::Int(2),
            ])],
        )
        .unwrap_err();
    assert!(e.message.contains("division by zero"), "{e}");
}

#[test]
fn a_worker_that_goes_away_does_not_lose_work() {
    let (mut a, b) = (spawn(None).unwrap(), spawn(None).unwrap());
    let (addr_a, addr_b) = (a.address(), b.address());
    let interp = Arc::new(interp_with(vec![addr_a.clone(), addr_b.clone()], None));
    let runner = interp.clone();
    let job = std::thread::spawn(move || {
        ints(
            runner
                .call("crunch", vec![range(2000).clone_values(40_000)])
                .unwrap(),
        )
    });
    // Take one machine away while the job is running.
    std::thread::sleep(std::time::Duration::from_millis(150));
    a.shutdown();
    let out = job.join().unwrap();
    assert_eq!(out.len(), 2000);
    let expected: i64 = (0..40_000i64).map(|i| (i * i) % 7).sum();
    assert!(
        out.iter().all(|&v| v == expected),
        "every result is complete and correct"
    );
    assert!(
        health(&addr_b)["calls"].as_u64().unwrap() > 0,
        "the surviving worker finished the job"
    );
}

trait CloneValues {
    fn clone_values(&self, with: i64) -> Value;
}

impl CloneValues for Value {
    /// A vector of `n` copies of `with`, where `n` is this vector's length.
    fn clone_values(&self, with: i64) -> Value {
        let Value::Vec(v) = self else { panic!() };
        Value::vec((0..v.len()).map(|_| Value::Int(with)).collect())
    }
}

#[test]
fn unreachable_workers_are_reported() {
    let interp = interp_with(vec!["127.0.0.1:1".into()], None);
    let e = interp.call("squares", vec![range(3)]).unwrap_err();
    assert!(e.message.contains("no worker is available"), "{e}");
}

#[test]
fn one_dead_worker_among_live_ones_is_skipped() {
    let live = spawn(None).unwrap();
    let interp = interp_with(vec!["127.0.0.1:1".into(), live.address()], None);
    assert_eq!(
        ints(interp.call("squares", vec![range(20)]).unwrap()),
        (0..20).map(|x| x * x).collect::<Vec<_>>()
    );
}

#[test]
fn tokens_protect_workers() {
    let w = spawn(Some("s3cret".into())).unwrap();
    let good = interp_with(vec![w.address()], Some("s3cret".into()));
    assert_eq!(
        ints(good.call("squares", vec![range(4)]).unwrap()),
        [0, 1, 4, 9]
    );
    let bad = interp_with(vec![w.address()], Some("wrong".into()));
    assert!(
        bad.call("squares", vec![range(4)])
            .unwrap_err()
            .message
            .contains("rejected the token")
    );
    let none = interp_with(vec![w.address()], None);
    assert!(
        none.call("squares", vec![range(4)])
            .unwrap_err()
            .message
            .contains("rejected the token")
    );
}

#[test]
fn workers_refuse_programs_that_do_not_compile() {
    let w = spawn(None).unwrap();
    let broken = image(
        r#"
fn f() -> i32:
    "nope"
"#,
    );
    let exec = ClusterExecutor::new(vec![w.address()], &broken, None).unwrap();
    let e = exec.map("f", vec![json!(1)]).unwrap_err();
    assert!(e.contains("does not type-check"), "{e}");
}

#[test]
fn programs_are_sent_once_and_reused() {
    let w = spawn(None).unwrap();
    let interp = interp_with(vec![w.address()], None);
    for _ in 0..3 {
        interp.call("squares", vec![range(5)]).unwrap();
    }
    assert_eq!(
        health(&w.address())["programs"],
        1,
        "the same image is stored once"
    );
}

#[test]
fn results_match_local_execution() {
    let w = spawn(None).unwrap();
    let remote = interp_with(vec![w.address()], None);
    let local = {
        let loaded = image(PROGRAM).load();
        Interp::new(telvax_hir::lower(&loaded.program).0).unwrap()
    };
    let items = range(50);
    assert_eq!(
        remote
            .call("crunch", vec![items.clone()])
            .unwrap()
            .to_json(),
        local.call("crunch", vec![items]).unwrap().to_json()
    );
}
