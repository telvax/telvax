//! `std.model`: saves a value (a model's weights, its fitted preprocessing, any struct of them) to a file and loads it
//! back as the type the program names. The file is safetensors: an 8-byte little-endian length, a JSON header, then the
//! raw bytes of the tensors. Telvax keeps the shape of the value in the header's `__metadata__`. Loading parses JSON
//! and copies bytes; nothing in the file is ever run.

use std::collections::BTreeMap;
use std::sync::Arc;

use arrow::array::{AsArray, Int64Array};
use arrow::datatypes::{DataType, Int64Type};
use serde_json::{Value as Json, json};
use telvax_hir::{Dim, Prim, Ty};

use crate::Interp;
use crate::ml::{EncoderVal, FeaturesVal, ScalerVal};
use crate::sparse::SparseVal;
use crate::value::{Native, StructVal, TensorVal, Value};

/// The `format` in the metadata, and the version this Telvax writes and reads.
const FORMAT: &str = "telvax-model";
const VERSION: u32 = 1;
/// A header larger than this is refused before it is read.
const MAX_HEADER: u64 = 100 << 20;

/// What `save` builds: the tree of the value, and the tensors it refers to by name.
#[derive(Default)]
struct Writer {
    tensors: Vec<(String, &'static str, Vec<usize>, Vec<u8>)>,
}

impl Writer {
    /// A tensor named after where it is in the value (`weights`, `layers[0].bias`), so other tools can find it.
    fn tensor(
        &mut self,
        name: String,
        dtype: &'static str,
        shape: Vec<usize>,
        bytes: Vec<u8>,
    ) -> String {
        self.tensors.push((name.clone(), dtype, shape, bytes));
        name
    }

    fn f32s(&mut self, name: String, shape: Vec<usize>, xs: &[f32]) -> String {
        self.tensor(
            name,
            "F32",
            shape,
            xs.iter().flat_map(|x| x.to_le_bytes()).collect(),
        )
    }

    fn i64s(&mut self, name: String, xs: &[i64]) -> String {
        self.tensor(
            name,
            "I64",
            vec![xs.len()],
            xs.iter().flat_map(|x| x.to_le_bytes()).collect(),
        )
    }

