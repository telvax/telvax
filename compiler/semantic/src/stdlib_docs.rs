//! The text of the standard library reference (`docs/reference/std.md`).
//!
//! The functions and types are in `stdlib.rs`; this file says what each one does. A test
//! (`tests/reference.rs`) fails when an item has no entry here, when an entry names an item that does
//! not exist, and when `docs/reference/std.md` is not what [`reference_markdown`] writes.
//!
//! The signatures of the methods come from the tables in `stdlib.rs`. The signatures of the functions are
//! written here, because the type checker computes them with code.

use crate::stdlib;

/// One function: its path (`std::text::trim`), how it is written, and what it does.
#[derive(Debug)]
pub struct FunctionDoc {
    pub path: &'static str,
    pub signature: &'static str,
    pub summary: &'static str,
}

const fn f(path: &'static str, signature: &'static str, summary: &'static str) -> FunctionDoc {
    FunctionDoc {
        path,
        signature,
        summary,
    }
}

/// What a module is for, in the order the reference lists them.
pub const MODULES: &[(&str, &str)] = &[
    (
        "text",
        "Functions on `Text`. The same operations exist as methods (`s.trim()`).",
    ),
    ("string", "More functions on `Text`, and `join` for lists."),
    (
        "math",
        "Numbers. An `Int` argument gives an `Int` where that makes sense; a `Float` anywhere gives a `Float`.",
    ),
    (
        "iter",
        "Lists: `map`, `filter`, `fold` and the rest. Every function here is also a method of `List` (`xs.map(f)`).",
    ),
    (
        "map",
        "Building a `Map[K, V]`. The methods of a map (`insert`, `get`, ...) are listed under Methods.",
    ),
    (
        "option",
        "Function forms of the methods of `T?`. `o.map(f)` and `std.option.map(o, f)` are the same call.",
    ),
    (
        "result",
        "Function forms of the methods of `Result[T, E]`. `r.map(f)` and `std.result.map(r, f)` are the same call.",
    ),
    (
        "tensor",
        "Tensors. Shapes are checked before the program runs. Every function here is also a method (`a.matmul(b)`).",
    ),
    (
        "table",
        "Tables on Arrow. A function that reads data needs the row type written down: `let t: Table[Row] = ...`. Most are also methods of `Table`.",
    ),
    (
        "task",
        "`Task[T]`, what an `async fn` gives. `await` runs one; these run several.",
    ),
    ("time", "Clock and waiting."),
    ("cache", "A map with a size limit and an expiry."),
    ("random", "Random values from the operating system."),
    ("io", "Writing to the standard output."),
    ("fs", "Files. Needs the `fs.read` / `fs.write` permission."),
    ("env", "Environment variables. Needs the `env` permission."),
    (
        "metrics",
        "Counters the program makes, listed by the server's `/metrics`.",
    ),
    (
        "log",
        "Log lines in the format and at the level of `telvax serve`.",
    ),
    ("json", "JSON text."),
    ("test", "Assertions and mocks for `telvax test`."),
    ("debug", "Printing while a program is being written."),
    ("dist", "Running calls on other threads or machines."),
    ("http", "HTTP status values for `@endpoint` functions."),
    (
        "ai",
        "Embeddings and language models. Without `--live` they are offline stand-ins that need no network.",
    ),
    (
        "db",
        "A vector database. Without `--live` it is a store in memory.",
    ),
];

