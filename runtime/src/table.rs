//! Arrow-backed tables (`Table<Row>`) and the `std.table` functions.
//!
//! A table is one Arrow `RecordBatch` whose columns follow the fields of a struct. Numeric
//! columns are exposed as tensors that share the batch's buffers, so reading a column copies
//! nothing. CSV and Parquet files are decoded straight into Arrow arrays.

use std::fs::File;
use std::sync::Arc;

use arrow::array::{
    Array, ArrayRef, BooleanArray, Date32Array, Date64Array, Float32Array, Float64Array, Int8Array,
    Int16Array, Int32Array, Int64Array, LargeStringArray, StringArray, TimestampMicrosecondArray,
    TimestampMillisecondArray, TimestampNanosecondArray, TimestampSecondArray, UInt8Array,
    UInt16Array, UInt32Array, UInt64Array, make_array,
};
use arrow::buffer::NullBuffer;
use arrow::compute::{
    SortColumn, SortOptions, cast, concat_batches, filter_record_batch, lexsort_to_indices,
    take_record_batch,
};
use arrow::datatypes::{
    DataType, Date32Type, Date64Type, Field, Schema, TimeUnit, TimestampMicrosecondType,
    TimestampMillisecondType, TimestampNanosecondType, TimestampSecondType,
};
use arrow::record_batch::RecordBatch;
use arrow::temporal_conversions::{as_date, as_datetime};
use serde_json::Value as Json;
use telvax_hir::{Prim, Ty};

use crate::interp::{Interp, RuntimeError};
use crate::value::{StructVal, TableVal, TensorVal, Value};

type Res = Result<Value, RuntimeError>;

fn err<T>(msg: impl Into<String>) -> Result<T, RuntimeError> {
    Err(RuntimeError::new(msg))
}

fn io<E: std::fmt::Display>(what: &str) -> impl Fn(E) -> RuntimeError + '_ {
    move |e| RuntimeError::new(format!("{what}: {e}"))
}

pub(crate) fn arrow_type(ty: &Ty) -> Result<DataType, RuntimeError> {
    Ok(match ty {
        Ty::Option(inner) if !matches!(**inner, Ty::Option(_)) => return arrow_type(inner),
        Ty::Prim(Prim::I8) => DataType::Int8,
        Ty::Prim(Prim::I16) => DataType::Int16,
        Ty::Prim(Prim::I32) => DataType::Int32,
        Ty::Prim(Prim::I64) => DataType::Int64,
        Ty::Prim(Prim::U8) => DataType::UInt8,
        Ty::Prim(Prim::U16) => DataType::UInt16,
        Ty::Prim(Prim::U32) => DataType::UInt32,
        Ty::Prim(Prim::U64) => DataType::UInt64,
        Ty::Prim(Prim::F32) => DataType::Float32,
        Ty::Prim(Prim::F64) => DataType::Float64,
        Ty::Prim(Prim::Bool) => DataType::Boolean,
        Ty::Prim(Prim::Str) => DataType::Utf8,
        Ty::Prim(Prim::Date) => DataType::Date32,
        Ty::Prim(Prim::Timestamp) => DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
        other => return err(format!("`{other}` cannot be a table column")),
    })
}

pub(crate) fn schema_of(fields: &[(String, Ty)]) -> Result<Arc<Schema>, RuntimeError> {
    let cols = fields
        .iter()
        .map(|(n, t)| Ok(Field::new(n, arrow_type(t)?, matches!(t, Ty::Option(_)))))
        .collect::<Result<Vec<_>, RuntimeError>>()?;
    Ok(Arc::new(Schema::new(cols)))
}