    fn value(&mut self, v: &Value, at: &str) -> Result<Json, String> {
        let child = |name: &str| {
            if at.is_empty() {
                name.to_string()
            } else {
                format!("{at}.{name}")
            }
        };
        Ok(match v {
            Value::Unit => json!({ "unit": null }),
            Value::Bool(b) => json!({ "bool": b }),
            Value::Int(i) => json!({ "int": i }),
            Value::Float(x) => json!({ "float": float(*x) }),
            Value::Str(s) => json!({ "text": &**s }),
            Value::Vec(items) => {
                let items: Vec<Json> = items
                    .snapshot()
                    .iter()
                    .enumerate()
                    .map(|(i, x)| self.value(x, &format!("{at}[{i}]")))
                    .collect::<Result<_, _>>()?;
                json!({ "list": items })
            }
            Value::Tuple(items) => {
                let items: Vec<Json> = items
                    .iter()
                    .enumerate()
                    .map(|(i, x)| self.value(x, &child(&i.to_string())))
                    .collect::<Result<_, _>>()?;
                json!({ "tuple": items })
            }
            Value::Enum(e) if e.name == "Option" => match e.fields.first() {
                Some(x) => json!({ "some": self.value(x, at)? }),
                None => json!({ "none": null }),
            },
            Value::Struct(s) => {
                let mut fields = serde_json::Map::new();
                for (name, x) in &s.fields {
                    fields.insert(name.clone(), self.value(x, &child(name))?);
                }
                json!({ "struct": readable(&s.name), "fields": fields })
            }
            Value::Tensor(t) => {
                let (dtype, bytes): (&'static str, Vec<u8>) = match t.dtype() {
                    DataType::Float32 => (
                        "F32",
                        t.to_f32_vec()
                            .iter()
                            .flat_map(|x| x.to_le_bytes())
                            .collect(),
                    ),
                    DataType::Float64 => (
                        "F64",
                        t.to_f64_vec()
                            .iter()
                            .flat_map(|x| x.to_le_bytes())
                            .collect(),
                    ),
                    DataType::Int64 => (
                        "I64",
                        t.array
                            .as_primitive::<Int64Type>()
                            .values()
                            .iter()
                            .flat_map(|x| x.to_le_bytes())
                            .collect(),
                    ),
                    other => {
                        return Err(format!(
                            "a tensor of {other} cannot be saved yet; f32, Float and Int can"
                        ));
                    }
                };
                {
                    let name = if at.is_empty() {
                        "value".to_string()
                    } else {
                        at.to_string()
                    };
                    json!({ "tensor": self.tensor(name, dtype, t.shape.clone(), bytes) })
                }
            }
            Value::Native(Native::Scaler(s)) => {
                json!({ "scaler": { "mean": float(s.mean), "std": float(s.std) } })
            }
            Value::Native(Native::LabelEncoder(e)) => {
                json!({ "label_encoder": { "classes": e.classes } })
            }
            Value::Native(Native::Features(f)) => json!({ "features": {
                "schema": f.schema,
                "target": f.target,
                "numeric": f.numeric.iter().map(|(n, s)| json!([n, float(s.mean), float(s.std)])).collect::<Vec<_>>(),
                "categorical": f.categorical.iter().map(|(n, c)| json!([n, c])).collect::<Vec<_>>(),
            } }),
            Value::Native(Native::Sparse(m)) => {
                let indptr: Vec<i64> = m.indptr.iter().map(|x| *x as i64).collect();
                let indices: Vec<i64> = m.indices.iter().map(|x| i64::from(*x)).collect();
                json!({ "sparse": {
                    "rows": m.rows,
                    "cols": m.cols,
                    "indptr": self.i64s(child("indptr"), &indptr),
                    "indices": self.i64s(child("indices"), &indices),
                    "data": self.f32s(child("data"), vec![m.data.len()], &m.data),
                } })
            }
            other => {
                return Err(format!(
                    "a {} cannot be saved; a model is made of numbers, texts, lists, tuples, options, structs, tensors, \
                     sparse matrices and the fitted steps of std.ml",
                    other.type_name()
                ));
            }
        })
    }
}

/// A name as the program writes it: the names of other modules (`crate__model__SalesPredictor`) become
/// `model.SalesPredictor`, and those of a dependency `pkg.module.Type`, as the diagnostics show them.
fn readable(name: &str) -> String {
    name.replace("crate__", "").replace("__", ".")
}

/// The last part of a readable name: `SalesPredictor` of `model.SalesPredictor`.
fn short_name(name: &str) -> &str {
    name.rsplit('.').next().unwrap_or(name)
}

/// A float as JSON: a number, or a text for the values JSON has no number for.
fn float(x: f64) -> Json {
    if x.is_finite() {
        json!(x)
    } else {
        json!(x.to_string())
    }
}

fn unfloat(j: &Json) -> Option<f64> {
    match j {
        Json::Number(n) => n.as_f64(),
        Json::String(s) => s.parse().ok(),
        _ => None,
    }
}

/// The type of a saved value, as the program would write it: what `describe` shows and what a mismatch names.
fn type_text(node: &Json, tensors: &BTreeMap<String, (String, Vec<usize>)>) -> String {
    let Some((kind, body)) = node
        .as_object()
        .and_then(|o| o.iter().find(|(k, _)| *k != "fields"))
    else {
        return "?".into();
    };
    match kind.as_str() {
        "unit" => "()".into(),
        "bool" => "Bool".into(),
        "int" => "Int".into(),
        "float" => "Float".into(),
        "text" => "Text".into(),
        "list" => format!(
            "List[{}]",
            body.as_array()
                .and_then(|a| a.first())
                .map_or("?".into(), |x| type_text(x, tensors))
        ),
        "tuple" => format!(
            "({})",
            body.as_array()
                .map(|a| a
                    .iter()
                    .map(|x| type_text(x, tensors))
                    .collect::<Vec<_>>()
                    .join(", "))
                .unwrap_or_default()
        ),
        "some" => format!("Option[{}]", type_text(body, tensors)),
        "none" => "Option[?]".into(),
        "struct" => {
            let fields = node["fields"]
                .as_object()
                .map(|f| {
                    f.iter()
                        .map(|(n, x)| format!("{n}: {}", type_text(x, tensors)))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            format!(
                "{} {{ {} }}",
                body.as_str().unwrap_or("?"),
                fields.join(", ")
            )
        }
        "tensor" => match body.as_str().and_then(|n| tensors.get(n)) {
            Some((dtype, shape)) => {
                let elem = match dtype.as_str() {
                    "F32" => "f32",
                    "F64" => "Float",
                    _ => "Int",
                };
                let dims: Vec<String> = shape.iter().map(ToString::to_string).collect();
                format!("Tensor[{elem}, {}]", dims.join(", "))
            }
            None => "Tensor".into(),
        },
        "scaler" => "Scaler".into(),
        "label_encoder" => "LabelEncoder".into(),
        "features" => "Features".into(),
        "sparse" => "Sparse".into(),
        other => other.to_string(),
    }
}

/// The bytes of a model file holding `v`.
pub(crate) fn to_bytes(v: &Value) -> Result<Vec<u8>, String> {
    let mut w = Writer::default();
    let tree = w.value(v, "")?;
    let shapes: BTreeMap<String, (String, Vec<usize>)> = w
        .tensors
        .iter()
        .map(|(n, d, s, _)| (n.clone(), ((*d).to_string(), s.clone())))
        .collect();
    let mut header = serde_json::Map::new();
    header.insert(
        "__metadata__".into(),
        json!({
            "format": FORMAT,
            "version": VERSION.to_string(),
            "type": type_text(&tree, &shapes),
            "value": tree.to_string(),
        }),
    );
    let mut data = Vec::new();
    for (name, dtype, shape, bytes) in &w.tensors {
        let start = data.len();
        data.extend_from_slice(bytes);
        header.insert(
            name.clone(),
            json!({ "dtype": dtype, "shape": shape, "data_offsets": [start, data.len()] }),
        );
    }
    let mut head = Json::Object(header).to_string().into_bytes();
    // safetensors pads the header with spaces so the data starts at a multiple of 8.
    while (head.len() + 8) % 8 != 0 {
        head.push(b' ');
    }
    let mut out = (head.len() as u64).to_le_bytes().to_vec();
    out.extend_from_slice(&head);
    out.extend_from_slice(&data);
    Ok(out)
}

/// A file read back: the header and where the data starts.
struct File<'a> {
    meta: serde_json::Map<String, Json>,
    tensors: BTreeMap<String, (String, Vec<usize>, &'a [u8])>,
}

fn open(bytes: &[u8]) -> Result<File<'_>, String> {
    let bad = |why: &str| format!("not a Telvax model file: {why}");
    if bytes.len() < 8 {
        return Err(bad("it is too short"));
    }
    let n = u64::from_le_bytes(bytes[..8].try_into().expect("8 bytes"));
    if n > MAX_HEADER || 8 + n > bytes.len() as u64 {
        return Err(bad(
            "the header length is wrong (the file is damaged or cut short)",
        ));
    }
    let head: Json = serde_json::from_slice(&bytes[8..8 + n as usize])
        .map_err(|_| bad("the header is not JSON"))?;
    let data = &bytes[8 + n as usize..];
    let Json::Object(mut head) = head else {
        return Err(bad("the header is not an object"));
    };
    let meta = match head.remove("__metadata__") {
        Some(Json::Object(m)) => m,
        _ => {
            return Err(bad(
                "it has no Telvax metadata (a safetensors file from elsewhere?)",
            ));
        }
    };
    if meta.get("format").and_then(Json::as_str) != Some(FORMAT) {
        return Err(bad("its format is not `telvax-model`"));
    }
    let version: u32 = meta
        .get("version")
        .and_then(Json::as_str)
        .and_then(|v| v.parse().ok())
        .ok_or_else(|| bad("it has no version"))?;
    if version > VERSION {
        return Err(format!(
            "the model file is of format version {version}, written by a newer Telvax; this one reads version {VERSION}"
        ));
    }
    let mut tensors = BTreeMap::new();
    for (name, t) in head {
        let dtype = t["dtype"].as_str().unwrap_or("").to_string();
        let shape: Vec<usize> = t["shape"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_u64().map(|x| x as usize))
                    .collect()
            })
            .unwrap_or_default();
        let (Some(a), Some(b)) = (t["data_offsets"][0].as_u64(), t["data_offsets"][1].as_u64())
        else {
            return Err(bad(&format!("the tensor `{name}` has no offsets")));
        };
        let size = match dtype.as_str() {
            "F32" => 4,
            "F64" | "I64" => 8,
            other => return Err(bad(&format!("the tensor `{name}` is of type `{other}`"))),
        };
        let count = shape.iter().try_fold(1usize, |acc, d| acc.checked_mul(*d));
        if a > b
            || b > data.len() as u64
            || count.and_then(|c| c.checked_mul(size)) != Some((b - a) as usize)
        {
            return Err(bad(&format!(
                "the bytes of the tensor `{name}` do not match its shape (the file is damaged)"
            )));
        }
        tensors.insert(name, (dtype, shape, &data[a as usize..b as usize]));
    }
    Ok(File { meta, tensors })
}