pub const FUNCTIONS: &[FunctionDoc] = &[
    // ---- text
    f(
        "std::text::clean_whitespace",
        "std.text.clean_whitespace(text: Text) -> Text",
        "Replaces every run of whitespace (spaces, tabs, new lines) with one space and removes it at both ends.",
    ),
    f(
        "std::text::trim",
        "std.text.trim(text: Text) -> Text",
        "Removes whitespace at both ends.",
    ),
    f(
        "std::text::lowercase",
        "std.text.lowercase(text: Text) -> Text",
        "The text in lower case.",
    ),
    f(
        "std::text::uppercase",
        "std.text.uppercase(text: Text) -> Text",
        "The text in upper case.",
    ),
    f(
        "std::text::truncate",
        "std.text.truncate(text: Text, n: Int) -> Text",
        "The first `n` characters. A shorter text is returned whole.",
    ),
    f(
        "std::text::normalize",
        "std.text.normalize(text: Text, form: Text) -> Text",
        "Writes the text in a Unicode normalization form: `\"NFC\"`, `\"NFD\"`, `\"NFKC\"` or `\"NFKD\"`. Any other `form` is an error. `\"NFKD\"` splits an accented letter into the letter and a combining mark.",
    ),
    f(
        "std::text::strip_accents",
        "std.text.strip_accents(text: Text) -> Text",
        "The text without accents and other combining marks: `\"ação\"` becomes `\"acao\"`.",
    ),
    f(
        "std::text::remove_chars",
        "std.text.remove_chars(text: Text, chars: Text) -> Text",
        "The text without every character that appears in `chars`.",
    ),
    f(
        "std::text::matches",
        "std.text.matches(text: Text, pattern: Text) -> Bool",
        "Whether the pattern (a regular expression) matches anywhere in the text. An invalid pattern is an error.",
    ),
    f(
        "std::text::find",
        "std.text.find(text: Text, pattern: Text) -> Text?",
        "The first part of the text that the pattern matches, or `None`.",
    ),
    f(
        "std::text::find_all",
        "std.text.find_all(text: Text, pattern: Text) -> List[Text]",
        "Every part of the text that the pattern matches, left to right, without overlaps.",
    ),
    f(
        "std::text::replace_all",
        "std.text.replace_all(text: Text, pattern: Text, with: Text) -> Text",
        "The text with every match replaced by `with`, where `$1` is the first group and `${name}` a named group.",
    ),
    f(
        "std::text::split",
        "std.text.split(text: Text, sep: Text) -> List[Text]",
        "Cuts the text at every `sep`. An empty `sep` is an error.",
    ),
    f(
        "std::text::to_text",
        "std.text.to_text(value) -> Text",
        "Any value as text, the way `print` shows it.",
    ),
    f(
        "std::text::format",
        "std.text.format(value, spec: Text) -> Text",
        "Formats a value with a format spec, the part after `:` in an f-string: `.2f`, `>8`, `05d`, `+.1e`, `,`.",
    ),
    // ---- string
    f(
        "std::string::join",
        "std.string.join(items: List[T], sep: Text) -> Text",
        "Shows every item as text and joins them with `sep`.",
    ),
    f(
        "std::string::slice",
        "std.string.slice(text: Text, start: Int, end: Int) -> Text",
        "The characters from `start` up to, not including, `end`. A negative index counts from the end; an index past the end stops at the end; `start` after `end` gives an empty text.",
    ),
    f(
        "std::string::repeat",
        "std.string.repeat(text: Text, n: Int) -> Text",
        "The text `n` times. A negative `n` is an error.",
    ),
    // ---- math
    f(
        "std::math::pow",
        "std.math.pow(base, exp)",
        "`base` to the power `exp`. Two integers give an `Int` (an overflow or a negative exponent is an error: write `2.0 ** -1`); otherwise a `Float`.",
    ),
    f(
        "std::math::floor_div",
        "std.math.floor_div(a, b)",
        "Division rounded down, so `floor_div(-7, 2)` is `-4`. Division by zero is an error.",
    ),
    f(
        "std::math::min",
        "std.math.min(a, b)",
        "The smaller of two numbers, or of two `Text` values (compared character by character).",
    ),
    f(
        "std::math::max",
        "std.math.max(a, b)",
        "The larger of two numbers, or of two `Text` values (compared character by character).",
    ),
    f(
        "std::math::abs",
        "std.math.abs(x)",
        "The absolute value. The smallest `Int` has none, which is an error.",
    ),
    f(
        "std::math::sqrt",
        "std.math.sqrt(x) -> Float",
        "The square root.",
    ),
    f(
        "std::math::floor",
        "std.math.floor(x) -> Float",
        "The largest whole number not above `x`, as a `Float`.",
    ),
    f(
        "std::math::ceil",
        "std.math.ceil(x) -> Float",
        "The smallest whole number not below `x`, as a `Float`.",
    ),
    f(
        "std::math::round",
        "std.math.round(x) -> Float",
        "The nearest whole number, as a `Float`. Halves round away from zero.",
    ),
    f(
        "std::math::to_float",
        "std.math.to_float(x) -> Float",
        "A number as a `Float`.",
    ),
    f(
        "std::math::to_int",
        "std.math.to_int(x) -> Int",
        "A number as an `Int`, dropping the fraction (toward zero). `NaN` and infinity are errors.",
    ),
    // ---- iter
    f(
        "std::iter::map",
        "std.iter.map(items: List[T], f: (T) -> U) -> List[U]",
        "A list of `f` applied to every item.",
    ),
    f(
        "std::iter::filter",
        "std.iter.filter(items: List[T], keep: (T) -> Bool) -> List[T]",
        "The items for which `keep` is true, in order.",
    ),
    f(
        "std::iter::any",
        "std.iter.any(items: List[T], f: (T) -> Bool) -> Bool",
        "Whether `f` is true for at least one item.",
    ),
    f(
        "std::iter::all",
        "std.iter.all(items: List[T], f: (T) -> Bool) -> Bool",
        "Whether `f` is true for every item.",
    ),
    f(
        "std::iter::find",
        "std.iter.find(items: List[T], f: (T) -> Bool) -> T?",
        "The first item for which `f` is true, or `None`.",
    ),
    f(
        "std::iter::for_each",
        "std.iter.for_each(items: List[T], f: (T) -> ())",
        "Calls `f` on every item, in order.",
    ),
    f(
        "std::iter::fold",
        "std.iter.fold(items: List[T], init: A, f: (A, T) -> A) -> A",
        "Starts with `init` and replaces it with `f(acc, item)` for every item.",
    ),
    f(
        "std::iter::enumerate",
        "std.iter.enumerate(items: List[T]) -> List[(Int, T)]",
        "Pairs every item with its position, counting from 0.",
    ),
    f(
        "std::iter::zip",
        "std.iter.zip(a: List[T], b: List[U]) -> List[(T, U)]",
        "Pairs the items of two lists. It stops at the end of the shorter one.",
    ),
    f(
        "std::iter::sum",
        "std.iter.sum(items: List[N]) -> N",
        "The sum of a list of numbers: 0 for an empty list, a `Float` if any item is one. An `Int` overflow is an error.",
    ),
    f(
        "std::iter::min",
        "std.iter.min(items: List[T]) -> T?",
        "The smallest number or text, or `None` for an empty list.",
    ),
    f(
        "std::iter::max",
        "std.iter.max(items: List[T]) -> T?",
        "The largest number or text, or `None` for an empty list.",
    ),
    f(
        "std::iter::skip",
        "std.iter.skip(items: List[T], n: Int) -> List[T]",
        "The list without its first `n` items.",
    ),
    f(
        "std::iter::take",
        "std.iter.take(items: List[T], n: Int) -> List[T]",
        "At most the first `n` items.",
    ),
    f(
        "std::iter::count",
        "std.iter.count(items: List[T]) -> u64",
        "The number of items.",
    ),
    f(
        "std::iter::slice",
        "std.iter.slice(items: List[T], start: Int, end: Int) -> List[T]",
        "The items from `start` up to, not including, `end`, with the index rules of `std.string.slice`.",
    ),
    f(
        "std::iter::concat",
        "std.iter.concat(a: List[T], b: List[T]) -> List[T]",
        "A new list with the items of `a` and then those of `b`.",
    ),
    f(
        "std::iter::range",
        "std.iter.range(end: Int) -> List[Int]\nstd.iter.range(start: Int, end: Int) -> List[Int]",
        "The integers from `start` (0 if omitted) up to, not including, `end`.",
    ),
    f(
        "std::collect",
        "std.collect(items: List[T]) -> List[T]",
        "Returns the list it is given.",
    ),
    // ---- map
    f(
        "std::map::new",
        "std.map.new() -> Map[K, V]",
        "An empty map. The key and value types come from the annotation: `let m: Map[Text, Int] = {}`.",
    ),
    f(
        "std::map::from_entries",
        "std.map.from_entries(entries: List[(K, V)]) -> Map[K, V]",
        "A map from `(key, value)` pairs. A key is a `Text`, an integer, a `Bool` or an enum without data; a later pair replaces an earlier one with the same key.",
    ),
    // ---- option
    f(
        "std::option::map",
        "std.option.map(o: T?, f: (T) -> U) -> U?",
        "Function form of the `Option` method `map`.",
    ),
    f(
        "std::option::and_then",
        "std.option.and_then(o: T?, f: (T) -> U?) -> U?",
        "Function form of the `Option` method `and_then`.",
    ),
    f(
        "std::option::filter",
        "std.option.filter(o: T?, keep: (T) -> Bool) -> T?",
        "Function form of the `Option` method `filter`.",
    ),
    f(
        "std::option::ok_or",
        "std.option.ok_or(o: T?, error: E) -> Result[T, E]",
        "Function form of the `Option` method `ok_or`.",
    ),
    f(
        "std::option::unwrap_or_else",
        "std.option.unwrap_or_else(o: T?, f: () -> T) -> T",
        "Function form of the `Option` method `unwrap_or_else`.",
    ),
    f(
        "std::option::expect",
        "std.option.expect(o: T?, message: Text) -> T",
        "Function form of the `Option` method `expect`.",
    ),
    // ---- result
    f(
        "std::result::is_ok",
        "std.result.is_ok(r: Result[T, E]) -> Bool",
        "Function form of the `Result` method `is_ok`.",
    ),
    f(
        "std::result::is_err",
        "std.result.is_err(r: Result[T, E]) -> Bool",
        "Function form of the `Result` method `is_err`.",
    ),
    f(
        "std::result::unwrap",
        "std.result.unwrap(r: Result[T, E]) -> T",
        "Function form of the `Result` method `unwrap`.",
    ),
    f(
        "std::result::expect",
        "std.result.expect(r: Result[T, E], message: Text) -> T",
        "Function form of the `Result` method `expect`.",
    ),
    f(
        "std::result::unwrap_or",
        "std.result.unwrap_or(r: Result[T, E], default: T) -> T",
        "Function form of the `Result` method `unwrap_or`.",
    ),
    f(
        "std::result::unwrap_or_else",
        "std.result.unwrap_or_else(r: Result[T, E], f: (E) -> T) -> T",
        "Function form of the `Result` method `unwrap_or_else`.",
    ),
    f(
        "std::result::map",
        "std.result.map(r: Result[T, E], f: (T) -> U) -> Result[U, E]",
        "Function form of the `Result` method `map`.",
    ),
    f(
        "std::result::map_err",
        "std.result.map_err(r: Result[T, E], f: (E) -> F) -> Result[T, F]",
        "Function form of the `Result` method `map_err`.",
    ),
    f(
        "std::result::and_then",
        "std.result.and_then(r: Result[T, E], f: (T) -> Result[U, E]) -> Result[U, E]",
        "Function form of the `Result` method `and_then`.",
    ),
    f(
        "std::result::ok",
        "std.result.ok(r: Result[T, E]) -> T?",
        "Function form of the `Result` method `ok`.",
    ),
    f(
        "std::result::err",
        "std.result.err(r: Result[T, E]) -> E?",
        "Function form of the `Result` method `err`.",
    ),
    // ---- tensor
    f(
        "std::tensor::zeros",
        "std.tensor.zeros() -> Tensor[T, dims]",
        "A tensor of zeros. The element type and shape come from the annotation: `let t: Tensor[f32, 2, 3] = std.tensor.zeros()`.",
    ),
    f(
        "std::tensor::from_vec",
        "std.tensor.from_vec(values: List[T]) -> Tensor[T, dims]",
        "A tensor with the values of a list. The shape comes from the annotation, and the number of values must fit it.",
    ),
    f(
        "std::tensor::reshape",
        "std.tensor.reshape(t: Tensor) -> Tensor[T, dims]",
        "The same elements in another shape, written in the annotation. The number of elements must not change; a mismatch that can be seen before running is a compile error.",
    ),
    f(
        "std::tensor::to_vec",
        "std.tensor.to_vec(t: Tensor[T, dims]) -> List[T]",
        "The elements, row by row, as a list.",
    ),
    f(
        "std::tensor::add",
        "std.tensor.add(a: Tensor, b: Tensor) -> Tensor",
        "Element-wise sum. The two shapes and element types must agree.",
    ),
    f(
        "std::tensor::sub",
        "std.tensor.sub(a: Tensor, b: Tensor) -> Tensor",
        "Element-wise difference. The two shapes and element types must agree.",
    ),
    f(
        "std::tensor::mul",
        "std.tensor.mul(a: Tensor, b: Tensor) -> Tensor",
        "Element-wise product (not a matrix product: see `matmul`).",
    ),
    f(
        "std::tensor::div",
        "std.tensor.div(a: Tensor, b: Tensor) -> Tensor",
        "Element-wise quotient.",
    ),
    f(
        "std::tensor::scale",
        "std.tensor.scale(t: Tensor, k: Float) -> Tensor",
        "Every element times `k`.",
    ),
    f(
        "std::tensor::relu",
        "std.tensor.relu(t: Tensor) -> Tensor",
        "Replaces negative elements with 0.",
    ),
    f(
        "std::tensor::sigmoid",
        "std.tensor.sigmoid(t: Tensor) -> Tensor",
        "`1 / (1 + e^-x)` for every element.",
    ),
    f(
        "std::tensor::softmax",
        "std.tensor.softmax(t: Tensor) -> Tensor",
        "Turns every row (the last axis) into probabilities that sum to 1.",
    ),
    f(
        "std::tensor::sum",
        "std.tensor.sum(t: Tensor[T, dims]) -> T",
        "The sum of all elements.",
    ),
    f(
        "std::tensor::mean",
        "std.tensor.mean(t: Tensor[T, dims]) -> T",
        "The mean of all elements.",
    ),
    f(
        "std::tensor::argmax",
        "std.tensor.argmax(t: Tensor) -> u64",
        "The position of the largest element when the tensor is read row by row. The first one wins a tie.",
    ),
    f(
        "std::tensor::dot",
        "std.tensor.dot(a: Tensor[T, N], b: Tensor[T, N]) -> T",
        "The dot product of two vectors of the same length.",
    ),
    f(
        "std::tensor::matmul",
        "std.tensor.matmul(a: Tensor[T, M, K], b: Tensor[T, K, N]) -> Tensor[T, M, N]\nstd.tensor.matmul(a: Tensor[T, M, K], v: Tensor[T, K]) -> Tensor[T, M]",
        "The matrix product. The inner sizes `K` must agree; a mismatch is a compile error.",
    ),
    f(
        "std::tensor::transpose",
        "std.tensor.transpose(t: Tensor[T, M, N]) -> Tensor[T, N, M]",
        "Swaps the two axes of a matrix.",
    ),
    // ---- table
    f(
        "std::table::read_csv",
        "std.table.read_csv(path: Text) -> Table[Row]",
        "Reads a CSV file with a header line. The columns are read in the order of the fields of `Row`. An empty cell in a column that is not `T?` is an error. Needs `fs.read`.",
    ),
    f(
        "std::table::read_parquet",
        "std.table.read_parquet(path: Text) -> Table[Row]",
        "Reads a Parquet file. The columns are matched to the fields of `Row` by name and converted to their types. Not in `telvax-lite`. Needs `fs.read`.",
    ),
    f(
        "std::table::read_ipc",
        "std.table.read_ipc(path: Text) -> Table[Row]",
        "Reads an Arrow IPC file (Feather v2). The file is memory-mapped, so a file with one batch and no compression is not copied. Do not rewrite the file while the table is in use. Needs `fs.read`.",
    ),
    f(
        "std::table::read_flight",
        "std.table.read_flight(url: Text, ticket: Text) -> Table[Row]",
        "Fetches a table from an Arrow Flight server (`DoGet`). `url` is `grpc://host:port` or `grpc+tls://host:port`. Only in builds with the `flight` feature. Needs `net`.",
    ),
    f(
        "std::table::read_csv_batches",
        "std.table.read_csv_batches(path: Text, n: Int) -> List[Table[Row]]",
        "Reads a CSV file into tables of at most `n` rows, without building one joined copy. All of them are in memory. `n` must be at least 1. Needs `fs.read`.",
    ),
    f(
        "std::table::read_parquet_batches",
        "std.table.read_parquet_batches(path: Text, n: Int) -> List[Table[Row]]",
        "Like `read_csv_batches`, for a Parquet file. Not in `telvax-lite`.",
    ),
    f(
        "std::table::read_ipc_batches",
        "std.table.read_ipc_batches(path: Text, n: Int) -> List[Table[Row]]",
        "Like `read_csv_batches`, for an Arrow IPC file. The chunks share the memory-mapped file.",
    ),
    f(
        "std::table::sql",
        "std.table.sql(t: Table[Row], query: Text) -> Table[Out]",
        "Runs a SQL query on the table, which is named `t` in the query. The result type is written in the annotation and its fields are matched to the columns by name. Not in `telvax-lite`.",
    ),
    f(
        "std::table::from_rows",
        "std.table.from_rows(rows: List[Row]) -> Table[Row]",
        "A table from a list of values of the row type.",
    ),
    f(
        "std::table::to_rows",
        "std.table.to_rows(t: Table[Row]) -> List[Row]",
        "The rows as a list of values of the row type.",
    ),
    f(
        "std::table::len",
        "std.table.len(t: Table[Row]) -> u64",
        "The number of rows.",
    ),
    f(
        "std::table::write_csv",
        "std.table.write_csv(t: Table[Row], path: Text)",
        "Writes a CSV file with a header line. Needs `fs.write`.",
    ),
    f(
        "std::table::write_parquet",
        "std.table.write_parquet(t: Table[Row], path: Text)",
        "Writes a Parquet file. Not in `telvax-lite`. Needs `fs.write`.",
    ),
    f(
        "std::table::write_ipc",
        "std.table.write_ipc(t: Table[Row], path: Text)",
        "Writes an Arrow IPC file (Feather v2), which PyArrow, Polars and pandas read. Needs `fs.write`.",
    ),
    f(
        "std::table::write_flight",
        "std.table.write_flight(t: Table[Row], url: Text, path: Text)",
        "Sends the table to an Arrow Flight server (`DoPut`) under `path`. Only in builds with the `flight` feature. Needs `net`.",
    ),
    f(
        "std::table::head",
        "std.table.head(t: Table[Row], n: Int) -> Table[Row]",
        "The first `n` rows (all of them if there are fewer). `n` must not be negative.",
    ),
    f(
        "std::table::tail",
        "std.table.tail(t: Table[Row], n: Int) -> Table[Row]",
        "The last `n` rows.",
    ),
    f(
        "std::table::sort_by",
        "std.table.sort_by(t: Table[Row], column: Text) -> Table[Row]",
        "The rows ordered by a column, smallest first. A name that is not a column is an error that lists the columns.",
    ),
    f(
        "std::table::sort_by_desc",
        "std.table.sort_by_desc(t: Table[Row], column: Text) -> Table[Row]",
        "The rows ordered by a column, largest first.",
    ),
    f(
        "std::table::concat",
        "std.table.concat(a: Table[Row], b: Table[Row]) -> Table[Row]",
        "The rows of `a` and then those of `b`. Both need the same row type.",
    ),
    f(
        "std::table::select",
        "std.table.select(t: Table[Row], columns: List[Text]) -> Table[{...}]",
        "Only the named columns, in the order given. The names are written as a list of text in place, and the result row is worked out from them, so no annotation is needed. A name that is not a column, or one named twice, is an error.",
    ),
    f(
        "std::table::group_by",
        "std.table.group_by(t: Table[Row], keys: List[Text], aggregations: List[Text]) -> Table[{...}]",
        "One row per distinct value of the key columns, in order of first appearance, then one column `<column>_<function>` for each aggregation written `column:function`: `sum` (`Int` for an integer column, `Float` for a decimal one), `mean` (`Float`), `min` and `max` (the column type) and `count` (`Int`, any column). Everything is written as text in lists in place, so the result row is worked out from them and needs no annotation. An unknown column or function, a column typed `T?` and a result column named twice are errors.",
    ),
    f(
        "std::table::split",
        "std.table.split(t: Table[Row], n: Int) -> List[Table[Row]]",
        "Cuts the table into tables of at most `n` rows that share its buffers, so nothing is copied. `n` must be at least 1.",
    ),
    f(
        "std::table::filter",
        "std.table.filter(t: Table[Row], keep: (Row) -> Bool) -> Table[Row]",
        "The rows for which `keep` is true.",
    ),
    // ---- task
    f(
        "std::task::gather",
        "std.task.gather(tasks: List[Task[T]]) -> Task[List[T]]",
        "A task that runs all of them at the same time and gives their values in the same order.",
    ),
    f(
        "std::task::run",
        "std.task.run(task: Task[T]) -> T",
        "Runs a task and waits for its value, which is what `await` does.",
    ),
    f(
        "std::task::spawn",
        "std.task.spawn(task: Task[T], on_error: (Text) -> () = ...)",
        "Starts a task and returns at once. The task goes on after the function that started it has returned, so an `@endpoint` can answer `202` and finish later. Its value is dropped. If it fails, `on_error` is called with the message (a job can then be marked as failed); without `on_error` the message is written to the standard error. At most 256 run at once. In `telvax run` and `telvax test` it waits for the task, as `run` does.",
    ),
    f(
        "std::task::sleep",
        "std.task.sleep(ms: Int) -> Task[()]",
        "A task that finishes after `ms` milliseconds.",
    ),
    // ---- time
    f(
        "std::time::sleep",
        "std.time.sleep(ms: Int)",
        "Waits `ms` milliseconds, blocking the thread.",
    ),
    f(
        "std::time::now",
        "std.time.now() -> Float",
        "The seconds since 1970-01-01 00:00 UTC, with a fraction.",
    ),
    // ---- io
    f(
        "std::io::print",
        "std.io.print(text: Text)",
        "Writes the text to the standard output, without a new line.",
    ),
    f(
        "std::io::println",
        "std.io.println(text: Text)",
        "Writes the text and a new line. This is the `print` that needs no `std.` prefix.",
    ),
    // ---- fs
    f(
        "std::fs::read",
        "std.fs.read(path: Text) -> Result[Text, Text]",
        "The content of a file as text, or the reason it could not be read. Needs `fs.read`.",
    ),
    f(
        "std::fs::embed",
        "std.fs.embed(path: Text) -> Text",
        "The content of a file, read when the program is built and kept inside it. The path is a text literal, relative to the directory of the main file. Needs no capability.",
    ),
    f(
        "std::fs::write",
        "std.fs.write(path: Text, text: Text) -> Result[(), Text]",
        "Writes a text to a file, replacing it. Needs `fs.write`.",
    ),
    f(
        "std::fs::exists",
        "std.fs.exists(path: Text) -> Bool",
        "Whether the path exists. Needs `fs.read`.",
    ),
    // ---- cache
    f(
        "std::cache::new",
        "std.cache.new(max: Int, ttl_seconds: Float) -> Cache",
        "A cache that holds at most `max` entries and forgets an entry `ttl_seconds` after it was set. When it is full, `set` drops the entry that was used least recently. It is shared by every request like a top-level `Map`: each call is atomic. `max` and `ttl_seconds` must be above 0. The values are not type checked yet: `get` gives an `Option` of whatever was stored.",
    ),
    // ---- random
    f(
        "std::random::uuid",
        "std.random.uuid() -> Text",
        "A random version 4 UUID as 32 lower case hexadecimal digits, from the operating system's random source. Fit for ids that must not be guessed. Needs no capability.",
    ),
    f(
        "std::random::int",
        "std.random.int(low: Int, high: Int) -> Int",
        "A random whole number from `low` to `high`, both included, all equally likely. `low` above `high` is an error. Needs no capability.",
    ),
    // ---- metrics
    f(
        "std::metrics::count",
        "std.metrics.count(name: Text, labels: Map[Text, Text] = {}, by: Int = 1)",
        "Adds `by` (1 if left out) to the counter called `name`. Without labels the server lists it in `/metrics` as `telvax_counter_total{name=\"...\"}` when it runs with `--metrics`; with labels, as `name{label=\"value\"}` with its own `# TYPE`, so `std.metrics.count(\"jobs_total\", {\"status\": \"done\"})` is `jobs_total{status=\"done\"}`. A name with labels is letters, digits, `_` and `:` and does not start with a digit, label names the same without `:`; at most 1,000 label sets per name. Needs no capability.",
    ),
    // ---- log
    f(
        "std::log::debug",
        "std.log.debug(message: Text, fields: Map[Text, Text] = {})",
        "Writes a line at the level `debug`, for what helps while a program is being written. The line goes to the standard error. With `--log-format json` it is `{\"ts\": ..., \"level\": ..., \"message\": ..., <fields>}`; otherwise (and under `telvax run` and `telvax test`) it is `level message key=value`. A field called `ts`, `level` or `message` is left out. A line below `--log-level` (`info` by default) is not written. Needs no capability.",
    ),
    f(
        "std::log::info",
        "std.log.info(message: Text, fields: Map[Text, Text] = {})",
        "Writes a line at the level `info`, for what happened in the normal way. The line goes to the standard error. With `--log-format json` it is `{\"ts\": ..., \"level\": ..., \"message\": ..., <fields>}`; otherwise (and under `telvax run` and `telvax test`) it is `level message key=value`. A field called `ts`, `level` or `message` is left out. A line below `--log-level` (`info` by default) is not written. Needs no capability.",
    ),
    f(
        "std::log::warn",
        "std.log.warn(message: Text, fields: Map[Text, Text] = {})",
        "Writes a line at the level `warn`, for what went wrong but did not stop the work. The line goes to the standard error. With `--log-format json` it is `{\"ts\": ..., \"level\": ..., \"message\": ..., <fields>}`; otherwise (and under `telvax run` and `telvax test`) it is `level message key=value`. A field called `ts`, `level` or `message` is left out. A line below `--log-level` (`info` by default) is not written. Needs no capability.",
    ),
    f(
        "std::log::error",
        "std.log.error(message: Text, fields: Map[Text, Text] = {})",
        "Writes a line at the level `error`, for what failed. The line goes to the standard error. With `--log-format json` it is `{\"ts\": ..., \"level\": ..., \"message\": ..., <fields>}`; otherwise (and under `telvax run` and `telvax test`) it is `level message key=value`. A field called `ts`, `level` or `message` is left out. A line below `--log-level` (`info` by default) is not written. Needs no capability.",
    ),
    // ---- env
    f(
        "std::env::get",
        "std.env.get(name: Text) -> Text?",
        "The value of an environment variable, or `None` if it is not set. Needs `env`.",
    ),
    // ---- json
    f(
        "std::json::stringify",
        "std.json.stringify(value) -> Text",
        "The value as JSON text. `None` is `null` and `Some(x)` is just `x`.",
    ),
    f(
        "std::json::parse",
        "std.json.parse(text: Text) -> Result[T, Text]",
        "Reads JSON into the type written in the annotation: `let q: Result[Query, Text] = std.json.parse(body)`. The error says what did not fit.",
    ),
    // ---- test
    f(
        "std::test::assert",
        "std.test.assert(condition: Bool)",
        "Fails the test, with `assertion failed`, if the condition is false.",
    ),
    f(
        "std::test::assert_eq",
        "std.test.assert_eq(left, right)",
        "Fails the test, showing both values, if they are not equal. Comparing two different types is a compile error.",
    ),
    f(
        "std::test::assert_msg",
        "std.test.assert_msg(condition: Bool, message: Text)",
        "Like `assert`, with your message.",
    ),
    f(
        "std::test::fail",
        "std.test.fail(message: Text) -> Never",
        "Fails the test with the message.",
    ),
    f(
        "std::test::mock",
        "std.test.mock(target, replacement)",
        "Replaces a function with another of the same type until `reset_mocks`. `target` is the function, or the path of a std function as `Text`.",
    ),
    f(
        "std::test::calls",
        "std.test.calls(target) -> Int",
        "How many times a mocked function was called. A function that was not mocked is an error.",
    ),
    f(
        "std::test::reset_mocks",
        "std.test.reset_mocks()",
        "Removes every mock.",
    ),
    // ---- debug
    f(
        "std::debug::dbg",
        "std.debug.dbg(value: T, label: Text) -> T",
        "Writes `label = value` to the standard error and returns the value, so it can be put in the middle of an expression.",
    ),
    // ---- dist
    f(
        "std::dist::map",
        "std.dist.map(items: List[T], work: (T) -> U) -> List[U]",
        "Like `std.iter.map`, but `work` must be the name of a top-level function. It runs on local threads, or on the machines of `--workers`, or on Ray from Python.",
    ),
    // ---- http
    f(
        "std::http::Status::BadRequest",
        "std.http.Status.BadRequest(message: Text) -> Status",
        "An HTTP 400. Returning it as the `Err` of an `@endpoint` function answers with that status.",
    ),
    f(
        "std::http::Status::Unauthorized",
        "std.http.Status.Unauthorized(message: Text) -> Status",
        "An HTTP 401.",
    ),
    f(
        "std::http::Status::Forbidden",
        "std.http.Status.Forbidden(message: Text) -> Status",
        "An HTTP 403.",
    ),
    f(
        "std::http::Status::NotFound",
        "std.http.Status.NotFound(message: Text) -> Status",
        "An HTTP 404.",
    ),
    f(
        "std::http::Status::Unprocessable",
        "std.http.Status.Unprocessable(message: Text) -> Status",
        "An HTTP 422.",
    ),
    f(
        "std::http::Response::new",
        "std.http.Response.new(body: T) -> Response",
        "An answer with status 200, no headers and `body` written as JSON.",
    ),
    f(
        "std::http::Response::events",
        "std.http.Response.events(producer: fn(Sink)) -> Response",
        "An answer of `text/event-stream`. The server calls `producer` while the client reads; each `sink.send(...)` is one event. `.status` and `.header` work as for any `Response`.",
    ),
    f(
        "std::http::Status::Internal",
        "std.http.Status.Internal(message: Text) -> Status",
        "An HTTP 500.",
    ),
    // ---- ai
    f(
        "std::ai::embeddings::OllamaEmbedder::new",
        "std.ai.embeddings.OllamaEmbedder.new(model: Text) -> OllamaEmbedder",
        "An embedder for a model such as `nomic-embed-text`. With `--live` it asks an Ollama server (needs `net`); otherwise a deterministic stand-in makes the vector.",
    ),
    f(
        "std::ai::llm::OpenAIClient::new",
        "std.ai.llm.OpenAIClient.new(model: Text) -> OpenAIClient",
        "A client for a language model. With `--live` it calls the OpenAI API (needs `net`); otherwise it answers with a fixed stand-in text that quotes the question.",
    ),
    // ---- db
    f(
        "std::db::postgres::PostgresClient::connect",
        "std.db.postgres.PostgresClient.connect(url: Text) -> PostgresClient",
        "A SQL client for a PostgreSQL server. The connection is opened by the first statement. Needs `--live` and the `net` permission.",
    ),
    f(
        "std::db::pgvector::PgVectorClient::connect",
        "std.db.pgvector.PgVectorClient.connect(url: Text) -> PgVectorClient",
        "A vector store. With `--live` and a `postgres://` address it uses PostgreSQL with pgvector (needs `net`); otherwise it is a store in memory.",
    ),
];