/// One cell as a runtime value.
pub(crate) fn cell(array: &ArrayRef, i: usize) -> Result<Value, RuntimeError> {
    macro_rules! int {
        ($t:ty) => {
            Value::Int(i64::from(
                array.as_any().downcast_ref::<$t>().expect("dtype").value(i),
            ))
        };
    }
    Ok(match array.data_type() {
        DataType::Int8 => int!(Int8Array),
        DataType::Int16 => int!(Int16Array),
        DataType::Int32 => int!(Int32Array),
        DataType::Int64 => int!(Int64Array),
        DataType::UInt8 => int!(UInt8Array),
        DataType::UInt16 => int!(UInt16Array),
        DataType::UInt32 => int!(UInt32Array),
        DataType::UInt64 => {
            let v = array
                .as_any()
                .downcast_ref::<UInt64Array>()
                .expect("dtype")
                .value(i);
            Value::Int(
                i64::try_from(v)
                    .map_err(|_| RuntimeError::new("u64 value does not fit in an i64"))?,
            )
        }
        DataType::Float32 => Value::Float(f64::from(
            array
                .as_any()
                .downcast_ref::<Float32Array>()
                .expect("dtype")
                .value(i),
        )),
        DataType::Float64 => Value::Float(
            array
                .as_any()
                .downcast_ref::<Float64Array>()
                .expect("dtype")
                .value(i),
        ),
        DataType::Boolean => Value::Bool(
            array
                .as_any()
                .downcast_ref::<BooleanArray>()
                .expect("dtype")
                .value(i),
        ),
        DataType::Utf8 => Value::str(
            array
                .as_any()
                .downcast_ref::<StringArray>()
                .expect("dtype")
                .value(i),
        ),
        DataType::LargeUtf8 => Value::str(
            array
                .as_any()
                .downcast_ref::<LargeStringArray>()
                .expect("dtype")
                .value(i),
        ),
        DataType::Date32 => Value::str(format_date(as_date::<Date32Type>(i64::from(
            array
                .as_any()
                .downcast_ref::<Date32Array>()
                .expect("dtype")
                .value(i),
        )))?),
        DataType::Date64 => Value::str(format_date(as_date::<Date64Type>(
            array
                .as_any()
                .downcast_ref::<Date64Array>()
                .expect("dtype")
                .value(i),
        ))?),
        DataType::Timestamp(unit, _) => {
            macro_rules! ts {
                ($t:ty, $a:ty) => {
                    as_datetime::<$t>(array.as_any().downcast_ref::<$a>().expect("dtype").value(i))
                };
            }
            let naive = match unit {
                TimeUnit::Second => ts!(TimestampSecondType, TimestampSecondArray),
                TimeUnit::Millisecond => ts!(TimestampMillisecondType, TimestampMillisecondArray),
                TimeUnit::Microsecond => ts!(TimestampMicrosecondType, TimestampMicrosecondArray),
                TimeUnit::Nanosecond => ts!(TimestampNanosecondType, TimestampNanosecondArray),
            };
            let naive = naive.ok_or_else(|| RuntimeError::new("timestamp out of range"))?;
            Value::str(
                chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(naive, chrono::Utc)
                    .to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true),
            )
        }
        other => return err(format!("unsupported column type {other}")),
    })
}

fn format_date(d: Option<chrono::NaiveDate>) -> Result<String, RuntimeError> {
    d.map(|d| d.format("%Y-%m-%d").to_string())
        .ok_or_else(|| RuntimeError::new("date out of range"))
}

/// `"2026-10-17"` as days since 1970-01-01.
fn parse_date(s: &str) -> Result<i32, RuntimeError> {
    let d = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .map_err(|_| RuntimeError::new(format!("`{s}` is not a Date; write it as YYYY-MM-DD")))?;
    let epoch = chrono::NaiveDate::from_ymd_opt(1970, 1, 1).expect("epoch");
    i32::try_from(d.signed_duration_since(epoch).num_days())
        .map_err(|_| RuntimeError::new(format!("`{s}` is out of range for a Date")))
}

