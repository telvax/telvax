//! `std.ml` on a small table (`data/sales.csv`). The expected numbers are scikit-learn 1.5.2's for the same data:
//! `ColumnTransformer` of `StandardScaler` and `OneHotEncoder(handle_unknown="ignore")` fitted on the first 30 rows,
//! `accuracy_score`, `precision/recall/f1_score` with each average, `confusion_matrix`, `classification_report` (the
//! text is the same byte for byte), the regression errors, `roc_auc_score`, the sizes of `train_test_split` and of
//! `KFold(3, shuffle=True)`.

use telvax_runtime::{Interp, Value};

const PROGRAM: &str = r#"type Sale:
    region: Text
    units: Int
    price: Float
    sold_at: Timestamp
    churned: Bool
    label: Text

fn six(xs: List[Float]) -> Text:
    xs |> std.iter.map(v => f"{v:.6f}") |> std.text.join(",")

fn six32(xs: List[f32]) -> Text:
    xs |> std.iter.map(v => f"{v:.6f}") |> std.text.join(",")

fn guess(price: Float) -> Text:
    if price > 60.0:
        "hi"
    elif price > 30.0:
        "mid"
    else:
        "lo"

fn report(path: Text) -> Text:
    let out: List[Text] = []
    let t: Table[Sale] = std.table.read_csv(path)
    let (train, test) = std.ml.split_by_time(t, 0.25)
    out.push(f"time split {train.len()} {test.len()}")
    let f = std.ml.Features.fit(train, "churned", ["units", "price", "sold_at"], ["region"])
    out.push(f"{f.width()} {f.names()}")
    out.push(six32(std.tensor.to_vec(f.transform(test))))
    let y = test.label
    let p = std.tensor.to_vec(test.price) |> std.iter.map(guess)
    out.push(f"acc {std.ml.accuracy(y, p):.6f}")
    for avg in ["macro", "micro", "weighted"]:
        out.push(f"{avg} {std.ml.precision(y, p, avg):.6f} {std.ml.recall(y, p, avg):.6f} {std.ml.f1(y, p, avg):.6f}")
    out.push(f"{std.ml.confusion_matrix(y, p)}")
    out.push(std.ml.classification_report(y, p))
    let units = std.tensor.to_vec(test.units) |> std.iter.map(u => std.math.to_float(u) * 3.0)
    out.push(f"reg {std.ml.mae(test.price, units):.6f} {std.ml.mse(test.price, units):.6f} {std.ml.rmse(test.price, units):.6f} {std.ml.r2(test.price, units):.6f}")
    out.push(f"auc {std.ml.roc_auc(t.churned, t.price):.6f}")
    let (a, b) = std.ml.split(t, 0.25, 42)
    out.push(f"split {a.len()} {b.len()}")
    let (sa, sb) = std.ml.split_stratified(t, "label", 0.25, 7)
    out.push(f"strat {sa.len()} {sb.len()}")
    let folds = std.ml.k_fold(10, 3, 1)
    out.push(f"folds {folds |> std.iter.map(x => x.1.len())}")
    out.push(f"take {std.tensor.to_vec(std.table.take(t, [3, 0]).units)}")
    let s = std.ml.Scaler.fit([1.0, 2.0, 3.0, 4.0])
    out.push(f"scaler {six(s.apply([1.0, 4.0]))} {six(s.inverse(s.apply([2.5])))}")
    let e = std.ml.LabelEncoder.fit(["b", "a", "c", "a"])
    out.push(f"enc {e.classes()} {e.encode(["c", "a"])} {e.decode([1])}")
    out.push(f"time {std.time.weekday(t[0].sold_at)} {std.time.hour(t[3].sold_at)} {std.time.epoch(t[0].sold_at)}")
    out |> std.text.join("
")

fn target_as_feature(path: Text) -> Int:
    let t: Table[Sale] = std.table.read_csv(path)
    std.ml.Features.fit(t, "price", ["units", "price"], []).width()

fn unseen_label() -> List[Int]:
    std.ml.LabelEncoder.fit(["a", "b"]).encode(["c"])

fn bad_share(path: Text) -> u64:
    let t: Table[Sale] = std.table.read_csv(path)
    let (a, b) = std.ml.split(t, 1.5, 1)
    a.len()

fn one_class() -> Float:
    std.ml.roc_auc([True, True], [0.1, 0.2])

fn bad_average() -> Float:
    std.ml.f1(["a"], ["a"], "mean")

fn text_as_number(path: Text) -> Int:
    let t: Table[Sale] = std.table.read_csv(path)
    std.ml.Features.fit(t, "churned", ["region"], []).width()
"#;

fn load() -> Interp {
    let (ast, perrs) = telvax_parser::parse(PROGRAM);
    assert!(perrs.is_empty(), "parse: {perrs:?}");
    let (hir, diags) = telvax_hir::lower(&ast.unwrap());
    assert!(
        diags.is_empty(),
        "{:?}",
        diags.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
    Interp::new(hir).unwrap()
}

fn csv() -> Value {
    Value::str(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/sales.csv"))
}

#[test]
fn the_toolkit_gives_scikit_learns_numbers() {
    let got = load().call("report", vec![csv()]).unwrap().to_string();
    let want = r#"time split 30 10
7 ["units", "price", "sold_at", "region=east", "region=north", "region=south", "region=west"]
-0.129893,1.725852,-0.204725,0.000000,0.000000,1.000000,0.000000,-0.018556,0.834565,0.166476,0.000000,0.000000,0.000000,1.000000,-0.018556,-0.255100,0.562178,1.000000,0.000000,0.000000,0.000000,0.538127,1.783796,0.945120,0.000000,0.000000,1.000000,0.000000,-0.018556,-0.757682,1.340822,0.000000,1.000000,0.000000,0.000000,-1.465932,1.205968,1.724273,1.000000,0.000000,0.000000,0.000000,0.649464,-1.667480,-1.457194,0.000000,0.000000,0.000000,0.000000,0.649464,-0.725088,-1.062002,0.000000,0.000000,0.000000,0.000000,-0.686576,1.859043,-0.715302,0.000000,0.000000,0.000000,0.000000,1.317483,-1.754395,-0.319600,0.000000,0.000000,0.000000,0.000000
acc 0.600000
macro 0.588889 0.611111 0.573810
micro 0.600000 0.600000 0.600000
weighted 0.596667 0.600000 0.573571
(["hi", "lo", "mid"], [[3, 0, 0], [1, 1, 1], [1, 1, 2]])
              precision    recall  f1-score   support

          hi       0.60      1.00      0.75         3
          lo       0.50      0.33      0.40         3
         mid       0.67      0.50      0.57         4

    accuracy                           0.60        10
   macro avg       0.59      0.61      0.57        10
weighted avg       0.60      0.60      0.57        10

reg 42.936000 2396.342800 48.952454 -1.122023
auc 0.515625
split 30 10
strat 30 10
folds [4, 3, 3]
take [18, 24]
scaler -1.341641,1.341641 2.500000
enc ["a", "b", "c"] [2, 0] ["b"]
time 5 21 1768003200.0"#;
    assert_eq!(got, want);
}

#[test]
fn mistakes_are_errors_that_say_what_to_do() {
    let i = load();
    let e = |f: &str, args: Vec<Value>| i.call(f, args).unwrap_err().to_string();
    let m = e("target_as_feature", vec![csv()]);
    assert!(
        m.contains("the target `price` is also given as a feature"),
        "{m}"
    );
    let m = e("unseen_label", vec![]);
    assert!(
        m.contains("`c` was not in the training values; the classes are a, b"),
        "{m}"
    );
    let m = e("bad_share", vec![csv()]);
    assert!(
        m.contains("test_size is 1.5; it is the share of rows for the test, above 0 and below 1"),
        "{m}"
    );
    let m = e("one_class", vec![]);
    assert!(m.contains("need both True and False rows"), "{m}");
    let m = e("bad_average", vec![]);
    assert!(m.contains("it is `macro`, `micro` or `weighted`"), "{m}");
    let m = e("text_as_number", vec![csv()]);
    assert!(
        m.contains("the column `region` is text; give it as a categorical feature"),
        "{m}"
    );
}

const MODEL_PROGRAM: &str = r#"import std.ml.Features
import std.ml.LabelEncoder
import std.sparse.Sparse

type Sale:
    region: Text
    units: Int
    price: Float
    sold_at: Timestamp
    churned: Bool
    label: Text

type Model:
    features: Features
    weights: Tensor[f32, 7, 1]
    classes: LabelEncoder
    words: Sparse
    name: Text
    tags: List[Text]
    pair: (Int, Float)
    extra: Option[Int]

type Other:
    name: Text

fn scores(m: Model, t: Table[Sale]) -> List[f32]:
    std.tensor.to_vec(std.tensor.matmul(m.features.transform(t), m.weights))

fn roundtrip(data: Text, path: Text) -> Text:
    let t: Table[Sale] = std.table.read_csv(data)
    let (train, test) = std.ml.split_by_time(t, 0.25)
    let w: Tensor[f32, 7, 1] = std.tensor.from_vec([0.5, -1.25, 2.0, 1.0, 0.0, -0.5, 3.0])
    let m = Model(
        features: std.ml.Features.fit(train, "churned", ["units", "price", "sold_at"], ["region"]),
        weights: w,
        classes: std.ml.LabelEncoder.fit(train.label),
        words: std.sparse.from_triplets(2, 3, [(0, 2, 1.5), (1, 0, -2.0)]),
        name: "sales v1",
        tags: ["a", "b"],
        pair: (3, 0.25),
        extra: None,
    )
    std.model.save(m, path).unwrap()
    let loaded: Result[Model, Text] = std.model.load(path)
    let back = loaded.unwrap()
    let same = scores(m, test) == scores(back, test)
    f"{same} {back.name} {back.tags} {back.pair} {back.extra} {back.classes.classes()} {back.words.row(0)} {back.features.names()} {std.model.describe(path).unwrap()}"

fn wrong_type(path: Text) -> Text:
    let r: Result[Other, Text] = std.model.load(path)
    match r:
        case Ok(o):
            o.name
        case Err(e):
            e

fn bytes_roundtrip(path: Text) -> Text:
    let b = std.fs.read_bytes(path).unwrap()
    let r: Result[Model, Text] = std.model.from_bytes(b.slice(0, 50))
    match r:
        case Ok(m):
            m.name
        case Err(e):
            e


fn save_other(path: Text) -> Text:
    std.model.save(Other(name: "x"), path).unwrap()
    std.model.describe(path).unwrap()
"#;

/// The same type names with other fields: a program that changed since the model was saved.
const CHANGED_PROGRAM: &str = r#"
type Other:
    name: Text
    size: Int


type Model:
    weights: Tensor[f32, 8, 1]


fn load_other(path: Text) -> Text:
    let r: Result[Other, Text] = std.model.load(path)
    match r:
        case Ok(o):
            o.name
        case Err(e):
            e


fn load_model(path: Text) -> Text:
    let r: Result[Model, Text] = std.model.load(path)
    match r:
        case Ok(m):
            "loaded"
        case Err(e):
            e
"#;

fn interp(src: &str) -> Interp {
    let (ast, perrs) = telvax_parser::parse(src);
    assert!(perrs.is_empty(), "parse: {perrs:?}");
    let (hir, diags) = telvax_hir::lower(&ast.unwrap());
    assert!(
        diags.is_empty(),
        "{:?}",
        diags.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
    Interp::new(hir).unwrap()
}

fn scratch(name: &str) -> String {
    let dir = std::env::temp_dir().join(format!("telvax-model-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name).display().to_string()
}

/// A struct of fitted steps, weights, a sparse matrix and plain values is saved and loaded back as the same type, and
/// gives the same predictions. The file is safetensors: Python's `safetensors` 0.4.5 reads `weights` as a `float32`
/// array of shape (7, 1), and the sparse parts as `words.indptr`, `words.indices` and `words.data`.
#[test]
fn a_model_is_saved_and_loaded_as_its_type() {
    let path = scratch("model.safetensors");
    let i = interp(MODEL_PROGRAM);
    let got = i
        .call("roundtrip", vec![csv(), Value::str(path.as_str())])
        .unwrap()
        .to_string();
    assert_eq!(
        got,
        r#"True sales v1 ["a", "b"] (3, 0.25) None ["hi", "lo", "mid"] [(2, 1.5)] ["units", "price", "sold_at", "region=east", "region=north", "region=south", "region=west"] Model { classes: LabelEncoder, extra: Option[?], features: Features, name: Text, pair: (Int, Float), tags: List[Text], weights: Tensor[f32, 7, 1], words: Sparse }"#
    );
    let bytes = std::fs::read(&path).unwrap();
    let n = u64::from_le_bytes(bytes[..8].try_into().unwrap()) as usize;
    assert_eq!(
        (8 + n) % 8,
        0,
        "the data starts at a multiple of 8, as safetensors pads it"
    );
    let head: serde_json::Value = serde_json::from_slice(&bytes[8..8 + n]).unwrap();
    assert_eq!(head["weights"]["dtype"], "F32");
    assert_eq!(head["weights"]["shape"], serde_json::json!([7, 1]));
    assert_eq!(head["__metadata__"]["format"], "telvax-model");

    // Another type, a cut file, a newer format, a struct that changed, damaged tensor bytes.
    let e = i
        .call("wrong_type", vec![Value::str(path.as_str())])
        .unwrap()
        .to_string();
    assert!(
        e.contains("in the file, and the program expects `Other`"),
        "{e}"
    );
    let e = i
        .call("bytes_roundtrip", vec![Value::str(path.as_str())])
        .unwrap()
        .to_string();
    assert!(
        e.contains("the header length is wrong (the file is damaged or cut short)"),
        "{e}"
    );
    let newer = scratch("newer.safetensors");
    let mut changed_version = bytes.clone();
    let at = changed_version
        .windows(13)
        .position(|w| w == b"\"version\":\"1\"")
        .unwrap();
    changed_version[at + 11] = b'7';
    std::fs::write(&newer, &changed_version).unwrap();
    let e = i
        .call("wrong_type", vec![Value::str(newer.as_str())])
        .unwrap()
        .to_string();
    assert!(
        e.contains("format version 7, written by a newer Telvax; this one reads version 1"),
        "{e}"
    );
    let other = scratch("other.safetensors");
    i.call("save_other", vec![Value::str(other.as_str())])
        .unwrap();
    let changed = interp(CHANGED_PROGRAM);
    let e = changed
        .call("load_other", vec![Value::str(other.as_str())])
        .unwrap()
        .to_string();
    assert!(
        e.contains(
            "the file's `Other` has no field `size`; it was saved by another version of the program"
        ),
        "{e}"
    );
    let e = changed
        .call("load_model", vec![Value::str(path.as_str())])
        .unwrap()
        .to_string();
    assert!(
        e.contains("has a field `classes` the program's `Model` does not"),
        "{e}"
    );
    let damaged = scratch("damaged.safetensors");
    let mut cut = bytes.clone();
    cut.truncate(bytes.len() - 4);
    std::fs::write(&damaged, &cut).unwrap();
    let e = i
        .call("wrong_type", vec![Value::str(damaged.as_str())])
        .unwrap()
        .to_string();
    assert!(
        e.contains("do not match its shape (the file is damaged)"),
        "{e}"
    );
}