/// What a type of `std` is.
pub const TYPES: &[(&str, &str)] = &[
    (
        "std::http::Request",
        "The request an `@endpoint` function is answering. Give the function a parameter of this type to read it; it is not part of the payload.",
    ),
    (
        "std::http::Response",
        "An answer with a status and headers, for an `@endpoint` function that returns more than a body with status 200: `Ok(Response.new(body).status(202).header(\"location\", url))`. The body is written as JSON.",
    ),
    (
        "std::cache::Cache",
        "Made by `std.cache.new(max, ttl_seconds)`. Keys are `Text`. `get` and `set` count as a use of the key; an entry older than the ttl is not returned.",
    ),
    (
        "std::http::Sink",
        "What the producer of `Response.events` receives. `send(data)` writes one event, `event(name, data)` a named one and `comment(text)` a comment.",
    ),
    (
        "std::db::postgres::PostgresClient",
        "Made by `PostgresClient.connect(url)`. Its methods `execute` and `query` run SQL on a PostgreSQL server. Needs `--live` and the `net` permission.",
    ),
    (
        "std::http::Status",
        "An HTTP status with a message. The functions that make one are `std.http.Status.BadRequest` and the others listed under `http`.",
    ),
    (
        "std::ai::embeddings::OllamaEmbedder",
        "Made by `OllamaEmbedder.new(model)`. Its method `embed(text)` gives a `Tensor[f32, 768]`.",
    ),
    (
        "std::ai::llm::OpenAIClient",
        "Made by `OpenAIClient.new(model)`. Its method `complete(prompt)` gives the answer as `Text`.",
    ),
    (
        "std::db::pgvector::PgVectorClient",
        "Made by `PgVectorClient.connect(url)`. Its methods store vectors with `insert` and build a search with `search_collection`, `with_vector` and `limit`; `execute` runs it and gives rows with `id`, `text` and `score`. The most similar come first; the default limit is 10.",
    ),
    (
        "std::db::pgvector::VectorQuery",
        "A search being built: the value of `search_collection`, `with_vector` and `limit`, and the argument of `execute`.",
    ),
];