/// `"2026-10-17T16:53:03Z"` (any offset, or none meaning UTC) as microseconds since the epoch.
fn parse_timestamp(s: &str) -> Result<i64, RuntimeError> {
    let bad = || {
        RuntimeError::new(format!(
            "`{s}` is not a Timestamp; write it as YYYY-MM-DDTHH:MM:SSZ"
        ))
    };
    let utc = match chrono::DateTime::parse_from_rfc3339(s) {
        Ok(t) => t.naive_utc(),
        Err(_) => chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f")
            .or_else(|_| chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S%.f"))
            .map_err(|_| bad())?,
    };
    Ok(utc.and_utc().timestamp_micros())
}

/// A cell of a column declared `T?`: `Some(value)`, or `None` for a null.
fn field_cell(field: &Field, col: &ArrayRef, i: usize) -> Result<Value, RuntimeError> {
    if !field.is_nullable() {
        return cell(col, i);
    }
    if col.is_null(i) {
        return Ok(Value::none());
    }
    cell(col, i).map(Value::some)
}

impl TableVal {
    pub fn rows(&self) -> usize {
        self.batch.num_rows()
    }

    /// Row `i` as a struct value.
    pub fn row(&self, i: usize) -> Result<Value, RuntimeError> {
        if i >= self.rows() {
            return err(format!(
                "index out of bounds: the length is {} but the index is {i}",
                self.rows()
            ));
        }
        let schema = self.batch.schema();
        let mut fields = Vec::with_capacity(schema.fields().len());
        for (f, col) in schema.fields().iter().zip(self.batch.columns()) {
            fields.push((f.name().clone(), field_cell(f, col, i)?));
        }
        Ok(Value::Struct(Arc::new(StructVal {
            name: self.schema.clone(),
            fields,
        })))
    }

    /// A column: numeric columns become zero-copy tensors, the others plain vectors.
    pub fn column(&self, name: &str) -> Result<Value, RuntimeError> {
        let idx = self
            .batch
            .schema()
            .index_of(name)
            .map_err(|_| RuntimeError::new(format!("table has no column `{name}`")))?;
        let col = self.batch.column(idx).clone();
        let field = self.batch.schema().field(idx).clone();
        if field.is_nullable() {
            return Ok(Value::vec(
                (0..col.len())
                    .map(|i| field_cell(&field, &col, i))
                    .collect::<Result<_, _>>()?,
            ));
        }
        if col.data_type().is_numeric() {
            return Ok(Value::Tensor(Arc::new(TensorVal::from_array(
                vec![col.len()],
                col,
            )?)));
        }
        Ok(Value::vec(
            (0..col.len())
                .map(|i| cell(&col, i))
                .collect::<Result<_, _>>()?,
        ))
    }

    pub fn to_json(&self) -> Json {
        let schema = self.batch.schema();
        Json::Array(
            (0..self.rows())
                .map(|i| {
                    let mut m = serde_json::Map::new();
                    for (f, col) in schema.fields().iter().zip(self.batch.columns()) {
                        m.insert(
                            f.name().clone(),
                            field_cell(f, col, i)
                                .map(|v| v.to_json())
                                .unwrap_or(Json::Null),
                        );
                    }
                    Json::Object(m)
                })
                .collect(),
        )
    }
}

/// Builds a batch whose columns follow `fields` from struct values.
pub(crate) fn rows_to_batch(
    fields: &[(String, Ty)],
    rows: &[Value],
) -> Result<RecordBatch, RuntimeError> {
    let schema = schema_of(fields)?;
    let mut columns: Vec<ArrayRef> = Vec::with_capacity(fields.len());
    for (name, ty) in fields {
        let mut cells = Vec::with_capacity(rows.len());
        for r in rows {
            let Value::Struct(s) = r else {
                return err("a table is built from structs");
            };
            let v = s
                .fields
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, v)| v)
                .ok_or_else(|| RuntimeError::new(format!("row has no field `{name}`")))?;
            cells.push(v.clone());
        }
        columns.push(build_column(ty, &cells)?);
    }
    RecordBatch::try_new(schema, columns).map_err(io("building the table"))
}

fn build_column(ty: &Ty, cells: &[Value]) -> Result<ArrayRef, RuntimeError> {
    if let Ty::Option(inner) = ty {
        return build_nullable(inner, cells);
    }
    let int = |v: &Value| match v {
        Value::Int(i) => Ok(*i),
        other => err(format!("expected an integer, found {}", other.type_name())),
    };
    let float = |v: &Value| match v {
        Value::Float(x) => Ok(*x),
        Value::Int(i) => Ok(*i as f64),
        other => err(format!("expected a number, found {}", other.type_name())),
    };
    macro_rules! ints {
        ($arr:ident, $t:ty) => {{
            let mut out: Vec<$t> = Vec::with_capacity(cells.len());
            for c in cells {
                out.push(
                    <$t>::try_from(int(c)?)
                        .map_err(|_| RuntimeError::new("integer out of range for the column"))?,
                );
            }
            Arc::new($arr::from(out)) as ArrayRef
        }};
    }
    Ok(match ty {
        Ty::Prim(Prim::I8) => ints!(Int8Array, i8),
        Ty::Prim(Prim::I16) => ints!(Int16Array, i16),
        Ty::Prim(Prim::I32) => ints!(Int32Array, i32),
        Ty::Prim(Prim::I64) => ints!(Int64Array, i64),
        Ty::Prim(Prim::U8) => ints!(UInt8Array, u8),
        Ty::Prim(Prim::U16) => ints!(UInt16Array, u16),
        Ty::Prim(Prim::U32) => ints!(UInt32Array, u32),
        Ty::Prim(Prim::U64) => ints!(UInt64Array, u64),
        Ty::Prim(Prim::F32) => Arc::new(Float32Array::from(
            cells
                .iter()
                .map(|c| float(c).map(|x| x as f32))
                .collect::<Result<Vec<_>, _>>()?,
        )),
        Ty::Prim(Prim::F64) => Arc::new(Float64Array::from(
            cells.iter().map(float).collect::<Result<Vec<_>, _>>()?,
        )),
        Ty::Prim(Prim::Bool) => Arc::new(BooleanArray::from(
            cells
                .iter()
                .map(|c| match c {
                    Value::Bool(b) => Ok(*b),
                    other => err(format!("expected a bool, found {}", other.type_name())),
                })
                .collect::<Result<Vec<_>, _>>()?,
        )),
        Ty::Prim(Prim::Date) => Arc::new(Date32Array::from(
            text_cells(cells)?
                .iter()
                .map(|s| parse_date(s))
                .collect::<Result<Vec<_>, _>>()?,
        )),
        Ty::Prim(Prim::Timestamp) => Arc::new(
            TimestampMicrosecondArray::from(
                text_cells(cells)?
                    .iter()
                    .map(|s| parse_timestamp(s))
                    .collect::<Result<Vec<_>, _>>()?,
            )
            .with_timezone("UTC"),
        ),
        Ty::Prim(Prim::Str) => Arc::new(StringArray::from(
            cells
                .iter()
                .map(|c| match c {
                    Value::Str(s) => Ok(s.to_string()),
                    other => err(format!("expected a Text, found {}", other.type_name())),
                })
                .collect::<Result<Vec<_>, _>>()?,
        )),
        other => return err(format!("`{other}` cannot be a table column")),
    })
}

