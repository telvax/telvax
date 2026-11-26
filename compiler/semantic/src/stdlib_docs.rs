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
    (
        "string",
        "Deprecated: its functions are in `std.text` (`std.text.join`, `slice`, `repeat`). Edition 2027-preview removes it, and `telvax migrate` rewrites a program.",
    ),
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
    (
        "random",
        "Random values from the operating system (`uuid`, `int`), and `Generator`, a generator with a seed that gives the same numbers every time.",
    ),
    (
        "model",
        "Saving a trained model (its weights and its fitted preprocessing, any struct of them) to a safetensors file, and loading it back as the type the program names. Loading never runs anything from the file.",
    ),
    (
        "ml",
        "What goes around a model on a table: train/test splits, metrics, and preprocessing steps fitted on the training rows only (`Scaler`, `LabelEncoder`, `Features`). The numbers are scikit-learn's.",
    ),
    (
        "sparse",
        "`Sparse`, a matrix of `f32` where only the values that are not zero are stored (compressed sparse rows), for text features and other wide data.",
    ),
    ("io", "Writing to the standard output."),
    ("fs", "Files. Needs the `fs.read` / `fs.write` permission."),
    (
        "path",
        "Paths as text: join, split and compare them. They only read the text, never the disk, and need no permission. A path is read as the operating system does (`/` on Linux and macOS).",
    ),
    (
        "env",
        "Environment variables (`get`, which needs the `env` permission) and the words given to the program on the command line (`args`).",
    ),
    ("process", "The running program."),
    (
        "bytes",
        "`Bytes`: a sequence of bytes that does not change, for a file of any kind, a hash or a body. Made from a `Text` here, read from a file by `std.fs.read_bytes`, or by `std.encoding` and `std.crypto`.",
    ),
    (
        "encoding",
        "Bytes as text and back: hexadecimal and base64.",
    ),
    (
        "crypto",
        "Hashes on `Bytes`. They need no capability. For a `Text`, make the bytes with `std.bytes.from_text`.",
    ),
    (
        "plot",
        "Charts as SVG text: give the data and a title, get the image, and write it with `std.fs.write`. The same call gives the same text.",
    ),
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
    (
        "http",
        "HTTP status values for `@endpoint` functions, and `std.http.client`, to make requests: `get`, `head`, `delete`, `post` and `put`.",
    ),
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
    f(
        "std::text::join",
        "std.text.join(items: List[T], sep: Text) -> Text",
        "Shows every item as text and joins them with `sep`.",
    ),
    f(
        "std::text::slice",
        "std.text.slice(text: Text, start: Int, end: Int) -> Text",
        "The characters from `start` up to, not including, `end`. A negative index counts from the end; an index past the end stops at the end; `start` after `end` gives an empty text.",
    ),
    f(
        "std::text::repeat",
        "std.text.repeat(text: Text, n: Int) -> Text",
        "The text `n` times. A negative `n` is an error.",
    ),
    // ---- string
    f(
        "std::string::join",
        "std.string.join(items: List[T], sep: Text) -> Text",
        "Deprecated: write `std.text.join`, the same function. Edition 2027-preview removes `std.string`, and `telvax migrate` rewrites it.",
    ),
    f(
        "std::string::slice",
        "std.string.slice(text: Text, start: Int, end: Int) -> Text",
        "Deprecated: write `std.text.slice`, the same function. Edition 2027-preview removes `std.string`.",
    ),
    f(
        "std::string::repeat",
        "std.string.repeat(text: Text, n: Int) -> Text",
        "Deprecated: write `std.text.repeat`, the same function. Edition 2027-preview removes `std.string`.",
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
        "std::math::ln",
        "std.math.ln(x) -> Float",
        "The natural logarithm. It follows IEEE 754: `ln(0)` is `-inf` and the logarithm of a negative number is `NaN`; neither is an error.",
    ),
    f(
        "std::math::log2",
        "std.math.log2(x) -> Float",
        "The base 2 logarithm. `log2(0)` is `-inf` and a negative number gives `NaN`, as for `ln`.",
    ),
    f(
        "std::math::log10",
        "std.math.log10(x) -> Float",
        "The base 10 logarithm. `log10(0)` is `-inf` and a negative number gives `NaN`, as for `ln`.",
    ),
    f(
        "std::math::exp",
        "std.math.exp(x) -> Float",
        "`e` raised to `x`. It is `inf` when the result does not fit in a `Float`, not an error.",
    ),
    f(
        "std::math::sin",
        "std.math.sin(x) -> Float",
        "The sine of an angle in radians.",
    ),
    f(
        "std::math::cos",
        "std.math.cos(x) -> Float",
        "The cosine of an angle in radians.",
    ),
    f(
        "std::math::tan",
        "std.math.tan(x) -> Float",
        "The tangent of an angle in radians.",
    ),
    f(
        "std::math::atan2",
        "std.math.atan2(y, x) -> Float",
        "The angle in radians, from `-pi` to `pi`, of the point `(x, y)`: the arc tangent of `y / x` in the right quadrant. `atan2(0.0, 0.0)` is `0.0`.",
    ),
    f(
        "std::math::pi",
        "std.math.pi() -> Float",
        "The number pi, the ratio of a circle to its diameter (`3.141592653589793`).",
    ),
    f(
        "std::math::ln_gamma",
        "std.math.ln_gamma(x) -> Float",
        "The natural logarithm of the absolute value of the gamma function, so `ln_gamma(5)` is `ln(24)`. It does not overflow for large `x`, as `ln(gamma(x))` would. At `0` and the negative whole numbers, where the gamma function has a pole, it is `inf`. Accurate to about 1e-13.",
    ),
    f(
        "std::math::digamma",
        "std.math.digamma(x) -> Float",
        "The digamma function, the derivative of `ln_gamma`: `digamma(1)` is minus the Euler-Mascheroni constant. At `0` and the negative whole numbers it is `NaN`. Accurate to about 1e-13.",
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
        "The items from `start` up to, not including, `end`, with the index rules of `std.text.slice`.",
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
        "std::tensor::from_rows",
        "std.tensor.from_rows(rows: List[List[T]]) -> Tensor[T, R, C]",
        "A matrix with one row per list, when its size is only known as the program runs (`from_vec` takes the size from the annotation). Every row must have the same length. The elements are numbers: `Float` rows give `Tensor[Float, R, C]`, and an annotation `Tensor[f32, R, C]` makes them `f32`.",
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
        "std::tensor::ln",
        "std.tensor.ln(t: Tensor) -> Tensor",
        "The natural logarithm of every element, in the same shape. An element of 0 gives `-inf` and a negative one gives `NaN`, as `std.math.ln` does.",
    ),
    f(
        "std::tensor::exp",
        "std.tensor.exp(t: Tensor) -> Tensor",
        "`e` raised to every element, in the same shape.",
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
        "std::table::sample",
        "std.table.sample(t: Table[Row], n: Int, seed: Int) -> Table[Row]",
        "`n` rows of the table, chosen at random, none twice, in random order. The same `seed` gives the same rows on every machine and in every release (it is drawn with `std.random.Generator`). `n` from 0 to the number of rows; more is an error.",
    ),
    f(
        "std::table::with_column",
        "std.table.with_column(t: Table[Row], name: Text, f: (Row) -> V) -> Table",
        "The table with one more column. `name` is written in place as a text, it is not a column of `t` yet, and `f` is called with each row and gives the value, a number, a `Bool` or a `Text` (or an `Option` of one). The type of the rows of the result is worked out from `t` and `f`, as for `select`, so use the result as it is (`let u = t.with_column(\"polarity\", r => score(r.text))`) and read `u.polarity`.",
    ),
    f(
        "std::table::dropna",
        "std.table.dropna(t: Table[Row], columns: List[Text]) -> Table[Row]",
        "The rows that have a value in every one of the `columns`: a row is dropped when one of them is empty (a missing value) or, in a `Float` column, `NaN`. The row type does not change. A name that is not a column is an error that lists the columns. With an empty list nothing is dropped.",
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
    f(
        "std::time::format",
        "std.time.format(t: Float, pattern: Text) -> Text",
        "The time `t` (seconds since 1970-01-01 00:00 UTC, as `now` gives) written in UTC with a `strftime` pattern: `format(0.0, \"%Y%m%d-%H%M%S\")` is `\"19700101-000000\"`. A pattern with an unknown `%` code is an error. There is no local time zone and no date arithmetic.",
    ),
    f(
        "std::time::parse",
        "std.time.parse(text: Text, pattern: Text) -> Result[Float, Text]",
        "Reads a time written with a `strftime` pattern and gives it as seconds since 1970-01-01 00:00 UTC, the inverse of `format`. A pattern with only a date (`\"%Y-%m-%d\"`) is midnight. A pattern with a UTC offset (`%z`) is converted to UTC; without one the text is taken as UTC. The error says what could not be read.",
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
    f(
        "std::fs::create_dir",
        "std.fs.create_dir(path: Text) -> Result[(), Text]",
        "Creates a directory and the ones above it that are missing. It is not an error when the directory already exists. Needs `fs.write`.",
    ),
    f(
        "std::fs::list_dir",
        "std.fs.list_dir(path: Text) -> Result[List[Text], Text]",
        "The names of what is in a directory (files and directories, not the path), in alphabetical order, or the reason it could not be read. Needs `fs.read`.",
    ),
    f(
        "std::fs::walk",
        "std.fs.walk(path: Text) -> Result[List[Text], Text]",
        "The files under a directory, at any depth, as paths that start with `path`, in alphabetical order. Directories are not listed, only the files in them. Needs `fs.read`.",
    ),
    f(
        "std::fs::remove",
        "std.fs.remove(path: Text) -> Result[(), Text]",
        "Removes a file or an empty directory. A directory with something in it is an error, so it never removes a tree. Needs `fs.write`.",
    ),
    f(
        "std::fs::copy",
        "std.fs.copy(from: Text, to: Text) -> Result[(), Text]",
        "Copies a file to `to`, replacing it. The directory of `to` must exist. Needs `fs.read` and `fs.write`.",
    ),
    // ---- path
    f(
        "std::path::join",
        "std.path.join(path: Text, part: Text) -> Text",
        "`part` added to `path` with one `/` between them. `std.path` writes `/` on every system, Windows included (which reads it), so a program gives the same paths everywhere. When `part` is an absolute path, it replaces `path`. Join more than two with `join(join(a, b), c)`.",
    ),
    f(
        "std::path::parent",
        "std.path.parent(path: Text) -> Text?",
        "The path without its last part: `parent(\"a/b.txt\")` is `\"a\"` and `parent(\"b.txt\")` is `\"\"`. A root (`\"/\"`) or an empty path has none, so `None`.",
    ),
    f(
        "std::path::file_name",
        "std.path.file_name(path: Text) -> Text?",
        "The last part of the path: `file_name(\"a/b.txt\")` is `\"b.txt\"`. `None` when the path ends in `..` or is a root or empty.",
    ),
    f(
        "std::path::extension",
        "std.path.extension(path: Text) -> Text?",
        "The extension of the last part, without the dot: `extension(\"a/b.tar.gz\")` is `\"gz\"`. `None` when it has none, as for `\"a/b\"` and `\".hidden\"`.",
    ),
    f(
        "std::path::relative",
        "std.path.relative(path: Text, base: Text) -> Text?",
        "`path` seen from `base`: `relative(\"a/b/c.txt\", \"a\")` is `\"b/c.txt\"`. `None` when `path` is not under `base`. It compares parts, not characters, so `\"ab/c\"` is not under `\"a\"`, and it never reads the disk.",
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
    f(
        "std::fs::read_bytes",
        "std.fs.read_bytes(path: Text) -> Result[Bytes, Text]",
        "The content of a file as bytes, whatever it holds, or the reason it could not be read. Needs `fs.read`.",
    ),
    f(
        "std::fs::write_bytes",
        "std.fs.write_bytes(path: Text, data: Bytes) -> Result[(), Text]",
        "Writes the bytes to a file, replacing it. Needs `fs.write`.",
    ),
    f(
        "std::http::client::get",
        "std.http.client.get(url: Text)\nstd.http.client.get(url: Text, headers: Map[Text, Text]) -> Result[HttpResponse, Text]",
        "Sends a `GET` and gives the answer. Any status is an answer (`Ok`), a 404 or a 500 included: look at `status()` or `ok()`. Only a request that got no answer (the connection, a timeout of 30 seconds, a URL that is not one) is an `Err`, with the reason. A redirect is not followed: the answer is the 3xx with its `location` header. The body that is read is limited to 256 MB. Inside a request of `telvax serve` it sends a `traceparent` header with the trace of the request and a new span, so the service called can join the trace (a `traceparent` in `headers` is kept instead). Needs `net`, in every mode (`--live` or not), so a test replaces it with `std.test.mock`.",
    ),
    f(
        "std::http::client::head",
        "std.http.client.head(url: Text)\nstd.http.client.head(url: Text, headers: Map[Text, Text]) -> Result[HttpResponse, Text]",
        "Like `get`, with a `HEAD`: the headers and the status, no body. Needs `net`.",
    ),
    f(
        "std::http::client::delete",
        "std.http.client.delete(url: Text)\nstd.http.client.delete(url: Text, headers: Map[Text, Text]) -> Result[HttpResponse, Text]",
        "Like `get`, with a `DELETE`. Needs `net`.",
    ),
    f(
        "std::http::client::post",
        "std.http.client.post(url: Text, body: Text)\nstd.http.client.post(url: Text, body: Text, headers: Map[Text, Text]) -> Result[HttpResponse, Text]",
        "Like `get`, with a `POST`. The body is a `Text` (sent as its UTF-8 bytes) or `Bytes`; no `content-type` is added, so put one in `headers` when the server wants it. Needs `net`.",
    ),
    f(
        "std::http::client::put",
        "std.http.client.put(url: Text, body: Text)\nstd.http.client.put(url: Text, body: Text, headers: Map[Text, Text]) -> Result[HttpResponse, Text]",
        "Like `post`, with a `PUT`. Needs `net`.",
    ),
    f(
        "std::bytes::from_text",
        "std.bytes.from_text(text: Text) -> Bytes",
        "The UTF-8 bytes of a text. `to_text` is the way back.",
    ),
    f(
        "std::encoding::hex",
        "std.encoding.hex(data: Bytes) -> Text",
        "The bytes as lower case hexadecimal, two digits for each byte: `hex(from_text(\"hi\"))` is `\"6869\"`.",
    ),
    f(
        "std::encoding::from_hex",
        "std.encoding.from_hex(text: Text) -> Result[Bytes, Text]",
        "The bytes of hexadecimal text, in either case. An odd number of digits or a character that is not a digit is an error that says where.",
    ),
    f(
        "std::encoding::base64",
        "std.encoding.base64(data: Bytes) -> Text",
        "The bytes as base64 with the standard alphabet and `=` padding (RFC 4648): `base64(from_text(\"foo\"))` is `\"Zm9v\"`.",
    ),
    f(
        "std::encoding::from_base64",
        "std.encoding.from_base64(text: Text) -> Result[Bytes, Text]",
        "The bytes of base64 text with the standard alphabet and the padding. It is strict: a line break, a space, a missing or misplaced `=` or a character outside the alphabet is an error.",
    ),
    f(
        "std::crypto::sha256",
        "std.crypto.sha256(data: Bytes) -> Bytes",
        "The SHA-256 digest of the bytes, 32 bytes long. Use `std.encoding.hex` to write it as text. It is a hash, not a way to store a password.",
    ),
    f(
        "std::crypto::hmac_sha256",
        "std.crypto.hmac_sha256(key: Bytes, data: Bytes) -> Bytes",
        "The HMAC-SHA256 (RFC 2104) of `data` with `key`, 32 bytes long. The result is `Bytes`, so it can be the key of the next step, as a request signature needs.",
    ),
    f(
        "std::random::Generator::new",
        "std.random.Generator.new(seed: Int) -> Generator",
        "A random number generator that starts from `seed`: the same seed gives the same numbers, on every machine and in every release. See `Generator` for the algorithm. It is not made for secrets or ids that must not be guessed: use `uuid` for those. Needs no capability.",
    ),
    // ---- model
    f(
        "std::model::save",
        "std.model.save(value: T, path: Text) -> Result[(), Text]",
        "Writes `value` to a file in the safetensors format: the tensors as raw little-endian bytes, named by where they are in the value (`weights`, `layers[0].bias`, `words.indptr`), and the shape of the value in the metadata. A value can be made of numbers, `Text`, `Bool`, lists, tuples, options, structs, tensors of `f32`, `Float` or `Int`, `Sparse`, and the fitted steps of `std.ml` (`Features`, `Scaler`, `LabelEncoder`), so a struct that holds the preprocessing and the weights is saved as one file. Needs `fs.write`.",
    ),
    f(
        "std::model::load",
        "std.model.load(path: Text) -> Result[T, Text]",
        "Reads a file written by `save` as the type `T` the program asks for (`let m: Result[Model, Text] = std.model.load(path)`). A file of another type (another struct, a field more or less, a tensor of another element type or size), of a newer format, or damaged is an `Err` that says which; the type in the file is in the message. Loading parses the header and copies bytes, and runs nothing from the file. Needs `fs.read`.",
    ),
    f(
        "std::model::to_bytes",
        "std.model.to_bytes(value: T) -> Result[Bytes, Text]",
        "The bytes `save` would write, to send or store elsewhere.",
    ),
    f(
        "std::model::from_bytes",
        "std.model.from_bytes(b: Bytes) -> Result[T, Text]",
        "`load` from bytes instead of a file.",
    ),
    f(
        "std::model::describe",
        "std.model.describe(path: Text) -> Result[Text, Text]",
        "The type of the value in a model file, as text (`Model { features: Features, weights: Tensor[f32, 7, 1], ... }`), to log when a service starts. Needs `fs.read`.",
    ),
    // ---- ml
    f(
        "std::ml::split",
        "std.ml.split(t: Table[Row], test_size: Float, seed: Int) -> (Table[Row], Table[Row])",
        "The rows shuffled with the seed (`std.random.Generator`) and cut in two: the training rows and the test rows. `test_size` is the share of the test, above 0 and below 1, rounded up as scikit-learn's `train_test_split`, so 40 rows and 0.25 give 30 and 10. The same seed gives the same split everywhere; the rows are not those of scikit-learn for the same number. Both parts need a row.",
    ),
    f(
        "std::ml::split_stratified",
        "std.ml.split_stratified(t: Table[Row], column: Text, test_size: Float, seed: Int) -> (Table[Row], Table[Row])",
        "Like `split`, with each class of `column` cut in the same share (rounded to the nearest row), so a rare class is in both parts.",
    ),
    f(
        "std::ml::split_by_time",
        "std.ml.split_by_time(t: Table[Row], test_size: Float) -> (Table[Row], Table[Row])",
        "The last rows as the test, in the order of the table: sort it by time first (`sort_by`). Nothing is shuffled, so no row of the future is used to train.",
    ),
    f(
        "std::ml::k_fold",
        "std.ml.k_fold(n: Int, k: Int, seed: Int) -> List[(List[Int], List[Int])]",
        "The rows `0` to `n - 1` shuffled and cut in `k` folds, as scikit-learn's `KFold(k, shuffle=True)`: for each fold, the training rows and the test rows, each in increasing order, to give to `std.table.take`. The first `n % k` folds have one row more. At least 2 folds and a row for each.",
    ),
    f(
        "std::ml::mae",
        "std.ml.mae(y_true: List[Float] | Tensor[T, N], y_pred: List[Float] | Tensor[T, N]) -> Float",
        "The mean absolute error. Each argument is a list of numbers or a tensor of one dimension (a numeric column, the output of a model); they are as many.",
    ),
    f(
        "std::ml::mse",
        "std.ml.mse(y_true, y_pred) -> Float",
        "The mean squared error.",
    ),
    f(
        "std::ml::rmse",
        "std.ml.rmse(y_true, y_pred) -> Float",
        "The square root of the mean squared error.",
    ),
    f(
        "std::ml::r2",
        "std.ml.r2(y_true, y_pred) -> Float",
        "The coefficient of determination: 1 for a perfect prediction, 0 for one as good as the mean, below 0 for a worse one. For a constant `y_true`, 1 when it is predicted exactly and 0 otherwise, as scikit-learn.",
    ),
    f(
        "std::ml::accuracy",
        "std.ml.accuracy(y_true: List[T], y_pred: List[T]) -> Float",
        "The share of rows predicted right. The labels are a list or a tensor of one dimension, of any type that prints (a `Text`, an `Int`, a `Bool`).",
    ),
    f(
        "std::ml::precision",
        "std.ml.precision(y_true: List[T], y_pred: List[T], average: Text) -> Float",
        "Of the rows predicted as a class, the share that are of it, averaged over the classes as `macro` (each class the same), `weighted` (by how many rows each class has) or `micro` (over all the rows). A class never predicted counts 0, as scikit-learn's `zero_division=0`.",
    ),
    f(
        "std::ml::recall",
        "std.ml.recall(y_true: List[T], y_pred: List[T], average: Text) -> Float",
        "Of the rows of a class, the share predicted as it, averaged as for `precision`.",
    ),
    f(
        "std::ml::f1",
        "std.ml.f1(y_true: List[T], y_pred: List[T], average: Text) -> Float",
        "The harmonic mean of precision and recall of each class, averaged as for `precision` (`macro` is the macro-F1).",
    ),
    f(
        "std::ml::confusion_matrix",
        "std.ml.confusion_matrix(y_true: List[T], y_pred: List[T]) -> (List[Text], List[List[Int]])",
        "The classes in order (as numbers when they all are, otherwise as text) and the matrix: row `i`, column `j` counts the rows of class `i` predicted as class `j`.",
    ),
    f(
        "std::ml::classification_report",
        "std.ml.classification_report(y_true: List[T], y_pred: List[T]) -> Text",
        "The precision, recall, F1 and number of rows of each class, the accuracy and the macro and weighted averages, in the text of scikit-learn's `classification_report`.",
    ),
    f(
        "std::ml::roc_auc",
        "std.ml.roc_auc(y_true: List[Bool], scores: List[Float]) -> Float",
        "The area under the ROC curve: the chance that a positive row scores above a negative one, ties counting half. It needs both positive and negative rows.",
    ),
    f(
        "std::ml::Scaler::fit",
        "std.ml.Scaler.fit(values: List[Float]) -> Scaler",
        "Learns the mean and the standard deviation of the training values.",
    ),
    f(
        "std::ml::LabelEncoder::fit",
        "std.ml.LabelEncoder.fit(values: List[Text]) -> LabelEncoder",
        "Learns the classes of the training values, in order; a class becomes its position.",
    ),
    f(
        "std::ml::Features::fit",
        "std.ml.Features.fit(train: Table[Row], target: Text, numeric: List[Text], categorical: List[Text]) -> Features",
        "Learns, from the training rows only, how a row becomes the input of a model: each `numeric` column standardized with its training mean and deviation (a `Date` or `Timestamp` as seconds since 1970, a `Bool` as 1 or 0), then each `categorical` column one-hot with its training classes. `target` names the column the model predicts; giving it as a feature is an error, since the model would read its answer. A column with missing values is an error.",
    ),
    f(
        "std::table::take",
        "std.table.take(t: Table[Row], rows: List[Int]) -> Table[Row]",
        "The rows at those positions, in that order (the folds of `std.ml.k_fold`); a position may repeat. A position outside the table is an error.",
    ),
    f(
        "std::time::weekday",
        "std.time.weekday(t: Timestamp | Date | Float) -> Int",
        "The day of the week in UTC, Monday 0 to Sunday 6 (as pandas' `dayofweek`). `t` is a `Timestamp`, a `Date`, or seconds since 1970.",
    ),
    f(
        "std::time::hour",
        "std.time.hour(t: Timestamp | Date | Float) -> Int",
        "The hour of the day in UTC, 0 to 23.",
    ),
    f(
        "std::time::epoch",
        "std.time.epoch(t: Timestamp | Date | Float) -> Float",
        "Seconds since 1970-01-01 UTC, the number a model reads for a time.",
    ),
    // ---- sparse
    f(
        "std::sparse::from_rows",
        "std.sparse.from_rows(rows: List[List[(Int, Float)]], cols: Int) -> Sparse",
        "A matrix with one row per item of `rows`, each a list of `(column, value)` in any order, and `cols` columns. Values given twice for one place are added; zeros are not stored. A column from `cols` on is an error.",
    ),
    f(
        "std::sparse::from_triplets",
        "std.sparse.from_triplets(rows: Int, cols: Int, triplets: List[(Int, Int, Float)]) -> Sparse",
        "A `rows` by `cols` matrix from `(row, column, value)` in any order. Values given twice for one place are added; a place outside the matrix is an error.",
    ),
    f(
        "std::sparse::from_dense",
        "std.sparse.from_dense(t: Tensor[T, R, C]) -> Sparse",
        "The values of a tensor of 2 dimensions that are not zero, as `f32`.",
    ),
    // ---- metrics
    f(
        "std::metrics::count",
        "std.metrics.count(name: Text, labels: Map[Text, Text] = {}, by: Int = 1)",
        "Adds `by` (1 if left out) to the counter called `name`. The server lists it in `/metrics` when it runs with `--metrics`, under its own name with its own `# TYPE`: `name`, or `name{label=\"value\"}` with labels, so `std.metrics.count(\"jobs_total\", {\"status\": \"done\"})` is `jobs_total{status=\"done\"}`. A name is letters, digits, `_` and `:` and does not start with a digit (anything else is an error, because `/metrics` lists the name as it is), label names the same without `:`; at most 1,000 label sets per name. Needs no capability.",
    ),
    f(
        "std::metrics::set",
        "std.metrics.set(name: Text, value: Float, labels: Map[Text, Text] = {})",
        "Sets the gauge called `name` to `value`, an `Int` or a `Float`: a number that goes up and down, such as the entries of a cache. The last call wins. The server lists it in `/metrics` when it runs with `--metrics`, under its own name with `# TYPE name gauge`: `name value`, or `name{label=\"value\"} value` with labels. A value that is not finite is listed as `NaN`, `+Inf` or `-Inf`. The name and the labels follow the rules of `std.metrics.count`, with at most 1,000 label sets per name, and a name is a counter or a gauge, not both (using one for the other is an error). Needs no capability.",
    ),
    // ---- log
    f(
        "std::log::debug",
        "std.log.debug(message: Text, fields: Map[Text, Text] = {})",
        "Writes a line at the level `debug`, for what helps while a program is being written. The line goes to the standard error. With `--log-format json` it is `{\"ts\": ..., \"level\": ..., \"message\": ..., <fields>}`; otherwise (and under `telvax run` and `telvax test`) it is `level message key=value`. A field called `ts`, `level` or `message` is left out. Inside a request of `telvax serve` the line also has the `trace_id` of the request and its `request_id`, unless the fields have their own. A line below `--log-level` (`info` by default) is not written. Needs no capability.",
    ),
    f(
        "std::log::info",
        "std.log.info(message: Text, fields: Map[Text, Text] = {})",
        "Writes a line at the level `info`, for what happened in the normal way. The line goes to the standard error. With `--log-format json` it is `{\"ts\": ..., \"level\": ..., \"message\": ..., <fields>}`; otherwise (and under `telvax run` and `telvax test`) it is `level message key=value`. A field called `ts`, `level` or `message` is left out. Inside a request of `telvax serve` the line also has the `trace_id` of the request and its `request_id`, unless the fields have their own. A line below `--log-level` (`info` by default) is not written. Needs no capability.",
    ),
    f(
        "std::log::warn",
        "std.log.warn(message: Text, fields: Map[Text, Text] = {})",
        "Writes a line at the level `warn`, for what went wrong but did not stop the work. The line goes to the standard error. With `--log-format json` it is `{\"ts\": ..., \"level\": ..., \"message\": ..., <fields>}`; otherwise (and under `telvax run` and `telvax test`) it is `level message key=value`. A field called `ts`, `level` or `message` is left out. Inside a request of `telvax serve` the line also has the `trace_id` of the request and its `request_id`, unless the fields have their own. A line below `--log-level` (`info` by default) is not written. Needs no capability.",
    ),
    f(
        "std::log::error",
        "std.log.error(message: Text, fields: Map[Text, Text] = {})",
        "Writes a line at the level `error`, for what failed. The line goes to the standard error. With `--log-format json` it is `{\"ts\": ..., \"level\": ..., \"message\": ..., <fields>}`; otherwise (and under `telvax run` and `telvax test`) it is `level message key=value`. A field called `ts`, `level` or `message` is left out. Inside a request of `telvax serve` the line also has the `trace_id` of the request and its `request_id`, unless the fields have their own. A line below `--log-level` (`info` by default) is not written. Needs no capability.",
    ),
    // ---- env
    f(
        "std::plot::bar",
        "std.plot.bar(labels: List[Text], values: List[Float], title: Text) -> Text",
        "A bar chart: one bar for each label, with a grid and the numbers of the vertical axis, 640 by 400. `labels` and `values` have the same length and `values` are finite numbers (a negative one hangs below the zero line); a label longer than 10 characters is cut for the axis. An empty list is an error.",
    ),
    f(
        "std::plot::line",
        "std.plot.line(xs: List[Float], ys: List[Float], title: Text) -> Text",
        "One series as a line, `ys` against `xs`, with a marker on each point when there are 60 or fewer. The points are joined in the order of `xs`. For dates, use the seconds of a time. `xs` and `ys` have the same length and are finite numbers. An empty list is an error.",
    ),
    f(
        "std::plot::lines",
        "std.plot.lines(xs: List[Float], series: Map[Text, List[Float]], title: Text) -> Text",
        "Several series against the same `xs`, each in its own color, with a legend of their names; the series are drawn in the order of their names. Each series has as many values as `xs`. The daily average of two scores, for instance: `lines(days, {\"textblob\": a, \"vader\": b}, \"Sentiment\")`.",
    ),
    f(
        "std::plot::density",
        "std.plot.density(values: List[Float], bins: Int, title: Text) -> Text",
        "A histogram scaled so that its area is 1, with the kernel density estimate drawn over it: a Gaussian kernel with the bandwidth of Scott's rule (the standard deviation times `n^(-1/5)`), as seaborn's `histplot(kde=True)` and SciPy's `gaussian_kde` use by default. `bins` from 1 to 1000.",
    ),
    f(
        "std::plot::box",
        "std.plot.box(groups: Map[Text, List[Float]], title: Text) -> Text",
        "A box plot for each group, in the order of their names: the box from the first to the third quartile (linear interpolation, as NumPy's default), a line at the median, whiskers to the furthest values within 1.5 times the height of the box (Tukey), and the values beyond them as circles. Every group needs a value.",
    ),
    f(
        "std::plot::axis_titles",
        "std.plot.axis_titles(svg: Text, x: Text, y: Text) -> Text",
        "A chart of `std.plot` with a title under the horizontal axis and one beside the vertical axis: `axis_titles(bar(...), \"region\", \"sales\")`. An empty title is left out.",
    ),
    f(
        "std::plot::histogram",
        "std.plot.histogram(values: List[Float], bins: Int, title: Text) -> Text",
        "How the `values` are spread: `bins` bars (from 1 to 1000) of equal width between the smallest value and the largest, each as high as the number of values in it; the largest value is in the last bar. The values are finite numbers and there is at least one.",
    ),
    f(
        "std::env::args",
        "std.env.args() -> List[Text]",
        "The words after the function name in `telvax run app.tvx main ...`, as they were written (the flags that `run` itself reads, such as `--allow`, are not in the list). It is empty when the program is not started by `telvax run`. Needs no permission.",
    ),
    f(
        "std::process::exit",
        "std.process.exit(code: Int) -> Never",
        "Ends the program at once with the status `code`, from 0 to 255 (0 is success). The output written so far is flushed. It does not return, so it can end any branch. A `main` that returns an `Err` ends the program with status 1 and the error on the standard error without it.",
    ),
    f(
        "std::env::get",
        "std.env.get(name: Text) -> Text?",
        "The value of an environment variable, or `None` if it is not set. Needs `env`.",
    ),
    // ---- json
    f(
        "std::json::stringify",
        "std.json.stringify(value) -> Text\nstd.json.stringify(value, indent: Int) -> Text",
        "The value as JSON text. `None` is `null` and `Some(x)` is just `x`. With `indent`, the text has one item per line, indented by that many spaces per level, as `json.dumps(value, indent=2)` of Python does; `indent` 0 or less gives the same compact text as without it.",
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
        "std::http::Response::text",
        "std.http.Response.text(body: Text) -> Response",
        "An answer with status 200 whose body is the text as it is, not written as JSON, with `content-type: text/plain; charset=utf-8`. `.status` and `.header` work as for any `Response`; a `.header(\"content-type\", ...)` replaces the content type.",
    ),
    f(
        "std::http::Response::html",
        "std.http.Response.html(body: Text) -> Response",
        "The same as `text`, with `content-type: text/html; charset=utf-8`: a page.",
    ),
    f(
        "std::http::Response::bytes",
        "std.http.Response.bytes(body: Bytes, content_type: Text) -> Response",
        "An answer with status 200 whose body is the bytes as they are, with the given `content-type`: an image, a PDF, a file to download (add `.header(\"content-disposition\", \"attachment; filename=report.csv\")`). An empty content type is an error.",
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
        "std::http::Upload",
        "A file of a `multipart/form-data` request, from `req.uploads()`.",
    ),
    (
        "std::http::Response",
        "An answer with a status and headers, for an `@endpoint` function that returns more than a body with status 200: `Ok(Response.new(body).status(202).header(\"location\", url))`. The body is written as JSON.",
    ),
    (
        "std::http::client::HttpResponse",
        "What `std.http.client` gives back: the status, the headers and the body. The body is kept as bytes, so a page, an image or a file are all fine: `text()` reads it as UTF-8 text and `bytes()` gives it as it came.",
    ),
    (
        "std::bytes::Bytes",
        "Made by `std.bytes.from_text`, `std.fs.read_bytes`, `std.encoding` or `std.crypto`. A sequence of bytes that never changes: `slice` and `concat` give new ones. Printing one shows its length (`Bytes(3)`), not its content: use `std.encoding.hex` to see it.",
    ),
    (
        "std::ml::Scaler",
        "Made by `std.ml.Scaler.fit(values)`: the mean and the population standard deviation of the training values, as scikit-learn's `StandardScaler` (a deviation of 0 scales by 1). `apply` uses them on other values, so the test rows are scaled with what the training rows gave.",
    ),
    (
        "std::ml::LabelEncoder",
        "Made by `std.ml.LabelEncoder.fit(values)`: the classes of the training values, in order.",
    ),
    (
        "std::ml::Features",
        "Made by `std.ml.Features.fit(train, target, numeric, categorical)`: everything learned from the training rows that turns a row into the input of a model. Because it is one value, the same scaling and encoding go wherever the model goes: an endpoint calls `transform` on its request, so a prediction cannot skip the preprocessing. `transform` gives the columns in the order of `names()`: the numeric ones, then one per class of each categorical one (`region=north`). A class not seen in training gives zeros, as scikit-learn's `OneHotEncoder(handle_unknown=\"ignore\")`.",
    ),
    (
        "std::sparse::Sparse",
        "Made by `std.sparse.from_rows`, `from_triplets` or `from_dense`. A matrix of `f32` in compressed sparse row form: each row keeps its values that are not zero, in order of column, so memory grows with the number of those and not with rows times columns. It does not change: each method gives a new matrix. The shape is not part of the type, so the sizes of `matvec` and `matmul` are checked when they run. The products, sums and scaling run in native code.",
    ),
    (
        "std::random::Generator",
        "Made by `std.random.Generator.new(seed)`. The algorithm is documented and fixed: xoshiro256** with its four 64-bit words filled by splitmix64 from the bits of the seed; `float` is the top 53 bits of a draw divided by 2^53; a whole number below `n` is drawn by Lemire's multiply-and-reject method, so all are equally likely; `shuffle` and `sample` are a Fisher-Yates shuffle from the front. Changing any of this would change what a program prints for a seed, so it is a breaking change. The numbers are not those of NumPy: a program is reproducible within Telvax. A `Generator` is a handle: the places that hold it draw from one sequence, and two calls at once are taken one after the other.",
    ),
    (
        "std::cache::Cache",
        "Made by `std.cache.new(max, ttl_seconds)`. Keys are `Text`. `get` and `set` count as a use of the key; an entry older than the ttl is not returned.",
    ),
    (
        "std::http::Next",
        "The rest of the chain, as the second parameter of a `@middleware` function: `next(req)` runs the next middleware and the route and gives their `Response`, which the middleware may change (`.header(...)`, `.status(...)`) or return as it is. A middleware that does not call `next` answers with its own `Response` and the route does not run. `next` can be called once.",
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
        "Made by `OpenAIClient.new(model)`. `complete(prompt)` gives the answer to one prompt as `Text`; `chat(messages)` and `stream(messages, on_delta)` hold a conversation. The methods `with_system`, `temperature`, `max_tokens`, `stop`, `with_key` and `with_base_url` give a new client with that option, so a client can be set up once and shared. Live, it talks to any OpenAI-compatible server: `OPENAI_BASE_URL` or `with_base_url` (a local model, Gemini's OpenAI-compatible endpoint, ...).",
    ),
    (
        "std::ai::llm::LlmReply",
        "What `chat` and `stream` give: the text of the answer, why the model stopped, and the tokens it counted.",
    ),
    (
        "std::ai::llm::LlmError",
        "Why `chat` or `stream` failed, as a `kind` a program can test (`if e.kind() == \"rate_limit\"`): `invalid_key`, `model_not_found`, `rate_limit` (with `retry_after`), `refused` (the content filter), `timeout`, `server`, `network` or `bad_request`, and a `message` for people.",
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
        "chat",
        "(messages: List[(Text, Text)]) -> Result[LlmReply, LlmError]",
        "Sends the conversation, a list of `(role, text)` with the roles `system`, `user` and `assistant` (the earlier turns are the memory), and gives the whole answer. The system prompt of `with_system` goes first. Offline it gives a fixed text that quotes the last message of the user. Needs `net` when live.",
    ),
    (
        "OpenAIClient",
        "stream",
        "(messages: List[(Text, Text)], on_delta: (Text) -> ()) -> Result[LlmReply, LlmError]",
        "Like `chat`, and calls `on_delta` with each piece of the answer as it comes, so a service can write it to the `Sink` of `Response.events` with no buffer. The whole answer is given at the end. An error of `on_delta` stops it.",
    ),
    (
        "OpenAIClient",
        "with_system",
        "(prompt: Text) -> OpenAIClient",
        "A client with this system prompt, sent first with each conversation.",
    ),
    (
        "OpenAIClient",
        "temperature",
        "(t: Float) -> OpenAIClient",
        "A client with this sampling temperature, from 0 to 2.",
    ),
    (
        "OpenAIClient",
        "max_tokens",
        "(n: Int) -> OpenAIClient",
        "A client whose answers are at most `n` tokens long.",
    ),
    (
        "OpenAIClient",
        "stop",
        "(words: List[Text]) -> OpenAIClient",
        "A client whose answers end before any of these texts.",
    ),
    (
        "OpenAIClient",
        "with_key",
        "(key: Text) -> OpenAIClient",
        "A client with this API key instead of `OPENAI_API_KEY`: the key a visitor gives, for instance. Without any key a live call is an `invalid_key` error.",
    ),
    (
        "OpenAIClient",
        "with_base_url",
        "(url: Text) -> OpenAIClient",
        "A client that talks to this OpenAI-compatible server instead of `OPENAI_BASE_URL` (`http://localhost:11434/v1` for Ollama).",
    ),
    ("LlmReply", "text", "() -> Text", "The answer."),
    (
        "LlmReply",
        "stop_reason",
        "() -> Text",
        "Why the model stopped: `stop` (it was done), `length` (`max_tokens` was reached), ...",
    ),
    (
        "LlmReply",
        "input_tokens",
        "() -> Int",
        "The tokens of the conversation, as the server counted them (0 when it does not say).",
    ),
    (
        "LlmReply",
        "output_tokens",
        "() -> Int",
        "The tokens of the answer, as the server counted them (0 when it does not say).",
    ),
    (
        "LlmError",
        "kind",
        "() -> Text",
        "`invalid_key`, `model_not_found`, `rate_limit`, `refused`, `timeout`, `server`, `network` or `bad_request`.",
    ),
    (
        "LlmError",
        "message",
        "() -> Text",
        "What happened, for a log or a person.",
    ),
    (
        "LlmError",
        "retry_after",
        "() -> Float?",
        "For `rate_limit`, the seconds the server asked to wait, when it said.",
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
        "HttpResponse",
        "status",
        "() -> Int",
        "The status code: 200, 404, ...",
    ),
    (
        "HttpResponse",
        "ok",
        "() -> Bool",
        "Whether the status is from 200 to 299.",
    ),
    (
        "HttpResponse",
        "header",
        "(name: Text) -> Text?",
        "The value of a header, whatever the case of `name`, or `None`. A header that came more than once is joined with a comma.",
    ),
    (
        "HttpResponse",
        "headers",
        "() -> Map[Text, Text]",
        "Every header, with the names in lower case; one that came more than once is joined with a comma.",
    ),
    (
        "HttpResponse",
        "text",
        "() -> Result[Text, Text]",
        "The body read as UTF-8 text, or the position of the first byte that is not valid (use `bytes()` then).",
    ),
    (
        "HttpResponse",
        "bytes",
        "() -> Bytes",
        "The body as it came.",
    ),
    ("Bytes", "len", "() -> Int", "How many bytes there are."),
    (
        "Bytes",
        "is_empty",
        "() -> Bool",
        "Whether there are no bytes.",
    ),
    (
        "Bytes",
        "get",
        "(index: Int) -> Int?",
        "The byte at the index, from 0 to 255, or `None` when the index is negative or past the end.",
    ),
    (
        "Bytes",
        "slice",
        "(from: Int, to: Int) -> Bytes",
        "The bytes from `from` up to, not including, `to`. A negative index counts from the end and one past either end stops at it, as for `xs[from:to]`.",
    ),
    (
        "Bytes",
        "concat",
        "(other: Bytes) -> Bytes",
        "The bytes followed by `other`, as new bytes.",
    ),
    (
        "Bytes",
        "to_text",
        "() -> Result[Text, Text]",
        "The bytes read as UTF-8 text, or the position of the first byte that is not valid.",
    ),
    (
        "Bytes",
        "to_list",
        "() -> List[Int]",
        "Each byte as a whole number from 0 to 255.",
    ),
    (
        "Scaler",
        "apply",
        "(values: List[Float]) -> List[Float]",
        "Each value minus the training mean, divided by the training deviation.",
    ),
    (
        "Scaler",
        "inverse",
        "(values: List[Float]) -> List[Float]",
        "Undoes `apply`: a scaled prediction back in the units of the data.",
    ),
    ("Scaler", "mean", "() -> Float", "The training mean."),
    (
        "Scaler",
        "std",
        "() -> Float",
        "The training standard deviation (1 when it was 0).",
    ),
    (
        "LabelEncoder",
        "encode",
        "(values: List[Text]) -> List[Int]",
        "Each value as the position of its class. A value that was not in training is an error that lists the classes.",
    ),
    (
        "LabelEncoder",
        "decode",
        "(ids: List[Int]) -> List[Text]",
        "Each position back as its class: the predictions of a model as labels.",
    ),
    (
        "LabelEncoder",
        "classes",
        "() -> List[Text]",
        "The classes, in order.",
    ),
    (
        "Features",
        "transform",
        "(t: Table[Row]) -> Tensor[f32, N, D]",
        "The rows of `t` as a matrix with one row per row of `t` and `width()` columns, using what was learned in training. The table needs the feature columns; the target is not read.",
    ),
    (
        "Features",
        "names",
        "() -> List[Text]",
        "The name of each column of `transform`: the numeric features, then `column=class` for each class of each categorical one.",
    ),
    (
        "Features",
        "width",
        "() -> Int",
        "How many columns `transform` gives.",
    ),
    (
        "Features",
        "target",
        "() -> Text",
        "The column the model predicts.",
    ),
    ("Sparse", "rows", "() -> Int", "The number of rows."),
    ("Sparse", "cols", "() -> Int", "The number of columns."),
    (
        "Sparse",
        "nnz",
        "() -> Int",
        "How many values are stored: the ones that are not zero.",
    ),
    (
        "Sparse",
        "get",
        "(row: Int, col: Int) -> Float",
        "The value at a place, 0 when nothing is stored there. A place outside the matrix is an error.",
    ),
    (
        "Sparse",
        "row",
        "(i: Int) -> List[(Int, Float)]",
        "The `(column, value)` pairs of row `i`, in order of column.",
    ),
    (
        "Sparse",
        "select_rows",
        "(rows: List[Int]) -> Sparse",
        "The rows in the order given (a train/test split); a row given twice appears twice.",
    ),
    (
        "Sparse",
        "select_cols",
        "(cols: List[Int]) -> Sparse",
        "The columns in the order given (the kept features): column `cols[j]` becomes column `j`.",
    ),
    (
        "Sparse",
        "row_sums",
        "() -> List[Float]",
        "The sum of each row.",
    ),
    (
        "Sparse",
        "col_sums",
        "() -> List[Float]",
        "The sum of each column.",
    ),
    (
        "Sparse",
        "row_counts",
        "() -> List[Int]",
        "How many values each row stores.",
    ),
    (
        "Sparse",
        "col_counts",
        "() -> List[Int]",
        "How many rows have a value in each column: the document frequency of a word.",
    ),
    (
        "Sparse",
        "normalize_rows",
        "() -> Sparse",
        "Each row divided by its length (L2 norm), so it has length 1; an empty row stays empty.",
    ),
    (
        "Sparse",
        "transpose",
        "() -> Sparse",
        "Rows become columns.",
    ),
    (
        "Sparse",
        "scale",
        "(k: Float) -> Sparse",
        "Every value times `k`.",
    ),
    (
        "Sparse",
        "scale_rows",
        "(weights: List[Float]) -> Sparse",
        "Row `i` times `weights[i]`; the list has one weight per row.",
    ),
    (
        "Sparse",
        "scale_cols",
        "(weights: List[Float]) -> Sparse",
        "Column `j` times `weights[j]` (the idf weights of TF-IDF); the list has one weight per column.",
    ),
    (
        "Sparse",
        "matvec",
        "(x: Tensor[f32, C]) -> Tensor[f32, R]",
        "The matrix times a vector with one value per column. Another length is an error.",
    ),
    (
        "Sparse",
        "matmul",
        "(b: Tensor[f32, C, K]) -> Tensor[f32, R, K]",
        "The matrix times a dense matrix with one row per column of this one. Other sizes are an error.",
    ),
    (
        "Sparse",
        "to_dense",
        "() -> Tensor[f32, R, C]",
        "The whole matrix as a dense tensor, zeros included: rows times columns values, so only for a small one.",
    ),
    (
        "Generator",
        "int",
        "(low: Int, high: Int) -> Int",
        "The next whole number from `low` to `high`, both included, all equally likely. `low` above `high` is an error.",
    ),
    (
        "Generator",
        "float",
        "() -> Float",
        "The next number from 0 up to, not including, 1, with 53 random bits.",
    ),
    (
        "Generator",
        "choice",
        "(items: List[T]) -> T",
        "One item of the list, chosen at random. An empty list is an error.",
    ),
    (
        "Generator",
        "shuffle",
        "(items: List[T]) -> List[T]",
        "A new list with the same items in a random order. The list that was given is not changed.",
    ),
    (
        "Generator",
        "sample",
        "(items: List[T], n: Int) -> List[T]",
        "`n` different items of the list, in random order (a list of `n` places, not a draw with repeats). `n` is from 0 to the length of the list. The list that was given is not changed.",
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
        "Request",
        "body",
        "() -> Bytes",
        "The body as it came, whatever its content type (empty for `GET` and `DELETE`, and in a `@middleware`, which runs before the body is read).",
    ),
    (
        "Request",
        "text",
        "() -> Result[Text, Text]",
        "The body read as UTF-8 text, or the position of the first byte that is not valid.",
    ),
    (
        "Request",
        "form",
        "() -> Map[Text, Text]",
        "The text fields of a form body, `application/x-www-form-urlencoded` (an HTML form) or `multipart/form-data`; empty for any other body. A field sent twice keeps the last value. The same fields also fill the payload parameter of the endpoint, as a query string does.",
    ),
    (
        "Request",
        "uploads",
        "() -> List[Upload]",
        "The files of a `multipart/form-data` body (an `<input type=\"file\">`), in the order they came. The body of a request is limited to 2 MB.",
    ),
    (
        "Upload",
        "name",
        "() -> Text",
        "The name of the form field.",
    ),
    (
        "Upload",
        "filename",
        "() -> Text?",
        "The name of the file as the client sent it. It comes from the client: do not use it as a path without checking it.",
    ),
    (
        "Upload",
        "content_type",
        "() -> Text?",
        "The content type the client gave for the file, if any.",
    ),
    (
        "Upload",
        "bytes",
        "() -> Bytes",
        "The content of the file as it came.",
    ),
    (
        "Upload",
        "text",
        "() -> Result[Text, Text]",
        "The content of the file read as UTF-8 text, or the position of the first byte that is not valid.",
    ),
    (
        "Upload",
        "len",
        "() -> Int",
        "The size of the file in bytes.",
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
        "The characters from `start` up to `end`; see `std.text.slice`.",
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
    ("Table", "to_rows", "See `std.table.to_rows`."),
    ("Table", "sql", "See `std.table.sql`."),
    ("Table", "filter", "See `std.table.filter`."),
    ("Table", "head", "See `std.table.head`."),
    ("Table", "tail", "See `std.table.tail`."),
    ("Table", "sort_by", "See `std.table.sort_by`."),
    ("Table", "sort_by_desc", "See `std.table.sort_by_desc`."),
    ("Table", "concat", "See `std.table.concat`."),
    ("Table", "split", "See `std.table.split`."),
    ("Table", "select", "See `std.table.select`."),
    ("Table", "dropna", "See `std.table.dropna`."),
    ("Table", "sample", "See `std.table.sample`."),
    ("Table", "with_column", "See `std.table.with_column`."),
    ("Table", "group_by", "See `std.table.group_by`."),
    ("Tensor", "add", "See `std.tensor.add`."),
    ("Tensor", "sub", "See `std.tensor.sub`."),
    ("Tensor", "mul", "See `std.tensor.mul`."),
    ("Tensor", "div", "See `std.tensor.div`."),
    ("Tensor", "scale", "See `std.tensor.scale`."),
    ("Tensor", "relu", "See `std.tensor.relu`."),
    ("Tensor", "sigmoid", "See `std.tensor.sigmoid`."),
    ("Tensor", "ln", "See `std.tensor.ln`."),
    ("Tensor", "exp", "See `std.tensor.exp`."),
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

/// A piece of the reference page: Markdown that is the same in every language (the names, the signatures, the
/// layout), or a text that a translation can replace, with the key the translation is filed under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Segment {
    Fixed(String),
    Text { key: String, en: String },
}

/// FNV-1a 64 of a text, in hex: a translation records the hash of the English text it translates, so a changed
/// English text shows which translations are out of date.
pub fn text_hash(text: &str) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in text.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

struct Page(Vec<Segment>);

impl Page {
    fn fixed(&mut self, s: impl Into<String>) {
        let s = s.into();
        if let Some(Segment::Fixed(last)) = self.0.last_mut() {
            last.push_str(&s);
        } else {
            self.0.push(Segment::Fixed(s));
        }
    }

    fn text(&mut self, key: impl Into<String>, en: &str) {
        self.0.push(Segment::Text {
            key: key.into(),
            en: en.to_string(),
        });
    }

    fn function(&mut self, d: &FunctionDoc) {
        self.fixed(format!(
            "#### `{}`\n\n```text\n{}\n```\n\n",
            dotted(d.path),
            d.signature
        ));
        self.text(format!("fn.{}", d.path), d.summary);
        self.fixed("\n\n");
    }

    fn table_head(&mut self) {
        self.fixed("| ");
        self.text("label.method", "Method");
        self.fixed(" | ");
        self.text("label.what", "What it does");
        self.fixed(" |\n|---|---|\n");
    }
}

/// The reference as segments, in order. Joined, they are [`reference_markdown`]; the site joins them with the texts of
/// a translation (`docs/reference/std.json` in the repository of the language) where it has them.
pub fn reference_segments() -> Vec<Segment> {
    let mut p = Page(Vec::new());
    p.fixed("# ");
    p.text("page.title", "Standard library");
    p.fixed("\n\n");
    p.text(
        "page.intro",
        "Every function and method that the language provides, with its type and what it does. \
Functions are called by their path (`std.text.trim(s)`), or after an `import`, by the name. Many also work \
as methods (`s.trim()`) or in a pipeline (`s |> std.text.trim()`). \
A parameter written `Table[Row]`, `Tensor` or `T` stands for any type of that kind.",
    );
    p.fixed("\n\n");
    p.text(
        "page.generated",
        "This page is written by `reference_markdown` in `compiler/semantic/src/stdlib_docs.rs` and checked by a \
test: a function without an entry there, or a page that is out of date, fails the build.",
    );
    p.fixed("\n\n## ");
    p.text("heading.functions", "Functions");
    p.fixed("\n\n");
    for (module, intro) in MODULES {
        let docs: Vec<&FunctionDoc> = FUNCTIONS
            .iter()
            .filter(|d| module_of(d.path) == *module)
            .collect();
        if docs.is_empty() {
            continue;
        }
        p.fixed(format!("### `{module}`\n\n"));
        p.text(format!("module.{module}"), intro);
        p.fixed("\n\n");
        for d in docs {
            p.function(d);
        }
    }
    let rest: Vec<&FunctionDoc> = FUNCTIONS
        .iter()
        .filter(|d| !MODULES.iter().any(|(m, _)| module_of(d.path) == *m))
        .collect();
    if !rest.is_empty() {
        p.fixed("### ");
        p.text("heading.other", "Other");
        p.fixed("\n\n");
        for d in rest {
            p.function(d);
        }
    }

    p.fixed("## ");
    p.text("heading.types", "Types");
    p.fixed("\n\n");
    for (path, summary) in TYPES {
        p.fixed(format!("### `{}`\n\n", dotted(path)));
        p.text(format!("type.{path}"), summary);
        p.fixed("\n\n");
        let mut any = false;
        for (ty, name, sig, text) in STD_TYPE_METHODS {
            if path.rsplit("::").next() == Some(*ty) {
                if !any {
                    p.table_head();
                    any = true;
                }
                p.fixed(format!("| `{name}{sig}` | "));
                p.text(format!("method.{ty}.{name}"), text);
                p.fixed(" |\n");
            }
        }
        if any {
            p.fixed("\n");
        }
    }

    p.fixed("## ");
    p.text("heading.methods", "Methods");
    p.fixed("\n\n");
    p.text(
        "methods.intro",
        "The methods the language gives to its own types. `T`, `U`, `E` stand for any type.",
    );
    p.fixed("\n\n");
    for (ty, table) in method_tables() {
        p.fixed(format!("### `{ty}`\n\n"));
        p.table_head();
        for (name, sig) in table {
            let text = METHODS
                .iter()
                .find(|(t, n, _)| t == &ty && n == name)
                .map_or("", |(_, _, s)| *s);
            p.fixed(format!("| `{name}{sig}` | "));
            p.text(format!("builtin.{ty}.{name}"), text);
            p.fixed(" |\n");
        }
        p.fixed("\n");
    }
    p.0
}

/// The whole reference, as Markdown.
pub fn reference_markdown() -> String {
    reference_segments()
        .into_iter()
        .map(|s| match s {
            Segment::Fixed(t) => t,
            Segment::Text { en, .. } => en,
        })
        .collect()
}

fn json_string(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The segments as JSON, one per line: `{"md": ...}` for a fixed piece, `{"key", "en", "hash"}` for a text. The site
/// reads it (`docs/reference/std.segments.json`) to write the page of a language.
pub fn reference_segments_json() -> String {
    let lines: Vec<String> = reference_segments()
        .iter()
        .map(|s| match s {
            Segment::Fixed(t) => format!("{{\"md\": {}}}", json_string(t)),
            Segment::Text { key, en } => format!(
                "{{\"key\": {}, \"en\": {}, \"hash\": \"{}\"}}",
                json_string(key),
                json_string(en),
                text_hash(en)
            ),
        })
        .collect();
    format!("[\n{}\n]\n", lines.join(",\n"))
}