/// The methods of the types of `std` (the ones in `stdlib.rs`'s `METHODS`).
pub const STD_TYPE_METHODS: &[(&str, &str, &str, &str)] = &[
    (
        "OllamaEmbedder",
        "embed",
        "(text: Text) -> Tensor[f32, 768]",
        "The embedding of the text.",
    ),
    (
        "OpenAIClient",
        "complete",
        "(prompt: Text) -> Text",
        "The model's answer to the prompt.",
    ),
    (
        "Response",
        "status",
        "(code: Int) -> Response",
        "The same answer with this status, from 100 to 599. Any other value is an HTTP 500.",
    ),
    (
        "Response",
        "header",
        "(name: Text, value: Text) -> Response",
        "The same answer with this header; it replaces one of the same name, `content-type` included.",
    ),
    (
        "Cache",
        "get",
        "(key: Text) -> V?",
        "The value of the key, or `None` if it is not there or has expired. Counts as a use of the key.",
    ),
    (
        "Cache",
        "set",
        "(key: Text, value: V)",
        "Stores the value, replacing the old one. If the cache is full, the entry used least recently is dropped.",
    ),
    (
        "Cache",
        "remove",
        "(key: Text) -> V?",
        "Removes the key and gives its value, or `None`.",
    ),
    (
        "Cache",
        "clear",
        "() -> Int",
        "Removes every entry and says how many there were.",
    ),
    (
        "Cache",
        "len",
        "() -> Int",
        "How many entries there are (an expired one counts until it is met).",
    ),
    (
        "Sink",
        "send",
        "(data: Text)",
        "Writes one event. A `Text` is sent as it is, any other value as JSON. A text with several lines becomes several `data:` lines. If the client has gone, this is an error that ends the producer.",
    ),
    (
        "Sink",
        "event",
        "(name: Text, data: Text)",
        "Writes an event with a name (an `event:` line), such as `done`.",
    ),
    (
        "Sink",
        "comment",
        "(text: Text)",
        "Writes a comment line (starts with `:`). Clients ignore it; it keeps the connection open.",
    ),
    (
        "Request",
        "header",
        "(name: Text) -> Text?",
        "The first value of the header, or `None`. The name is not case sensitive.",
    ),
    (
        "Request",
        "method",
        "() -> Text",
        "The HTTP method, in capitals: `GET`, `POST`, ...",
    ),
    (
        "Request",
        "path",
        "() -> Text",
        "The path of the URL, as it was sent, without the query string.",
    ),
    (
        "PostgresClient",
        "execute",
        "(sql: Text, params) -> Result[Int, Text]",
        "Runs a statement and gives the number of rows it changed. `params` fill `$1`, `$2`, ... and are never written into the SQL: a list when they are of one type (`[\"a\", \"b\"]`), a tuple when not (`(\"done\", 3)`). `None` is `NULL`.",
    ),
    (
        "PostgresClient",
        "query",
        "(sql: Text, params) -> Result[List[Row], Text]",
        "Runs a query and gives its rows as a list of a `type` of the program, which must be written down: `let rows: List[Job] = db.query(sql, [id])?`. A field is read from the column of the same name; `T?` allows `NULL`. `json` and `jsonb` come as `Text`; a `timestamptz`, `numeric` or `uuid` column must be cast in the SQL (`created_at::text`).",
    ),
    (
        "PgVectorClient",
        "insert",
        "(collection: Text, id: Text, text: Text, vector: Tensor[f32, 768])",
        "Stores a document with its vector in a collection.",
    ),
    (
        "PgVectorClient",
        "search_collection",
        "(collection: Text) -> VectorQuery",
        "Starts a search in a collection.",
    ),
    (
        "PgVectorClient",
        "with_vector",
        "(vector: Tensor[f32, 768]) -> VectorQuery",
        "On a query: sets the vector to search for. A search needs one.",
    ),
    (
        "PgVectorClient",
        "limit",
        "(n: Int) -> VectorQuery",
        "On a query: sets how many rows to return; the default is 10.",
    ),
    (
        "PgVectorClient",
        "execute",
        "() -> List",
        "On a query: runs the search. The rows have `id`, `text` and `score`, the most similar first.",
    ),
];