fn text_cells(cells: &[Value]) -> Result<Vec<Arc<str>>, RuntimeError> {
    cells
        .iter()
        .map(|c| match c {
            Value::Str(s) => Ok(s.clone()),
            other => err(format!("expected a Text, found {}", other.type_name())),
        })
        .collect()
}

/// A column of `T?`: the `None` cells become nulls. They are built as a placeholder value of the
/// element type first, then masked by the validity bitmap.
fn build_nullable(inner: &Ty, cells: &[Value]) -> Result<ArrayRef, RuntimeError> {
    let placeholder = match inner {
        Ty::Prim(Prim::F32 | Prim::F64) => Value::Float(0.0),
        Ty::Prim(Prim::Bool) => Value::Bool(false),
        Ty::Prim(Prim::Str) => Value::str(""),
        Ty::Prim(Prim::Date) => Value::str("1970-01-01"),
        Ty::Prim(Prim::Timestamp) => Value::str("1970-01-01T00:00:00Z"),
        _ => Value::Int(0),
    };
    let mut valid = Vec::with_capacity(cells.len());
    let mut plain = Vec::with_capacity(cells.len());
    for c in cells {
        match c {
            Value::Enum(e) if e.name == "Option" => match e.fields.first() {
                Some(v) => {
                    valid.push(true);
                    plain.push(v.clone());
                }
                None => {
                    valid.push(false);
                    plain.push(placeholder.clone());
                }
            },
            other => {
                return err(format!("expected an Option, found {}", other.type_name()));
            }
        }
    }
    let array = build_column(inner, &plain)?;
    let data = array
        .to_data()
        .into_builder()
        .nulls(Some(NullBuffer::from(valid)))
        .build()
        .map_err(io("building the table"))?;
    Ok(make_array(data))
}

/// Reorders / casts `batch` to the columns of `fields` (matched by name).
pub(crate) fn conform(
    batch: &RecordBatch,
    fields: &[(String, Ty)],
) -> Result<RecordBatch, RuntimeError> {
    let schema = schema_of(fields)?;
    let mut columns = Vec::with_capacity(fields.len());
    for (name, ty) in fields {
        let idx = batch
            .schema()
            .index_of(name)
            .map_err(|_| RuntimeError::new(format!("the data has no column `{name}`")))?;
        let want = arrow_type(ty)?;
        let col = cast(batch.column(idx), &want).map_err(|e| {
            RuntimeError::new(format!("column `{name}` cannot be read as {want}: {e}"))
        })?;
        if col.null_count() > 0 && !matches!(ty, Ty::Option(_)) {
            return err(format!(
                "column `{name}` contains empty values; declare it `{ty}?` to allow them"
            ));
        }
        columns.push(col);
    }
    RecordBatch::try_new(schema, columns).map_err(io("building the table"))
}

fn want_table(v: &Value) -> Result<&Arc<TableVal>, RuntimeError> {
    match v {
        Value::Table(t) => Ok(t),
        other => err(format!("expected a Table, found {}", other.type_name())),
    }
}