fn le<const N: usize>(bytes: &[u8]) -> impl Iterator<Item = [u8; N]> + '_ {
    bytes.as_chunks::<N>().0.iter().copied()
}

/// The short name of a type the standard library declares (`std::ml::Features` and `Features` are the same).
fn short(name: &str) -> &str {
    name.rsplit([':', '.']).next().unwrap_or(name)
}

struct Reader<'a, 'f> {
    file: &'f File<'a>,
    interp: &'f Interp,
}

impl Reader<'_, '_> {
    fn tensor(&self, node: &Json, at: &str) -> Result<(&str, Vec<usize>, &[u8]), String> {
        let name = node.as_str().unwrap_or("");
        let (dtype, shape, bytes) = self
            .file
            .tensors
            .get(name)
            .ok_or_else(|| format!("{at}: the tensor `{name}` is not in the file"))?;
        Ok((dtype.as_str(), shape.clone(), bytes))
    }

    fn i64s(&self, node: &Json, at: &str) -> Result<Vec<i64>, String> {
        let (dtype, _, bytes) = self.tensor(node, at)?;
        if dtype != "I64" {
            return Err(format!("{at}: expected whole numbers"));
        }
        Ok(le::<8>(bytes).map(i64::from_le_bytes).collect())
    }

    /// The value of `node`, which must be of type `ty`. `at` says where it is, for the error.
    fn value(&self, node: &Json, ty: &Ty, at: &str) -> Result<Value, String> {
        let found = || {
            type_text(
                node,
                &self
                    .file
                    .tensors
                    .iter()
                    .map(|(k, (d, s, _))| (k.clone(), (d.clone(), s.clone())))
                    .collect(),
            )
        };
        let mismatch = || {
            Err(format!(
                "{at} is `{}` in the file, and the program expects `{}`",
                found(),
                readable(&ty.to_string())
            ))
        };
        let get = |k: &str| node.get(k);
        match ty {
            Ty::Unit => get("unit").map(|_| Value::Unit).map_or_else(mismatch, Ok),
            Ty::Prim(Prim::Bool) => get("bool")
                .and_then(Json::as_bool)
                .map(Value::Bool)
                .map_or_else(mismatch, Ok),
            Ty::Prim(p) if p.is_int() => get("int")
                .and_then(Json::as_i64)
                .map(Value::Int)
                .map_or_else(mismatch, Ok),
            Ty::Prim(p) if p.is_float() => get("float")
                .and_then(unfloat)
                .map(Value::Float)
                .map_or_else(mismatch, Ok),
            Ty::Prim(Prim::Str) => get("text")
                .and_then(Json::as_str)
                .map(Value::str)
                .map_or_else(mismatch, Ok),
            Ty::Vec(inner) => {
                let Some(items) = get("list").and_then(Json::as_array) else {
                    return mismatch();
                };
                let items = items
                    .iter()
                    .enumerate()
                    .map(|(i, x)| self.value(x, inner, &format!("{at}[{i}]")))
                    .collect::<Result<_, _>>()?;
                Ok(Value::vec(items))
            }
            Ty::Tuple(parts) => {
                let Some(items) = get("tuple")
                    .and_then(Json::as_array)
                    .filter(|a| a.len() == parts.len())
                else {
                    return mismatch();
                };
                let items = items
                    .iter()
                    .zip(parts)
                    .enumerate()
                    .map(|(i, (x, t))| self.value(x, t, &format!("{at}.{i}")))
                    .collect::<Result<_, _>>()?;
                Ok(Value::Tuple(Arc::new(items)))
            }
            Ty::Option(inner) => match (get("some"), get("none")) {
                (Some(x), _) => Ok(Value::some(self.value(x, inner, at)?)),
                (None, Some(_)) => Ok(Value::none()),
                _ => mismatch(),
            },
            Ty::Struct(name) => {
                // The type as it is written (`SalesPredictor`), so a model saved by one program loads in another that
                // has the type in another module; its fields are checked one by one below.
                let stored_name = get("struct").and_then(Json::as_str).unwrap_or("");
                if short_name(&readable(stored_name)) != short_name(&readable(name)) {
                    return mismatch();
                }
                let shown = readable(name);
                let name_for_errors = &shown;
                let fields = self
                    .interp
                    .struct_fields(name)
                    .ok_or_else(|| format!("unknown type `{name}`"))?;
                let stored = node["fields"].as_object().cloned().unwrap_or_default();
                if let Some(extra) = stored.keys().find(|k| !fields.iter().any(|(n, _)| n == *k)) {
                    return Err(format!(
                        "{at}: the file's `{name_for_errors}` has a field `{extra}` the program's `{name_for_errors}` does \
                         not; it was saved by another version of the program"
                    ));
                }
                let mut out = Vec::new();
                for (field, fty) in fields {
                    let x = stored.get(field).ok_or_else(|| {
                        format!(
                            "{at}: the file's `{name_for_errors}` has no field `{field}`; it was saved by another version of \
                             the program"
                        )
                    })?;
                    out.push((field.clone(), self.value(x, fty, &format!("{at}.{field}"))?));
                }
                Ok(Value::Struct(Arc::new(StructVal {
                    name: name.clone(),
                    fields: out,
                })))
            }
            Ty::Tensor { elem, shape } => {
                let Some(t) = get("tensor") else {
                    return mismatch();
                };
                let (dtype, dims, bytes) = self.tensor(t, at)?;
                let fits = dims.len() == shape.len()
                    && dims
                        .iter()
                        .zip(shape)
                        .all(|(d, s)| !matches!(s, Dim::Lit(n) if *n as usize != *d));
                let tensor = match (dtype, &**elem) {
                    ("F32", Ty::Prim(Prim::F32)) if fits => {
                        TensorVal::from_f32(dims, le::<4>(bytes).map(f32::from_le_bytes).collect())
                    }
                    ("F64", Ty::Prim(Prim::F64)) if fits => {
                        TensorVal::from_f64(dims, le::<8>(bytes).map(f64::from_le_bytes).collect())
                    }
                    ("I64", Ty::Prim(Prim::I64)) if fits => TensorVal::from_array(
                        dims,
                        Arc::new(Int64Array::from(
                            le::<8>(bytes).map(i64::from_le_bytes).collect::<Vec<_>>(),
                        )),
                    )
                    .map_err(|e| e.to_string())?,
                    _ => return mismatch(),
                };
                Ok(Value::Tensor(Arc::new(tensor)))
            }
            Ty::Named(n) => self.native(node, short(n), at).unwrap_or_else(mismatch),
            _ => Err(format!("{at}: a `{ty}` cannot be loaded")),
        }
    }