/// What the built-in methods do: (type, method, summary). The signatures are in `stdlib.rs`.
pub const METHODS: &[(&str, &str, &str)] = &[
    ("Text", "is_empty", "Whether the text has no characters."),
    (
        "Text",
        "len",
        "The length in bytes (a letter with an accent is more than one).",
    ),
    ("Text", "trim", "Removes whitespace at both ends."),
    ("Text", "to_lowercase", "The text in lower case."),
    ("Text", "to_uppercase", "The text in upper case."),
    ("Text", "contains", "Whether `part` appears in the text."),
    (
        "Text",
        "starts_with",
        "Whether the text begins with `prefix`.",
    ),
    ("Text", "ends_with", "Whether the text ends with `suffix`."),
    ("Text", "replace", "Every `from` replaced by `to`."),
    (
        "Text",
        "split",
        "Cuts the text at every `sep`. An empty `sep` is an error.",
    ),
    (
        "Text",
        "find",
        "The byte position of the first `part`, or `None`.",
    ),
    (
        "Text",
        "to_int",
        "The text as an `Int`, ignoring whitespace at the ends, or `None` if it is not one.",
    ),
    (
        "Text",
        "to_float",
        "The text as a `Float`, ignoring whitespace at the ends, or `None`.",
    ),
    ("Text", "chars", "The characters, each as a `Text`."),
    ("Text", "lines", "The lines, without their line breaks."),
    (
        "Text",
        "slice",
        "The characters from `start` up to `end`; see `std.string.slice`.",
    ),
    ("List", "is_empty", "Whether the list has no items."),
    ("List", "len", "The number of items."),
    (
        "List",
        "push",
        "Adds an item at the end. Lists are shared: every name for the list sees it.",
    ),
    (
        "List",
        "contains",
        "Whether an item equal to `x` is in the list.",
    ),
    ("List", "first", "The first item, or `None`."),
    ("List", "last", "The last item, or `None`."),
    (
        "List",
        "pop",
        "Removes and returns the last item, or `None`.",
    ),
    (
        "List",
        "get",
        "The item at position `i`, or `None` if there is none. A negative `i` gives `None`.",
    ),
    (
        "List",
        "sort",
        "Orders the list in place, smallest first. The items must be numbers, text or `Bool`.",
    ),
    ("List", "reverse", "Reverses the list in place."),
    ("List", "map", "See `std.iter.map`."),
    ("List", "filter", "See `std.iter.filter`."),
    ("List", "take", "See `std.iter.take`."),
    ("List", "skip", "See `std.iter.skip`."),
    ("List", "count", "How many items make `f` true."),
    ("List", "any", "See `std.iter.any`."),
    ("List", "all", "See `std.iter.all`."),
    ("List", "find", "See `std.iter.find`."),
    ("List", "for_each", "See `std.iter.for_each`."),
    ("List", "fold", "See `std.iter.fold`."),
    ("List", "enumerate", "See `std.iter.enumerate`."),
    ("List", "zip", "See `std.iter.zip`."),
    ("List", "sum", "See `std.iter.sum`."),
    ("List", "min", "See `std.iter.min`."),
    ("List", "max", "See `std.iter.max`."),
    ("List", "join", "See `std.string.join`."),
    ("List", "slice", "See `std.iter.slice`."),
    ("Option", "is_some", "Whether there is a value."),
    ("Option", "is_none", "Whether there is no value."),
    ("Option", "unwrap", "The value. `None` is an error."),
    ("Option", "unwrap_or", "The value, or `default` for `None`."),
    (
        "Option",
        "unwrap_or_else",
        "The value, or the result of `f()` for `None`.",
    ),
    (
        "Option",
        "expect",
        "The value. `None` is an error with your message.",
    ),
    ("Option", "map", "`Some(f(value))`, or `None` for `None`."),
    (
        "Option",
        "and_then",
        "`f(value)`, which is itself an option, or `None` for `None`.",
    ),
    (
        "Option",
        "filter",
        "The option itself if there is a value and `f(value)` is true, otherwise `None`.",
    ),
    (
        "Option",
        "ok_or",
        "`Ok(value)`, or `Err(error)` for `None`.",
    ),
    ("Result", "is_ok", "Whether it is an `Ok`."),
    ("Result", "is_err", "Whether it is an `Err`."),
    (
        "Result",
        "unwrap",
        "The value of an `Ok`. An `Err` is an error.",
    ),
    (
        "Result",
        "expect",
        "The value of an `Ok`. An `Err` is an error with your message.",
    ),
    (
        "Result",
        "unwrap_or",
        "The value of an `Ok`, or `default` for an `Err`.",
    ),
    (
        "Result",
        "unwrap_or_else",
        "The value of an `Ok`, or `f(error)` for an `Err`.",
    ),
    (
        "Result",
        "map",
        "Applies `f` to the value of an `Ok`; an `Err` stays as it is.",
    ),
    (
        "Result",
        "map_err",
        "Applies `f` to the error of an `Err`; an `Ok` stays as it is.",
    ),
    (
        "Result",
        "and_then",
        "For an `Ok`, `f(value)`, which is itself a `Result` with the same error type; an `Err` stays as it is.",
    ),
    (
        "Result",
        "ok",
        "`Some(value)` for an `Ok`, otherwise `None`.",
    ),
    (
        "Result",
        "err",
        "`Some(error)` for an `Err`, otherwise `None`.",
    ),
    (
        "Map",
        "insert",
        "Sets the value of a key, replacing the old one. Maps are shared like lists.",
    ),
    ("Map", "get", "The value of a key, or `None`."),
    (
        "Map",
        "remove",
        "Removes a key and returns its value, or `None`.",
    ),
    ("Map", "contains_key", "Whether the key is in the map."),
    ("Map", "len", "The number of entries."),
    ("Map", "is_empty", "Whether the map has no entries."),
    ("Map", "keys", "The keys, in key order."),
    ("Map", "values", "The values, in key order."),
    ("Map", "entries", "The `(key, value)` pairs, in key order."),
    ("Table", "len", "The number of rows."),
    ("Table", "is_empty", "Whether the table has no rows."),
    ("Table", "sql", "See `std.table.sql`."),
    ("Table", "filter", "See `std.table.filter`."),
    ("Table", "head", "See `std.table.head`."),
    ("Table", "tail", "See `std.table.tail`."),
    ("Table", "sort_by", "See `std.table.sort_by`."),
    ("Table", "sort_by_desc", "See `std.table.sort_by_desc`."),
    ("Table", "concat", "See `std.table.concat`."),
    ("Table", "split", "See `std.table.split`."),
    ("Table", "select", "See `std.table.select`."),
    ("Table", "group_by", "See `std.table.group_by`."),
    ("Tensor", "add", "See `std.tensor.add`."),
    ("Tensor", "sub", "See `std.tensor.sub`."),
    ("Tensor", "mul", "See `std.tensor.mul`."),
    ("Tensor", "div", "See `std.tensor.div`."),
    ("Tensor", "scale", "See `std.tensor.scale`."),
    ("Tensor", "relu", "See `std.tensor.relu`."),
    ("Tensor", "sigmoid", "See `std.tensor.sigmoid`."),
    ("Tensor", "softmax", "See `std.tensor.softmax`."),
    ("Tensor", "sum", "See `std.tensor.sum`."),
    ("Tensor", "mean", "See `std.tensor.mean`."),
    ("Tensor", "argmax", "See `std.tensor.argmax`."),
    ("Tensor", "dot", "See `std.tensor.dot`."),
    ("Tensor", "matmul", "See `std.tensor.matmul`."),
    ("Tensor", "transpose", "See `std.tensor.transpose`."),
    ("Tensor", "reshape", "See `std.tensor.reshape`."),
    ("Tensor", "to_vec", "See `std.tensor.to_vec`."),
];