fn want_str(v: &Value) -> Result<Arc<str>, RuntimeError> {
    match v {
        Value::Str(s) => Ok(s.clone()),
        other => err(format!("expected a Text, found {}", other.type_name())),
    }
}

fn schema_name(result: &Ty, what: &str) -> Result<String, RuntimeError> {
    match result {
        Ty::Table(s) => Ok(s.clone()),
        _ => err(format!("`{what}` needs a statically known result type")),
    }
}

fn fields_of<'a>(interp: &'a Interp, name: &str) -> Result<&'a [(String, Ty)], RuntimeError> {
    interp
        .struct_fields(name)
        .ok_or_else(|| RuntimeError::new(format!("unknown type `{name}`")))
}

fn table(schema: String, batch: RecordBatch) -> Res {
    Ok(Value::Table(Arc::new(TableVal { schema, batch })))
}

/// The rows of a CSV file in batches of at most `size` rows (not yet conformed to `fields`).
fn csv_batches(
    path: &str,
    fields: &[(String, Ty)],
    size: usize,
) -> Result<(Arc<Schema>, Vec<RecordBatch>), RuntimeError> {
    // Read every column as nullable so `conform` can report an empty value in a required column
    // with the Telvax type, instead of Arrow's message.
    let schema = Arc::new(Schema::new(
        schema_of(fields)?
            .fields()
            .iter()
            .map(|f| f.as_ref().clone().with_nullable(true))
            .collect::<Vec<_>>(),
    ));
    let file = File::open(path).map_err(io(&format!("cannot open `{path}`")))?;
    let reader = arrow::csv::ReaderBuilder::new(schema.clone())
        .with_header(true)
        .with_batch_size(size)
        .build(file)
        .map_err(io("reading CSV"))?;
    let batches = reader
        .collect::<Result<Vec<_>, _>>()
        .map_err(io("reading CSV"))?;
    Ok((schema, batches))
}

/// The rows per chunk: at least one.
fn chunk_size(v: &Value) -> Result<usize, RuntimeError> {
    match v {
        Value::Int(n) if *n >= 1 => Ok(*n as usize),
        Value::Int(_) => err("the chunk size must be at least 1"),
        other => err(format!(
            "the chunk size must be an integer, found {}",
            other.type_name()
        )),
    }
}

/// `batch` cut into pieces of at most `size` rows. The pieces share its buffers.
fn slices(batch: &RecordBatch, size: usize) -> Vec<RecordBatch> {
    (0..batch.num_rows())
        .step_by(size)
        .map(|start| batch.slice(start, size.min(batch.num_rows() - start)))
        .collect()
}

fn read_csv(path: &str, fields: &[(String, Ty)]) -> Result<RecordBatch, RuntimeError> {
    let (schema, batches) = csv_batches(path, fields, 1024)?;
    let all = concat_batches(&schema, &batches).map_err(io("reading CSV"))?;
    conform(&all, fields)
}

fn write_csv(t: &TableVal, path: &str) -> Result<(), RuntimeError> {
    let file = File::create(path).map_err(io(&format!("cannot create `{path}`")))?;
    let mut w = arrow::csv::WriterBuilder::new()
        .with_header(true)
        .build(file);
    w.write(&t.batch).map_err(io("writing CSV"))
}

/// Arrow IPC file (Feather v2). The file is memory-mapped and the columns point into the mapping,
/// so nothing is copied for a file with one batch and no compression. The file must not be
/// truncated or rewritten while a table read from it is alive.
fn read_ipc(path: &str, fields: &[(String, Ty)]) -> Result<RecordBatch, RuntimeError> {
    let (schema, mut batches) = ipc_batches(path)?;
    // One batch stays zero-copy; several are joined, which copies.
    let all = match batches.len() {
        1 => batches.pop().expect("one batch"),
        _ => concat_batches(&schema, &batches).map_err(io("reading Arrow IPC"))?,
    };
    conform(&all, fields)
}