    fn native(&self, node: &Json, kind: &str, at: &str) -> Option<Result<Value, String>> {
        let body = node.get(match kind {
            "Scaler" => "scaler",
            "LabelEncoder" => "label_encoder",
            "Features" => "features",
            "Sparse" => "sparse",
            _ => return None,
        })?;
        let damaged = || format!("{at}: the `{kind}` in the file is damaged");
        let texts = |j: &Json| -> Option<Vec<String>> {
            j.as_array()?
                .iter()
                .map(|x| x.as_str().map(String::from))
                .collect()
        };
        Some(match kind {
            "Scaler" => match (unfloat(&body["mean"]), unfloat(&body["std"])) {
                (Some(mean), Some(std)) => Ok(Value::Native(Native::Scaler(Arc::new(ScalerVal {
                    mean,
                    std,
                })))),
                _ => Err(damaged()),
            },
            "LabelEncoder" => texts(&body["classes"])
                .map(|classes| {
                    Value::Native(Native::LabelEncoder(Arc::new(EncoderVal { classes })))
                })
                .ok_or_else(damaged),
            "Features" => (|| {
                let numeric = body["numeric"]
                    .as_array()?
                    .iter()
                    .map(|x| {
                        Some((
                            x[0].as_str()?.to_string(),
                            ScalerVal {
                                mean: unfloat(&x[1])?,
                                std: unfloat(&x[2])?,
                            },
                        ))
                    })
                    .collect::<Option<Vec<_>>>()?;
                let categorical = body["categorical"]
                    .as_array()?
                    .iter()
                    .map(|x| Some((x[0].as_str()?.to_string(), texts(&x[1])?)))
                    .collect::<Option<Vec<_>>>()?;
                Some(Value::Native(Native::Features(Arc::new(FeaturesVal {
                    schema: body["schema"].as_str()?.to_string(),
                    target: body["target"].as_str()?.to_string(),
                    numeric,
                    categorical,
                }))))
            })()
            .ok_or_else(damaged),
            _ => (|| -> Result<Value, String> {
                let rows = body["rows"].as_u64().ok_or_else(damaged)? as usize;
                let cols = body["cols"].as_u64().ok_or_else(damaged)? as usize;
                let indptr: Vec<usize> = self
                    .i64s(&body["indptr"], at)?
                    .into_iter()
                    .map(|x| x as usize)
                    .collect();
                let indices: Vec<u32> = self
                    .i64s(&body["indices"], at)?
                    .into_iter()
                    .map(|x| x as u32)
                    .collect();
                let (dtype, _, bytes) = self.tensor(&body["data"], at)?;
                let data: Vec<f32> = if dtype == "F32" {
                    le::<4>(bytes).map(f32::from_le_bytes).collect()
                } else {
                    vec![]
                };
                // The layout is checked, so a damaged file cannot index outside the matrix.
                let ok = indptr.len() == rows + 1
                    && indptr.first() == Some(&0)
                    && indptr.windows(2).all(|w| w[0] <= w[1])
                    && indptr.last() == Some(&data.len())
                    && indices.len() == data.len()
                    && indices.iter().all(|c| (*c as usize) < cols);
                if !ok {
                    return Err(damaged());
                }
                Ok(Value::Native(Native::Sparse(Arc::new(SparseVal {
                    rows,
                    cols,
                    indptr,
                    indices,
                    data,
                }))))
            })(),
        })
    }
}

/// The value of a model file, as `ty`.
pub(crate) fn from_bytes(interp: &Interp, bytes: &[u8], ty: &Ty) -> Result<Value, String> {
    let file = open(bytes)?;
    let tree: Json = file
        .meta
        .get("value")
        .and_then(Json::as_str)
        .and_then(|s| serde_json::from_str(s).ok())
        .ok_or("not a Telvax model file: the value is missing")?;
    Reader {
        file: &file,
        interp,
    }
    .value(&tree, ty, "the model")
}

/// The type of the value in a model file, as text.
pub(crate) fn describe(bytes: &[u8]) -> Result<String, String> {
    let file = open(bytes)?;
    Ok(file
        .meta
        .get("type")
        .and_then(Json::as_str)
        .unwrap_or("?")
        .to_string())
}