/// The built-in method tables, by the name the reference uses for the type.
pub fn method_tables() -> [(&'static str, &'static [stdlib::BuiltinMethod]); 7] {
    use crate::ty::{Dim, Prim, Ty};
    let t = |ty: &Ty| stdlib::builtin_methods(ty);
    let unknown = || Box::new(Ty::Unknown);
    [
        ("Text", t(&Ty::Prim(Prim::Str))),
        ("List", t(&Ty::Vec(unknown()))),
        ("Option", t(&Ty::Option(unknown()))),
        ("Result", t(&Ty::Result(unknown(), unknown()))),
        ("Map", t(&Ty::Map(unknown(), unknown()))),
        ("Table", t(&Ty::Table(String::new()))),
        (
            "Tensor",
            t(&Ty::Tensor {
                elem: unknown(),
                shape: vec![Dim::Lit(1)],
            }),
        ),
    ]
}

fn dotted(path: &str) -> String {
    path.replace("::", ".")
}

/// The module of a function path: `std::text::trim` is `text`. `std::collect` has no module and is
/// listed with the iterators.
fn module_of(path: &str) -> &str {
    let mut parts = path.split("::").skip(1);
    let first = parts.next().unwrap_or("");
    if parts.next().is_none() {
        "iter"
    } else {
        first
    }
}