/// The batches of an Arrow IPC file, as the file stores them.
fn ipc_batches(path: &str) -> Result<(Arc<Schema>, Vec<RecordBatch>), RuntimeError> {
    use arrow::buffer::Buffer;
    use arrow::ipc::convert::fb_to_schema;
    use arrow::ipc::reader::{FileDecoder, read_footer_length};
    use arrow::ipc::root_as_footer;

    let bad = |what: &str| RuntimeError::new(format!("reading Arrow IPC `{path}`: {what}"));
    let file = File::open(path).map_err(io(&format!("cannot open `{path}`")))?;
    // SAFETY: the mapping is read-only. A concurrent truncation by another process can make a
    // read fault, which is the same hazard as any memory-mapped file (documented on this fn).
    let map = unsafe { memmap2::Mmap::map(&file) }.map_err(io("mapping the file"))?;
    let buffer = Buffer::from(bytes::Bytes::from_owner(map));

    let trailer = buffer
        .len()
        .checked_sub(10)
        .ok_or_else(|| bad("the file is too short to be an Arrow IPC file"))?;
    let footer_len = read_footer_length(buffer[trailer..].try_into().expect("10 bytes"))
        .map_err(|e| bad(&e.to_string()))?;
    let footer_start = trailer
        .checked_sub(footer_len)
        .ok_or_else(|| bad("the footer is longer than the file"))?;
    let footer = root_as_footer(&buffer[footer_start..trailer])
        .map_err(|e| bad(&format!("invalid footer: {e}")))?;
    let schema = Arc::new(fb_to_schema(
        footer
            .schema()
            .ok_or_else(|| bad("the footer has no schema"))?,
    ));
    let block_data = |block: &arrow::ipc::Block| {
        let (offset, len) = (
            block.offset(),
            block.bodyLength() + i64::from(block.metaDataLength()),
        );
        match (usize::try_from(offset), usize::try_from(len)) {
            (Ok(o), Ok(l)) if o.checked_add(l).is_some_and(|end| end <= footer_start) => {
                Ok(buffer.slice_with_length(o, l))
            }
            _ => Err(bad("a block lies outside the file")),
        }
    };

    let mut decoder = FileDecoder::new(schema.clone(), footer.version());
    for block in footer.dictionaries().iter().flatten() {
        decoder
            .read_dictionary(block, &block_data(block)?)
            .map_err(|e| bad(&e.to_string()))?;
    }
    let mut batches = Vec::new();
    for block in footer.recordBatches().iter().flatten() {
        if let Some(b) = decoder
            .read_record_batch(block, &block_data(block)?)
            .map_err(|e| bad(&e.to_string()))?
        {
            batches.push(b);
        }
    }
    Ok((schema, batches))
}

/// Writes an Arrow IPC file (Feather v2), which PyArrow, Polars and pandas read.
fn write_ipc(t: &TableVal, path: &str) -> Result<(), RuntimeError> {
    let file = File::create(path).map_err(io(&format!("cannot create `{path}`")))?;
    let mut w = arrow::ipc::writer::FileWriter::try_new(file, t.batch.schema().as_ref())
        .map_err(io("writing Arrow IPC"))?;
    w.write(&t.batch).map_err(io("writing Arrow IPC"))?;
    w.finish().map_err(io("writing Arrow IPC"))
}

