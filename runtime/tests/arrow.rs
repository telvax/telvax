use std::sync::Arc;

use serde_json::json;
use telvax_runtime::{Interp, TensorVal, Value};

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

fn float(v: Value) -> f64 {
    match v {
        Value::Float(x) => x,
        other => panic!("expected float, got {other:?}"),
    }
}

fn int(v: Value) -> i64 {
    match v {
        Value::Int(i) => i,
        other => panic!("expected int, got {other:?}"),
    }
}

fn tensor(v: Value) -> Arc<TensorVal> {
    match v {
        Value::Tensor(t) => t,
        other => panic!("expected tensor, got {other:?}"),
    }
}

fn f32s(shape: &[usize], data: &[f32]) -> Value {
    Value::Tensor(Arc::new(TensorVal::from_f32(shape.to_vec(), data.to_vec())))
}

/// A fresh path for every call. Tests run in parallel, and one that rewrote a shared file while
/// another was reading it made a SQL test see an empty table now and then.
fn scratch(name: &str) -> String {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!("telvax-arrow-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    dir.join(format!("{n}-{name}"))
        .to_string_lossy()
        .into_owned()
}

#[test]
fn elementwise_and_reductions() {
    let src = r#"
fn add(a: Tensor[f32, 3], b: Tensor[f32, 3]) -> Tensor[f32, 3]:
    std.tensor.add(a, b)

fn chain(a: Tensor[f32, 4]) -> Tensor[f32, 4]:
    a |> std.tensor.scale(2.0) |> std.tensor.relu() |> std.tensor.scale(0.5)

fn total(a: Tensor[f32, 4]) -> f32:
    std.tensor.sum(a)

fn avg(a: Tensor[f32, 4]) -> f32:
    std.tensor.mean(a)

fn best(a: Tensor[f32, 4]) -> u64:
    std.tensor.argmax(a)

fn dot(a: Tensor[f32, 3], b: Tensor[f32, 3]) -> f32:
    std.tensor.dot(a, b)
"#;
    let sum = tensor(run(
        src,
        "add",
        vec![
            f32s(&[3], &[1.0, 2.0, 3.0]),
            f32s(&[3], &[10.0, 20.0, 30.0]),
        ],
    ));
    assert_eq!(sum.to_f32_vec(), [11.0, 22.0, 33.0]);
    let relu = tensor(run(src, "chain", vec![f32s(&[4], &[-1.0, 2.0, -3.0, 4.0])]));
    assert_eq!(relu.to_f32_vec(), [0.0, 2.0, 0.0, 4.0]);
    let v = f32s(&[4], &[1.0, 5.0, 2.0, 0.0]);
    assert_eq!(float(run(src, "total", vec![v.clone()])), 8.0);
    assert_eq!(float(run(src, "avg", vec![v.clone()])), 2.0);
    assert_eq!(int(run(src, "best", vec![v])), 1);
    assert_eq!(
        float(run(
            src,
            "dot",
            vec![f32s(&[3], &[1.0, 2.0, 3.0]), f32s(&[3], &[4.0, 5.0, 6.0])]
        )),
        32.0
    );
}

#[test]
fn matmul_transpose_and_softmax() {
    let src = r#"
fn mm(a: Tensor[f32, 2, 3], b: Tensor[f32, 3, 2]) -> Tensor[f32, 2, 2]:
    std.tensor.matmul(a, b)

fn mv(a: Tensor[f32, 2, 3], v: Tensor[f32, 3]) -> Tensor[f32, 2]:
    std.tensor.matmul(a, v)

fn tr(a: Tensor[f32, 2, 3]) -> Tensor[f32, 3, 2]:
    std.tensor.transpose(a)

fn sm(a: Tensor[f32, 2, 3]) -> Tensor[f32, 2, 3]:
    std.tensor.softmax(a)
"#;
    let a = f32s(&[2, 3], &[1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
    let b = f32s(&[3, 2], &[7.0, 8.0, 9.0, 10.0, 11.0, 12.0]);
    let c = tensor(run(src, "mm", vec![a.clone(), b]));
    assert_eq!(
        (c.shape.clone(), c.to_f32_vec()),
        (vec![2, 2], vec![58.0, 64.0, 139.0, 154.0])
    );
    let v = tensor(run(
        src,
        "mv",
        vec![a.clone(), f32s(&[3], &[1.0, 0.0, -1.0])],
    ));
    assert_eq!(
        (v.shape.clone(), v.to_f32_vec()),
        (vec![2], vec![-2.0, -2.0])
    );
    let t = tensor(run(src, "tr", vec![a.clone()]));
    assert_eq!(
        (t.shape.clone(), t.to_f32_vec()),
        (vec![3, 2], vec![1.0, 4.0, 2.0, 5.0, 3.0, 6.0])
    );
    let s = tensor(run(src, "sm", vec![a]));
    let rows = s.to_f32_vec();
    assert!(
        (rows[..3].iter().sum::<f32>() - 1.0).abs() < 1e-6
            && (rows[3..].iter().sum::<f32>() - 1.0).abs() < 1e-6
    );
    assert!(rows[2] > rows[1] && rows[1] > rows[0]);
}

#[test]
fn constructors_use_the_declared_type() {
    let src = r#"
fn z() -> Tensor[f32, 2, 2]:
    std.tensor.zeros()

fn fv(v: List[f32]) -> Tensor[f32, 2, 2]:
    std.tensor.from_vec(v)

fn rs(a: Tensor[f32, 2, 3]) -> Tensor[f32, 3, 2]:
    std.tensor.reshape(a)

fn back(a: Tensor[f32, 3]) -> List[f32]:
    std.tensor.to_vec(a)

fn ints() -> Tensor[Int, 3]:
    std.tensor.from_vec([1, 2, 3])
"#;
    assert_eq!(tensor(run(src, "z", vec![])).to_f32_vec(), [0.0; 4]);
    let v = Value::vec(vec![
        Value::Float(1.0),
        Value::Float(2.0),
        Value::Float(3.0),
        Value::Float(4.0),
    ]);
    let t = tensor(run(src, "fv", vec![v]));
    assert_eq!(
        (t.shape.clone(), t.to_f32_vec()),
        (vec![2, 2], vec![1.0, 2.0, 3.0, 4.0])
    );
    let i = load(src);
    let r = tensor(
        i.call("rs", vec![f32s(&[2, 3], &[1.0, 2.0, 3.0, 4.0, 5.0, 6.0])])
            .unwrap(),
    );
    assert_eq!(r.shape, [3, 2]);
    let wrong = Value::vec(vec![Value::Float(1.0)]);
    assert!(
        i.call("fv", vec![wrong])
            .unwrap_err()
            .message
            .contains("needs 4 elements")
    );
    assert!(
        matches!(i.call("back", vec![f32s(&[3], &[1.0, 2.0, 3.0])]).unwrap(), Value::Vec(v) if v.len() == 3)
    );
    assert_eq!(
        tensor(i.call("ints", vec![]).unwrap()).to_f64_vec(),
        [1.0, 2.0, 3.0]
    );
}

#[test]
fn runtime_shape_checks_catch_dynamic_mismatches() {
    let src = r#"
fn add(a: Tensor[f32, N], b: Tensor[f32, N]) -> Tensor[f32, N]:
    std.tensor.add(a, b)
"#;
    let i = load(src);
    let e = i
        .call(
            "add",
            vec![f32s(&[2], &[1.0, 2.0]), f32s(&[3], &[1.0, 2.0, 3.0])],
        )
        .unwrap_err();
    assert!(e.message.contains("shapes differ"), "{e}");
}

const TABLE_SRC: &str = r#"
type Row:
    id: Int
    score: f32
    name: Text
    ok: Bool

fn load(path: Text) -> Table[Row]:
    std.table.read_csv(path)

fn scores(t: Table[Row]) -> Tensor[f32, N]:
    t.score

fn names(t: Table[Row]) -> List[Text]:
    t.name

fn count(t: Table[Row]) -> u64:
    t.len()

fn second(t: Table[Row]) -> Row:
    t[1]

fn total(t: Table[Row]) -> f32:
    var s = 0.0
    for r in t:
        s += r.score
    s

fn top(t: Table[Row]) -> Table[Row]:
    t |> std.table.sql("SELECT * FROM t WHERE score > 1.5 ORDER BY score DESC")

fn good(t: Table[Row]) -> Table[Row]:
    t |> std.table.filter(r => r.ok)

fn avg(t: Table[Row]) -> f32:
    t.score |> std.tensor.mean()

fn save_csv(t: Table[Row], path: Text):
    std.table.write_csv(t, path)

fn save_pq(t: Table[Row], path: Text):
    std.table.write_parquet(t, path)

fn load_pq(path: Text) -> Table[Row]:
    std.table.read_parquet(path)

fn rebuild(t: Table[Row]) -> Table[Row]:
    std.table.from_rows(std.table.to_rows(t))
"#;

fn csv() -> String {
    let path = scratch("rows.csv");
    std::fs::write(
        &path,
        "id,score,name,ok\n1,1.0,ann,true\n2,2.5,bob,false\n3,3.5,cy,true\n",
    )
    .unwrap();
    path
}

#[test]
fn csv_ingestion_columns_and_rows() {
    let i = load(TABLE_SRC);
    let t = i.call("load", vec![Value::str(csv())]).unwrap();
    assert_eq!(int(i.call("count", vec![t.clone()]).unwrap()), 3);
    let scores = tensor(i.call("scores", vec![t.clone()]).unwrap());
    assert_eq!(scores.to_f32_vec(), [1.0, 2.5, 3.5]);
    let Value::Vec(names) = i.call("names", vec![t.clone()]).unwrap() else {
        panic!()
    };
    assert_eq!(
        names
            .snapshot()
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>(),
        ["ann", "bob", "cy"]
    );
    assert_eq!(
        i.call("second", vec![t.clone()]).unwrap().to_string(),
        "Row(id=2, score=2.5, name=\"bob\", ok=False)"
    );
    assert_eq!(float(i.call("total", vec![t.clone()]).unwrap()), 7.0);
    assert_eq!(
        float(i.call("avg", vec![t]).unwrap()),
        f64::from((7.0_f64 / 3.0) as f32)
    );
}

#[test]
fn numeric_columns_share_the_arrow_buffer() {
    let i = load(TABLE_SRC);
    let t = i.call("load", vec![Value::str(csv())]).unwrap();
    let Value::Table(table) = &t else { panic!() };
    let col = tensor(i.call("scores", vec![t.clone()]).unwrap());
    let idx = table.batch.schema().index_of("score").unwrap();
    assert_eq!(
        col.f32_slice().unwrap().as_ptr(),
        table.batch.column(idx).to_data().buffers()[0].as_ptr() as *const f32,
        "reading a column must not copy it"
    );
}

#[test]
fn sql_filter_and_round_trips() {
    let i = load(TABLE_SRC);
    let t = i.call("load", vec![Value::str(csv())]).unwrap();
    let top = i.call("top", vec![t.clone()]).unwrap();
    assert_eq!(
        top.to_json(),
        json!([
            {"id": 3, "score": 3.5, "name": "cy", "ok": true},
            {"id": 2, "score": 2.5, "name": "bob", "ok": false},
        ])
    );
    let good = i.call("good", vec![t.clone()]).unwrap();
    assert_eq!(int(i.call("count", vec![good]).unwrap()), 2);

    let out = scratch("out.csv");
    i.call("save_csv", vec![t.clone(), Value::str(out.as_str())])
        .unwrap();
    let again = i.call("load", vec![Value::str(out)]).unwrap();
    assert!(again.equals(&t), "CSV round trip");

    let pq = scratch("out.parquet");
    i.call("save_pq", vec![t.clone(), Value::str(pq.as_str())])
        .unwrap();
    let back = i.call("load_pq", vec![Value::str(pq)]).unwrap();
    assert!(back.equals(&t), "Parquet round trip");

    let rebuilt = i.call("rebuild", vec![t.clone()]).unwrap();
    assert!(rebuilt.equals(&t), "rows round trip");
}

#[test]
fn tables_report_bad_input() {
    let i = load(TABLE_SRC);
    let missing = i
        .call("load", vec![Value::str("/nonexistent/x.csv")])
        .unwrap_err();
    assert!(missing.message.contains("cannot open"), "{missing}");
    let bad = scratch("bad.csv");
    std::fs::write(&bad, "id,score,name,ok\nx,1.0,ann,true\n").unwrap();
    assert!(i.call("load", vec![Value::str(bad)]).is_err());
    let t = i.call("load", vec![Value::str(csv())]).unwrap();
    let sql = i.call("top", vec![t]).unwrap();
    assert!(matches!(sql, Value::Table(_)));
}

#[test]
fn tables_and_tensors_travel_as_json() {
    let src = r#"
type Row:
    id: Int
    score: f32

type In:
    rows: Table[Row]
    w: Tensor[f32, 2]

type Out:
    n: u64
    weighted: Tensor[f32, N]
    rows: Table[Row]

@endpoint(POST, "/score")
fn score(p: In) -> Result[Out, Text]:
    Ok(Out(n: p.rows.len(), weighted: p.rows.score |> std.tensor.scale(2.0), rows: p.rows))
"#;
    let i = load(src);
    let body = json!({"rows": [{"id": 1, "score": 0.5}, {"id": 2, "score": 1.5}], "w": [1.0, 2.0]});
    let out = i.call_endpoint("score", &body).unwrap();
    assert_eq!(out["n"], 2);
    assert_eq!(out["weighted"], json!([1.0, 3.0]));
    assert_eq!(out["rows"], body["rows"]);
}

#[test]
fn int8_quantized_matmul_stays_close_to_float() {
    let src = r#"
fn mm(a: Tensor[f32, 4, 8], b: Tensor[f32, 8, 3]) -> Tensor[f32, 4, 3]:
    std.tensor.matmul(a, b)
"#;
    let a: Vec<f32> = (0..32)
        .map(|i| ((i * 37 % 19) as f32 - 9.0) / 4.0)
        .collect();
    let b: Vec<f32> = (0..24)
        .map(|i| ((i * 53 % 23) as f32 - 11.0) / 7.0)
        .collect();
    let (ta, tb) = (f32s(&[4, 8], &a), f32s(&[8, 3], &b));

    let exact = load(src);
    let fp = tensor(exact.call("mm", vec![ta.clone(), tb.clone()]).unwrap()).to_f32_vec();

    let quant = load(src);
    quant.set_quantization(telvax_hir::Quantization::Int8);
    let q = tensor(quant.call("mm", vec![ta, tb]).unwrap()).to_f32_vec();

    assert_ne!(fp, q, "quantization must actually change the arithmetic");
    let scale = fp.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    let worst = fp
        .iter()
        .zip(&q)
        .map(|(x, y)| (x - y).abs())
        .fold(0.0f32, f32::max);
    assert!(
        worst / scale < 0.03,
        "relative error {} too large",
        worst / scale
    );
}

#[test]
fn int8_quantization_handles_zeros_and_exact_cases() {
    let src = r#"
fn mm(a: Tensor[f32, 2, 2], b: Tensor[f32, 2, 2]) -> Tensor[f32, 2, 2]:
    std.tensor.matmul(a, b)
"#;
    let i = load(src);
    i.set_quantization(telvax_hir::Quantization::Int8);
    let zero = f32s(&[2, 2], &[0.0; 4]);
    assert_eq!(
        tensor(i.call("mm", vec![zero.clone(), zero]).unwrap()).to_f32_vec(),
        [0.0; 4]
    );
    // Values on the int8 grid multiply exactly: scales are max/127 and entries are multiples.
    let a = f32s(&[2, 2], &[127.0, -127.0, 0.0, 127.0]);
    let id = f32s(&[2, 2], &[1.0, 0.0, 0.0, 1.0]);
    assert_eq!(
        tensor(i.call("mm", vec![a, id]).unwrap()).to_f32_vec(),
        [127.0, -127.0, 0.0, 127.0]
    );
}

#[test]
fn per_channel_quantization_is_more_accurate_when_columns_differ() {
    // One column is a thousand times larger than the others: a single per-tensor scale wastes
    // almost every level on the small columns.
    let src = r#"
fn mm(a: Tensor[f32, 6, 16], b: Tensor[f32, 16, 4]) -> Tensor[f32, 6, 4]:
    std.tensor.matmul(a, b)
"#;
    let a: Vec<f32> = (0..96)
        .map(|i| ((i * 37 % 29) as f32 - 14.0) / 5.0)
        .collect();
    let b: Vec<f32> = (0..64)
        .map(|i| {
            let column_scale = [1.0, 0.01, 1000.0, 0.1][i % 4];
            ((i * 53 % 31) as f32 - 15.0) / 15.0 * column_scale
        })
        .collect();
    let (ta, tb) = (f32s(&[6, 16], &a), f32s(&[16, 4], &b));
    let run_with = |q: telvax_hir::Quantization| {
        let interp = load(src);
        interp.set_quantization(q);
        tensor(interp.call("mm", vec![ta.clone(), tb.clone()]).unwrap()).to_f32_vec()
    };
    let exact = run_with(telvax_hir::Quantization::None);
    // Relative error per column, worst case over the rows.
    let worst = |got: &[f32]| {
        (0..4)
            .map(|j| {
                let scale = (0..6)
                    .map(|i| exact[i * 4 + j].abs())
                    .fold(0.0f32, f32::max);
                (0..6)
                    .map(|i| (exact[i * 4 + j] - got[i * 4 + j]).abs() / scale)
                    .fold(0.0f32, f32::max)
            })
            .fold(0.0f32, f32::max)
    };
    let per_tensor = worst(&run_with(telvax_hir::Quantization::Int8));
    let per_channel = worst(&run_with(telvax_hir::Quantization::Int8Channel));
    let int4 = worst(&run_with(telvax_hir::Quantization::Int4Channel));
    println!(
        "worst relative error: per-tensor {per_tensor:.4}, per-channel {per_channel:.4}, int4 {int4:.4}"
    );
    assert!(
        per_tensor > 0.05,
        "per-tensor must lose the small columns: {per_tensor}"
    );
    assert!(
        per_channel < 0.05,
        "per-channel stays accurate: {per_channel}"
    );
    assert!(per_channel < per_tensor / 3.0);
    // Four bits cost accuracy, but each column still has its own scale.
    assert!(int4 > per_channel && int4 < 0.5, "int4 error {int4}");
}

#[test]
fn tensor_methods_and_operators_match_the_function_forms() {
    let src = r#"
fn methods(x: Tensor[f32, 2, 3], w: Tensor[f32, 3, 2]) -> Tensor[f32, 2, 2]:
    x.matmul(w).relu()

fn functions(x: Tensor[f32, 2, 3], w: Tensor[f32, 3, 2]) -> Tensor[f32, 2, 2]:
    std.tensor.relu(std.tensor.matmul(x, w))

fn chain(x: Tensor[f32, 2, 3], w: Tensor[f32, 3, 2]) -> Tensor[f32, 2, 2]:
    x.matmul(w).sigmoid().scale(2.0)

fn total(x: Tensor[f32, 2, 3]) -> f32:
    x.sum() + x.mean()

fn ops(a: Tensor[f32, 4], b: Tensor[f32, 4]) -> Tensor[f32, 4]:
    (a + b) * a - b / a

fn ops_by_hand(a: Tensor[f32, 4], b: Tensor[f32, 4]) -> Tensor[f32, 4]:
    std.tensor.sub(std.tensor.mul(std.tensor.add(a, b), a), std.tensor.div(b, a))

fn scaled(a: Tensor[f32, 4]) -> Tensor[f32, 4]:
    a * 3.0 + a * 0.5

fn flipped(a: Tensor[f32, 4]) -> Tensor[f32, 4]:
    2.0 * a
"#;
    let x = f32s(&[2, 3], &[1.0, -2.0, 3.0, -4.0, 5.0, -6.0]);
    let w = f32s(&[3, 2], &[0.5, -1.0, 1.5, 2.0, -0.25, 1.0]);
    let get = |f: &str, args: Vec<Value>| tensor(run(src, f, args)).to_f32_vec();
    assert_eq!(
        get("methods", vec![x.clone(), w.clone()]),
        get("functions", vec![x.clone(), w.clone()])
    );
    assert_eq!(get("chain", vec![x.clone(), w.clone()]).len(), 4);
    assert!(float(run(src, "total", vec![x])) != 0.0);

    let a = f32s(&[4], &[1.0, 2.0, 4.0, 8.0]);
    let b = f32s(&[4], &[2.0, 4.0, 8.0, 16.0]);
    assert_eq!(
        get("ops", vec![a.clone(), b.clone()]),
        get("ops_by_hand", vec![a.clone(), b])
    );
    assert_eq!(get("scaled", vec![a.clone()]), vec![3.5, 7.0, 14.0, 28.0]);
    assert_eq!(get("flipped", vec![a]), vec![2.0, 4.0, 8.0, 16.0]);
}

#[test]
fn inline_row_types_prelude_and_table_methods() {
    let src = r#"
type Sale:
    id: Int
    region: Text
    units: Int

fn totals(t: Table[Sale]) -> Table[{region: Text, units: Int}]:
    t.sql("SELECT region, SUM(units) AS units FROM t GROUP BY region ORDER BY region")

fn count(t: Table[{region: Text, units: Int}]) -> u64:
    t.len()

fn run_it(t: Table[Sale]) -> u64:
    count(totals(t))

fn shout():
    print("hello")
"#;
    let interp = load(src);
    let out = interp
        .call_json(
            "run_it",
            &[json!([
                {"id": 1, "region": "n", "units": 4},
                {"id": 2, "region": "s", "units": 5},
                {"id": 3, "region": "n", "units": 6},
            ])],
        )
        .unwrap();
    assert_eq!(out, json!(2));
    interp.call("shout", vec![]).unwrap();
}

const TABLE_OPS_SRC: &str = r#"
type Row:
    id: Int
    score: f32
    name: Text
    ok: Bool

fn load(path: Text) -> Table[Row]:
    std.table.read_csv(path)

fn ids(t: Table[Row]) -> List[Int]:
    var out: List[Int] = []
    for r in t:
        out.push(r.id)
    out

fn first(t: Table[Row], n: Int) -> List[Int]:
    ids(t.head(n))

fn last(t: Table[Row], n: Int) -> List[Int]:
    ids(t.tail(n))

fn by_score_desc(t: Table[Row]) -> List[Int]:
    ids(t.sort_by_desc("score"))

fn by_name(t: Table[Row]) -> List[Int]:
    ids(t.sort_by("name"))

fn by_missing(t: Table[Row]) -> List[Int]:
    ids(t.sort_by("nope"))

fn doubled(t: Table[Row]) -> List[Int]:
    ids(t.concat(t))

fn good(t: Table[Row]) -> List[Int]:
    ids(t.filter(r => r.ok))
"#;

fn ids(v: Value) -> Vec<i64> {
    let Value::Vec(v) = v else { panic!("{v:?}") };
    v.snapshot().into_iter().map(int).collect()
}

#[test]
fn table_head_tail_sort_concat_filter() {
    let i = load(TABLE_OPS_SRC);
    let t = i.call("load", vec![Value::str(csv())]).unwrap();
    let call = |f: &str, extra: Vec<Value>| {
        let mut args = vec![t.clone()];
        args.extend(extra);
        i.call(f, args)
    };
    assert_eq!(ids(call("first", vec![Value::Int(2)]).unwrap()), [1, 2]);
    assert_eq!(ids(call("first", vec![Value::Int(99)]).unwrap()), [1, 2, 3]);
    assert_eq!(ids(call("last", vec![Value::Int(2)]).unwrap()), [2, 3]);
    assert_eq!(
        ids(call("last", vec![Value::Int(0)]).unwrap()),
        [] as [i64; 0]
    );
    assert_eq!(ids(call("by_score_desc", vec![]).unwrap()), [3, 2, 1]);
    assert_eq!(ids(call("by_name", vec![]).unwrap()), [1, 2, 3]);
    assert_eq!(ids(call("doubled", vec![]).unwrap()), [1, 2, 3, 1, 2, 3]);
    assert_eq!(ids(call("good", vec![]).unwrap()), [1, 3]);
    let e = call("by_missing", vec![]).unwrap_err().to_string();
    assert!(
        e.contains("no column `nope`") && e.contains("id, score, name, ok"),
        "{e}"
    );
    let e = call("first", vec![Value::Int(-1)]).unwrap_err().to_string();
    assert!(e.contains("zero or more"), "{e}");
}

const SELECT_SRC: &str = r#"
type Row:
    id: Int
    score: f32
    name: Text
    ok: Bool

fn load(path: Text) -> Table[Row]:
    std.table.read_csv(path)

fn names(t: Table[Row]) -> List[Text]:
    var out: List[Text] = []
    for r in t.select(["name", "id"]):
        out.push(r.name)
    out

fn ids_after_select(t: Table[Row]) -> List[Int]:
    var out: List[Int] = []
    for r in t.select(["id", "score"]).sort_by_desc("score"):
        out.push(r.id)
    out

fn width(t: Table[Row]) -> u64:
    t.select(["ok"]).len()
"#;

#[test]
fn table_select_keeps_only_the_named_columns() {
    let i = load(SELECT_SRC);
    let t = i.call("load", vec![Value::str(csv())]).unwrap();
    let names = i.call("names", vec![t.clone()]).unwrap();
    let Value::Vec(v) = names else {
        panic!("{names:?}")
    };
    let got: Vec<String> = v.snapshot().iter().map(|x| format!("{x:?}")).collect();
    assert!(got[0].contains("ann") && got[2].contains("cy"), "{got:?}");
    assert_eq!(
        ids(i.call("ids_after_select", vec![t.clone()]).unwrap()),
        [3, 2, 1]
    );
    assert_eq!(int(i.call("width", vec![t]).unwrap()), 3);
}

const NULLABLE_SRC: &str = r#"
type Reading:
    id: Int
    temp: f64?
    note: Text?

fn load(path: Text) -> Table[Reading]:
    std.table.read_csv(path)

fn load_pq(path: Text) -> Table[Reading]:
    std.table.read_parquet(path)

fn save_pq(t: Table[Reading], path: Text):
    std.table.write_parquet(t, path)

fn temp_or(t: Table[Reading], i: Int) -> f64:
    t[i].temp.unwrap_or(-1.0)

fn has_note(t: Table[Reading], i: Int) -> Bool:
    t[i].note.is_some()

fn temps(t: Table[Reading]) -> List[f64?]:
    t.temp

fn warm(t: Table[Reading]) -> Table[Reading]:
    t |> std.table.sql("SELECT * FROM t WHERE temp > 20.0")

fn build() -> Table[Reading]:
    std.table.from_rows([
        Reading(id: 1, temp: Some(21.5), note: None),
        Reading(id: 2, temp: None, note: Some("broken")),
    ])
"#;

fn readings() -> String {
    let path = scratch("readings.csv");
    std::fs::write(&path, "id,temp,note\n1,21.5,\n2,,broken\n3,18.0,ok\n").unwrap();
    path
}

#[test]
fn nullable_columns_hold_nulls_as_options() {
    let i = load(NULLABLE_SRC);
    let t = i.call("load", vec![Value::str(readings())]).unwrap();
    let at = |f: &str, n: i64| i.call(f, vec![t.clone(), Value::Int(n)]).unwrap();
    assert_eq!(float(at("temp_or", 0)), 21.5);
    assert_eq!(float(at("temp_or", 1)), -1.0);
    assert!(matches!(at("has_note", 0), Value::Bool(false)));
    assert!(matches!(at("has_note", 1), Value::Bool(true)));
    let Value::Vec(col) = i.call("temps", vec![t.clone()]).unwrap() else {
        panic!()
    };
    assert_eq!(
        col.snapshot()
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>(),
        ["Some(21.5)", "None", "Some(18.0)"]
    );

    // Parquet keeps the nulls, and SQL treats them as NULL.
    let pq = scratch("readings.parquet");
    i.call("save_pq", vec![t.clone(), Value::str(pq.clone())])
        .unwrap();
    let back = i.call("load_pq", vec![Value::str(pq)]).unwrap();
    assert_eq!(
        float(i.call("temp_or", vec![back, Value::Int(1)]).unwrap()),
        -1.0
    );
    let warm = i.call("warm", vec![t]).unwrap();
    let Value::Table(w) = warm else { panic!() };
    assert_eq!(w.rows(), 1);
}

#[test]
fn nullable_columns_from_rows_and_required_columns() {
    let i = load(NULLABLE_SRC);
    let t = i.call("build", vec![]).unwrap();
    let at = |f: &str, n: i64| i.call(f, vec![t.clone(), Value::Int(n)]).unwrap();
    assert_eq!(float(at("temp_or", 0)), 21.5);
    assert_eq!(float(at("temp_or", 1)), -1.0);
    assert!(matches!(at("has_note", 1), Value::Bool(true)));

    // A column that is not declared `T?` still rejects empty values and says how to allow them.
    let i = load(TABLE_SRC);
    let path = scratch("holes.csv");
    std::fs::write(&path, "id,score,name,ok\n1,,ann,true\n").unwrap();
    let e = i
        .call("load", vec![Value::str(path)])
        .map(|_| ())
        .unwrap_err()
        .to_string();
    assert!(
        e.contains("column `score` contains empty values") && e.contains("f32?"),
        "{e}"
    );
}

const TIME_SRC: &str = r#"
type Event:
    id: Int
    day: Date
    at: Timestamp
    seen: Date?

fn load(path: Text) -> Table[Event]:
    std.table.read_csv(path)

fn save_pq(t: Table[Event], path: Text):
    std.table.write_parquet(t, path)

fn load_pq(path: Text) -> Table[Event]:
    std.table.read_parquet(path)

fn day_of(t: Table[Event], i: Int) -> Text:
    t[i].day

fn at_of(t: Table[Event], i: Int) -> Text:
    t[i].at

fn seen_or(t: Table[Event], i: Int) -> Text:
    t[i].seen.unwrap_or("never")

fn days(t: Table[Event]) -> List[Date]:
    t.day

fn recent(t: Table[Event]) -> Table[Event]:
    t |> std.table.sql("SELECT * FROM t WHERE day >= DATE '2026-02-01' ORDER BY day")

fn newest_first(t: Table[Event]) -> Table[Event]:
    t.sort_by_desc("at")

fn build() -> Table[Event]:
    std.table.from_rows([
        Event(id: 1, day: "2026-10-17", at: "2026-10-17T16:53:03Z", seen: None),
        Event(id: 2, day: "2026-10-18", at: "2026-10-18T09:00:00+02:00", seen: Some("2026-10-19")),
    ])

fn bad() -> Table[Event]:
    std.table.from_rows([Event(id: 1, day: "17/10/2026", at: "2026-10-17T16:53:03Z", seen: None)])
"#;

fn events() -> String {
    let path = scratch("events.csv");
    std::fs::write(
        &path,
        "id,day,at,seen\n1,2026-01-15,2026-01-15T08:30:00Z,\n2,2026-03-02,2026-03-02T23:59:59.5Z,2026-03-03\n",
    )
    .unwrap();
    path
}

#[test]
fn date_and_timestamp_columns() {
    let i = load(TIME_SRC);
    let t = i.call("load", vec![Value::str(events())]).unwrap();
    let text = |f: &str, t: &Value, n: i64| {
        i.call(f, vec![t.clone(), Value::Int(n)])
            .unwrap()
            .to_string()
    };
    assert_eq!(text("day_of", &t, 1), "2026-03-02");
    assert_eq!(text("at_of", &t, 0), "2026-01-15T08:30:00Z");
    assert_eq!(text("at_of", &t, 1), "2026-03-02T23:59:59.500Z");
    assert_eq!(text("seen_or", &t, 0), "never");
    assert_eq!(text("seen_or", &t, 1), "2026-03-03");

    // The Arrow types are real temporal types, so Parquet and SQL see dates, not strings.
    let Value::Table(raw) = &t else { panic!() };
    assert_eq!(
        raw.batch
            .schema()
            .field_with_name("day")
            .unwrap()
            .data_type(),
        &arrow::datatypes::DataType::Date32
    );
    let pq = scratch("events.parquet");
    i.call("save_pq", vec![t.clone(), Value::str(pq.clone())])
        .unwrap();
    let back = i.call("load_pq", vec![Value::str(pq)]).unwrap();
    assert_eq!(text("at_of", &back, 1), "2026-03-02T23:59:59.500Z");
    let Value::Table(r) = i.call("recent", vec![t.clone()]).unwrap() else {
        panic!()
    };
    assert_eq!(r.rows(), 1);
    let Value::Table(n) = i.call("newest_first", vec![t]).unwrap() else {
        panic!()
    };
    assert!(n.row(0).unwrap().to_string().contains("2026-03-02"));
}

#[test]
fn date_and_timestamp_from_rows_and_errors() {
    let i = load(TIME_SRC);
    let t = i.call("build", vec![]).unwrap();
    let text = |f: &str, n: i64| {
        i.call(f, vec![t.clone(), Value::Int(n)])
            .unwrap()
            .to_string()
    };
    assert_eq!(text("day_of", 1), "2026-10-18");
    // An offset is converted to UTC.
    assert_eq!(text("at_of", 1), "2026-10-18T07:00:00Z");
    assert_eq!(text("seen_or", 1), "2026-10-19");
    let e = i.call("bad", vec![]).unwrap_err().to_string();
    assert!(e.contains("not a Date") && e.contains("YYYY-MM-DD"), "{e}");
}

const IPC_SRC: &str = r#"
type Row:
    id: Int
    score: f32
    name: Text
    ok: Bool

fn load(path: Text) -> Table[Row]:
    read_csv(path)

fn save(t: Table[Row], path: Text):
    std.table.write_ipc(t, path)

fn open(path: Text) -> Table[Row]:
    read_ipc(path)

fn scores(t: Table[Row]) -> Tensor[f32, N]:
    t.score
"#;

#[test]
fn arrow_ipc_round_trip() {
    let i = load(IPC_SRC);
    let t = i.call("load", vec![Value::str(csv())]).unwrap();
    let path = scratch("rows.arrow");
    i.call("save", vec![t.clone(), Value::str(path.clone())])
        .unwrap();
    let back = i.call("open", vec![Value::str(path)]).unwrap();
    let (Value::Table(a), Value::Table(b)) = (&back, &t) else {
        panic!()
    };
    assert_eq!(a.batch.columns(), b.batch.columns());
    assert_eq!(
        tensor(i.call("scores", vec![back]).unwrap()).to_f32_vec(),
        [1.0, 2.5, 3.5]
    );
}

#[test]
fn arrow_ipc_reads_dictionary_columns_and_reports_bad_files() {
    use arrow::array::{BooleanArray, DictionaryArray, Float32Array, Int64Array};
    use arrow::datatypes::Int32Type;
    use arrow::record_batch::RecordBatch;

    // What pandas / Polars write for a categorical column.
    let batch = RecordBatch::try_from_iter([
        ("id", Arc::new(Int64Array::from(vec![1, 2])) as _),
        ("score", Arc::new(Float32Array::from(vec![0.5, 1.5])) as _),
        (
            "name",
            Arc::new(DictionaryArray::<Int32Type>::from_iter(["a", "b"])) as _,
        ),
        ("ok", Arc::new(BooleanArray::from(vec![true, false])) as _),
    ])
    .unwrap();
    let path = scratch("dict.arrow");
    let mut w = arrow::ipc::writer::FileWriter::try_new(
        std::fs::File::create(&path).unwrap(),
        &batch.schema(),
    )
    .unwrap();
    w.write(&batch).unwrap();
    w.finish().unwrap();
    drop(w);
    let i = load(IPC_SRC);
    let Value::Table(t) = i.call("open", vec![Value::str(path)]).unwrap() else {
        panic!()
    };
    assert_eq!(
        t.row(1).unwrap().to_string(),
        "Row(id=2, score=1.5, name=\"b\", ok=False)"
    );

    let junk = scratch("junk.arrow");
    std::fs::write(&junk, b"not an arrow file at all").unwrap();
    let e = i
        .call("open", vec![Value::str(junk)])
        .unwrap_err()
        .to_string();
    assert!(e.contains("Arrow IPC"), "{e}");
    let short = scratch("short.arrow");
    std::fs::write(&short, b"abc").unwrap();
    let e = i
        .call("open", vec![Value::str(short)])
        .unwrap_err()
        .to_string();
    assert!(e.contains("too short"), "{e}");
    let e = i
        .call("open", vec![Value::str(scratch("missing.arrow"))])
        .unwrap_err()
        .to_string();
    assert!(e.contains("cannot open"), "{e}");
}

/// The columns point into the file mapping: bytes changed in the file show up in the table
/// without reading it again. A copying reader would still show the old value.
#[cfg(unix)]
#[test]
fn arrow_ipc_columns_are_memory_mapped() {
    use std::io::{Seek, SeekFrom, Write};
    let i = load(IPC_SRC);
    let t = i.call("load", vec![Value::str(csv())]).unwrap();
    let path = scratch("mapped.arrow");
    i.call("save", vec![t, Value::str(path.clone())]).unwrap();
    let back = i.call("open", vec![Value::str(path.clone())]).unwrap();

    let bytes = std::fs::read(&path).unwrap();
    let needle = 2.5f32.to_le_bytes();
    let at = bytes
        .windows(4)
        .position(|w| w == needle)
        .expect("2.5 in file");
    let mut f = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
    f.seek(SeekFrom::Start(at as u64)).unwrap();
    f.write_all(&9.5f32.to_le_bytes()).unwrap();
    f.sync_all().unwrap();

    assert_eq!(
        tensor(i.call("scores", vec![back]).unwrap()).to_f32_vec(),
        [1.0, 9.5, 3.5]
    );
}

#[cfg(not(feature = "flight"))]
#[test]
fn flight_without_the_feature_says_how_to_enable_it() {
    let i = load(
        r#"
type Row:
    id: Int

fn fetch() -> Table[Row]:
    std.table.read_flight("grpc://localhost:8815", "t")
"#,
    );
    let e = i.call("fetch", vec![]).map(|_| ()).unwrap_err().to_string();
    assert!(e.contains("--features flight"), "{e}");
}

const BATCH_SRC: &str = r#"
type Row:
    id: Int
    score: f32
    name: Text
    ok: Bool

fn csv_parts(path: Text, n: Int) -> List[Table[Row]]:
    std.table.read_csv_batches(path, n)

fn pq_parts(path: Text, n: Int) -> List[Table[Row]]:
    std.table.read_parquet_batches(path, n)

fn ipc_parts(path: Text, n: Int) -> List[Table[Row]]:
    std.table.read_ipc_batches(path, n)

fn load(path: Text) -> Table[Row]:
    read_csv(path)

fn save_pq(t: Table[Row], path: Text):
    std.table.write_parquet(t, path)

fn save_ipc(t: Table[Row], path: Text):
    std.table.write_ipc(t, path)

fn parts(t: Table[Row], n: Int) -> List[Table[Row]]:
    t.split(n)

fn total(t: List[Table[Row]]) -> f32:
    var s = 0.0
    for part in t:
        for r in part:
            s += r.score
    s
"#;

fn lens(v: Value) -> Vec<usize> {
    let Value::Vec(parts) = v else { panic!() };
    parts
        .snapshot()
        .iter()
        .map(|p| match p {
            Value::Table(t) => t.rows(),
            other => panic!("expected a table, got {other:?}"),
        })
        .collect()
}

#[test]
fn tables_are_read_and_split_in_batches() {
    let i = load(BATCH_SRC);
    let t = i.call("load", vec![Value::str(csv())]).unwrap();
    let pq = scratch("batches.parquet");
    let ipc = scratch("batches.arrow");
    i.call("save_pq", vec![t.clone(), Value::str(pq.clone())])
        .unwrap();
    i.call("save_ipc", vec![t.clone(), Value::str(ipc.clone())])
        .unwrap();
    let two = Value::Int(2);

    assert_eq!(
        lens(i.call("parts", vec![t.clone(), two.clone()]).unwrap()),
        [2, 1]
    );
    assert_eq!(
        lens(i.call("parts", vec![t.clone(), Value::Int(10)]).unwrap()),
        [3]
    );
    for (f, path) in [("csv_parts", csv()), ("pq_parts", pq), ("ipc_parts", ipc)] {
        let parts = i
            .call(f, vec![Value::str(path.clone()), two.clone()])
            .unwrap();
        assert_eq!(lens(parts.clone()), [2, 1], "{f}");
        assert_eq!(float(i.call("total", vec![parts]).unwrap()), 7.0, "{f}");
        assert_eq!(
            lens(i.call(f, vec![Value::str(path), Value::Int(1)]).unwrap()),
            [1, 1, 1],
            "{f}"
        );
    }

    // A split shares the table's buffers.
    let Value::Table(whole) = &t else { panic!() };
    let Value::Vec(parts) = i.call("parts", vec![t.clone(), two.clone()]).unwrap() else {
        panic!()
    };
    let Value::Table(first) = parts.snapshot()[0].clone() else {
        panic!()
    };
    assert_eq!(
        first.batch.column(1).to_data().buffers()[0].as_ptr(),
        whole.batch.column(1).to_data().buffers()[0].as_ptr()
    );

    let e = i
        .call("parts", vec![t, Value::Int(0)])
        .unwrap_err()
        .to_string();
    assert!(e.contains("at least 1"), "{e}");
}