/// The whole reference, as Markdown.
pub fn reference_markdown() -> String {
    let mut out = String::new();
    out.push_str("# Standard library\n\n");
    out.push_str(
        "Every function and method that the language provides, with its type and what it does. \
Functions are called by their path (`std.text.trim(s)`), or after an `import`, by the name. Many also work \
as methods (`s.trim()`) or in a pipeline (`s |> std.text.trim()`). \
A parameter written `Table[Row]`, `Tensor` or `T` stands for any type of that kind.\n\n",
    );
    out.push_str(
        "This page is written by `reference_markdown` in `compiler/semantic/src/stdlib_docs.rs` and checked by a \
test: a function without an entry there, or a page that is out of date, fails the build.\n\n",
    );

    out.push_str("## Functions\n\n");
    for (module, intro) in MODULES {
        let docs: Vec<&FunctionDoc> = FUNCTIONS
            .iter()
            .filter(|d| module_of(d.path) == *module)
            .collect();
        if docs.is_empty() {
            continue;
        }
        out.push_str(&format!("### `{module}`\n\n{intro}\n\n"));
        for d in docs {
            out.push_str(&format!(
                "#### `{}`\n\n```text\n{}\n```\n\n{}\n\n",
                dotted(d.path),
                d.signature,
                d.summary
            ));
        }
    }
    let rest: Vec<&FunctionDoc> = FUNCTIONS
        .iter()
        .filter(|d| !MODULES.iter().any(|(m, _)| module_of(d.path) == *m))
        .collect();
    if !rest.is_empty() {
        out.push_str("### Other\n\n");
        for d in rest {
            out.push_str(&format!(
                "#### `{}`\n\n```text\n{}\n```\n\n{}\n\n",
                dotted(d.path),
                d.signature,
                d.summary
            ));
        }
    }

    out.push_str("## Types\n\n");
    for (path, summary) in TYPES {
        out.push_str(&format!("### `{}`\n\n{summary}\n\n", dotted(path)));
        let mut any = false;
        for (ty, name, sig, text) in STD_TYPE_METHODS {
            if path.rsplit("::").next() == Some(*ty) {
                if !any {
                    out.push_str("| Method | What it does |\n|---|---|\n");
                    any = true;
                }
                out.push_str(&format!("| `{name}{sig}` | {text} |\n"));
            }
        }
        if any {
            out.push('\n');
        }
    }

    out.push_str("## Methods\n\n");
    out.push_str(
        "The methods the language gives to its own types. `T`, `U`, `E` stand for any type.\n\n",
    );
    for (ty, table) in method_tables() {
        out.push_str(&format!(
            "### `{ty}`\n\n| Method | What it does |\n|---|---|\n"
        ));
        for (name, sig) in table {
            let text = METHODS
                .iter()
                .find(|(t, n, _)| t == &ty && n == name)
                .map_or("", |(_, _, s)| *s);
            out.push_str(&format!("| `{name}{sig}` | {text} |\n"));
        }
        out.push('\n');
    }
    out
}