/// The rows of a Parquet file in batches of at most `size` rows, when given.
#[cfg(feature = "tables")]
fn parquet_batches(
    path: &str,
    size: Option<usize>,
) -> Result<(Arc<Schema>, Vec<RecordBatch>), RuntimeError> {
    use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
    let file = File::open(path).map_err(io(&format!("cannot open `{path}`")))?;
    let mut builder =
        ParquetRecordBatchReaderBuilder::try_new(file).map_err(io("reading Parquet"))?;
    if let Some(n) = size {
        builder = builder.with_batch_size(n);
    }
    let file_schema = builder.schema().clone();
    let batches = builder
        .build()
        .map_err(io("reading Parquet"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(io("reading Parquet"))?;
    Ok((file_schema, batches))
}

#[cfg(feature = "tables")]
fn read_parquet(path: &str, fields: &[(String, Ty)]) -> Result<RecordBatch, RuntimeError> {
    let (file_schema, batches) = parquet_batches(path, None)?;
    let all = concat_batches(&file_schema, &batches).map_err(io("reading Parquet"))?;
    conform(&all, fields)
}

#[cfg(feature = "tables")]
fn write_parquet(t: &TableVal, path: &str) -> Result<(), RuntimeError> {
    let file = File::create(path).map_err(io(&format!("cannot create `{path}`")))?;
    let mut w = parquet::arrow::ArrowWriter::try_new(file, t.batch.schema(), None)
        .map_err(io("writing Parquet"))?;
    w.write(&t.batch).map_err(io("writing Parquet"))?;
    w.close().map(|_| ()).map_err(io("writing Parquet"))
}

/// Runs `query` with the table registered as `t`. DataFusion needs a Tokio runtime, so the
/// query runs on its own short-lived thread and works from any calling context.
#[cfg(feature = "tables")]
fn run_sql(batch: &RecordBatch, query: &str) -> Result<RecordBatch, RuntimeError> {
    use datafusion::datasource::MemTable;
    use datafusion::prelude::SessionContext;

    let outcome = std::thread::scope(|s| {
        s.spawn(|| -> Result<RecordBatch, String> {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| e.to_string())?;
            rt.block_on(async {
                let ctx = SessionContext::new();
                let provider = MemTable::try_new(batch.schema(), vec![vec![batch.clone()]])
                    .map_err(|e| e.to_string())?;
                ctx.register_table("t", Arc::new(provider))
                    .map_err(|e| e.to_string())?;
                let df = ctx.sql(query).await.map_err(|e| e.to_string())?;
                let schema = Arc::new(df.schema().as_arrow().clone());
                let batches = df.collect().await.map_err(|e| e.to_string())?;
                concat_batches(&schema, &batches).map_err(|e| e.to_string())
            })
        })
        .join()
    });
    match outcome {
        Ok(Ok(b)) => Ok(b),
        Ok(Err(e)) => err(format!("SQL error: {e}")),
        Err(_) => err("the SQL engine panicked"),
    }
}

#[cfg(not(feature = "flight"))]
fn flight_unavailable<T>() -> Result<T, RuntimeError> {
    err("Arrow Flight is not available in this build; build with `--features flight`")
}

#[cfg(not(feature = "tables"))]
fn unavailable<T>(what: &str) -> Result<T, RuntimeError> {
    err(format!(
        "{what} is not available in this build (compiled without the `tables` feature)"
    ))
}

pub(crate) fn call(interp: &Interp, path: &str, args: &[Value], result: &Ty) -> Res {
    let name = path.rsplit("::").next().unwrap_or(path);
    match name {
        "len" => Ok(Value::Int(want_table(&args[0])?.rows() as i64)),
        "read_csv" | "read_parquet" | "read_ipc" => {
            let schema = schema_name(result, path)?;
            let fields = fields_of(interp, &schema)?;
            let file = want_str(&args[0])?;
            let batch = if name == "read_csv" {
                read_csv(&file, fields)?
            } else if name == "read_ipc" {
                read_ipc(&file, fields)?
            } else {
                #[cfg(feature = "tables")]
                {
                    read_parquet(&file, fields)?
                }
                #[cfg(not(feature = "tables"))]
                {
                    return unavailable("Parquet");
                }
            };
            table(schema, batch)
        }
        "read_csv_batches" | "read_parquet_batches" | "read_ipc_batches" => {
            let Ty::Vec(row) = result else {
                return err(format!("`{path}` needs a statically known result type"));
            };
            let schema = schema_name(row, path)?;
            let fields = fields_of(interp, &schema)?;
            let file = want_str(&args[0])?;
            let size = chunk_size(&args[1])?;
            let batches: Vec<RecordBatch> = match name {
                "read_csv_batches" => csv_batches(&file, fields, size)?.1,
                "read_ipc_batches" => ipc_batches(&file)?
                    .1
                    .iter()
                    .flat_map(|b| slices(b, size))
                    .collect(),
                _ => {
                    #[cfg(feature = "tables")]
                    {
                        parquet_batches(&file, Some(size))?.1
                    }
                    #[cfg(not(feature = "tables"))]
                    {
                        return unavailable("Parquet");
                    }
                }
            };
            Ok(Value::vec(
                batches
                    .iter()
                    .filter(|b| b.num_rows() > 0)
                    .map(|b| table(schema.clone(), conform(b, fields)?))
                    .collect::<Result<_, _>>()?,
            ))
        }
        "split" => {
            let t = want_table(&args[0])?;
            let size = chunk_size(&args[1])?;
            Ok(Value::vec(
                slices(&t.batch, size)
                    .into_iter()
                    .map(|b| table(t.schema.clone(), b))
                    .collect::<Result<_, _>>()?,
            ))
        }
        "read_flight" => {
            let schema = schema_name(result, path)?;
            let fields = fields_of(interp, &schema)?;
            let (url, ticket) = (want_str(&args[0])?, want_str(&args[1])?);
            #[cfg(feature = "flight")]
            {
                let batches = crate::flight::read(&url, &ticket)?;
                let Some(first) = batches.first() else {
                    return err(format!("the Flight server returned no data for `{ticket}`"));
                };
                let all =
                    concat_batches(&first.schema(), &batches).map_err(io("reading Flight"))?;
                table(schema, conform(&all, fields)?)
            }
            #[cfg(not(feature = "flight"))]
            {
                let _ = (fields, url, ticket);
                flight_unavailable()
            }
        }
        "write_flight" => {
            let t = want_table(&args[0])?;
            let (url, name) = (want_str(&args[1])?, want_str(&args[2])?);
            #[cfg(feature = "flight")]
            {
                crate::flight::write(&url, &name, &t.batch).map(|()| Value::Unit)
            }
            #[cfg(not(feature = "flight"))]
            {
                let _ = (t, url, name);
                flight_unavailable()
            }
        }
        "write_ipc" => write_ipc(want_table(&args[0])?, &want_str(&args[1])?).map(|()| Value::Unit),
        "write_csv" => write_csv(want_table(&args[0])?, &want_str(&args[1])?).map(|()| Value::Unit),
        "write_parquet" => {
            #[cfg(feature = "tables")]
            {
                write_parquet(want_table(&args[0])?, &want_str(&args[1])?).map(|()| Value::Unit)
            }
            #[cfg(not(feature = "tables"))]
            {
                unavailable("Parquet")
            }
        }
        "sql" => {
            #[cfg(feature = "tables")]
            {
                let schema = schema_name(result, path)?;
                let t = want_table(&args[0])?;
                let out = run_sql(&t.batch, &want_str(&args[1])?)?;
                table(schema.clone(), conform(&out, fields_of(interp, &schema)?)?)
            }
            #[cfg(not(feature = "tables"))]
            {
                unavailable("SQL")
            }
        }
        "from_rows" => {
            let Ty::Table(schema) = result else {
                return err("`from_rows` needs a statically known row type");
            };
            let Value::Vec(rows) = &args[0] else {
                return err("`from_rows` needs a List");
            };
            let batch = rows_to_batch(fields_of(interp, schema)?, &rows.snapshot())?;
            table(schema.clone(), batch)
        }
        "to_rows" => {
            let t = want_table(&args[0])?;
            Ok(Value::vec(
                (0..t.rows()).map(|i| t.row(i)).collect::<Result<_, _>>()?,
            ))
        }
        "filter" => {
            let t = want_table(&args[0])?;
            let mut keep = Vec::with_capacity(t.rows());
            for i in 0..t.rows() {
                match interp.apply(&args[1], vec![t.row(i)?])? {
                    Value::Bool(b) => keep.push(b),
                    other => {
                        return err(format!("filter predicate returned {}", other.type_name()));
                    }
                }
            }
            let batch = filter_record_batch(&t.batch, &BooleanArray::from(keep))
                .map_err(io("filtering"))?;
            table(t.schema.clone(), batch)
        }
        "head" | "tail" => {
            let t = want_table(&args[0])?;
            let Value::Int(n) = args[1] else {
                return err("the row count must be an integer");
            };
            let n = usize::try_from(n)
                .map_err(|_| RuntimeError::new(format!("`{name}` needs a count of zero or more")))?
                .min(t.rows());
            let start = if name == "head" { 0 } else { t.rows() - n };
            table(t.schema.clone(), t.batch.slice(start, n))
        }
        "sort_by" | "sort_by_desc" => {
            let t = want_table(&args[0])?;
            let column = want_str(&args[1])?;
            let Some(i) = t.batch.schema().index_of(&column).ok() else {
                let names: Vec<_> = t
                    .batch
                    .schema()
                    .fields()
                    .iter()
                    .map(|f| f.name().clone())
                    .collect();
                return err(format!(
                    "no column `{column}` to sort by; the columns are {}",
                    names.join(", ")
                ));
            };
            let keys = [SortColumn {
                values: t.batch.column(i).clone(),
                options: Some(SortOptions {
                    descending: name == "sort_by_desc",
                    nulls_first: false,
                }),
            }];
            let order = lexsort_to_indices(&keys, None).map_err(io("sorting"))?;
            let batch = take_record_batch(&t.batch, &order).map_err(io("sorting"))?;
            table(t.schema.clone(), batch)
        }
        "select" => {
            let t = want_table(&args[0])?;
            let schema = schema_name(result, path)?;
            let batch = conform(&t.batch, fields_of(interp, &schema)?)?;
            table(schema, batch)
        }
        "concat" => {
            let (a, b) = (want_table(&args[0])?, want_table(&args[1])?);
            if a.schema != b.schema {
                return err(format!(
                    "`concat` needs two tables with the same rows, found `Table[{}]` and `Table[{}]`",
                    a.schema, b.schema
                ));
            }
            let batch = concat_batches(&a.batch.schema(), [&a.batch, &b.batch])
                .map_err(io("concatenating"))?;
            table(a.schema.clone(), batch)
        }
        other => err(format!(
            "std function `std::table::{other}` is not implemented by the runtime"
        )),
    }
}
