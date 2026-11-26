# Standard library

Every function and method that the language provides, with its type and what it does. Functions are called by their path (`std.text.trim(s)`), or after an `import`, by the name. Many also work as methods (`s.trim()`) or in a pipeline (`s |> std.text.trim()`). A parameter written `Table[Row]`, `Tensor` or `T` stands for any type of that kind.

This page is written by `reference_markdown` in `compiler/semantic/src/stdlib_docs.rs` and checked by a test: a function without an entry there, or a page that is out of date, fails the build.

## Functions

### `text`

Functions on `Text`. The same operations exist as methods (`s.trim()`).

#### `std.text.clean_whitespace`

```text
std.text.clean_whitespace(text: Text) -> Text
```

Replaces every run of whitespace (spaces, tabs, new lines) with one space and removes it at both ends.

#### `std.text.trim`

```text
std.text.trim(text: Text) -> Text
```

Removes whitespace at both ends.

#### `std.text.lowercase`

```text
std.text.lowercase(text: Text) -> Text
```

The text in lower case.

#### `std.text.uppercase`

```text
std.text.uppercase(text: Text) -> Text
```

The text in upper case.

#### `std.text.truncate`

```text
std.text.truncate(text: Text, n: Int) -> Text
```

The first `n` characters. A shorter text is returned whole.

#### `std.text.normalize`

```text
std.text.normalize(text: Text, form: Text) -> Text
```

Writes the text in a Unicode normalization form: `"NFC"`, `"NFD"`, `"NFKC"` or `"NFKD"`. Any other `form` is an error. `"NFKD"` splits an accented letter into the letter and a combining mark.

#### `std.text.strip_accents`

```text
std.text.strip_accents(text: Text) -> Text
```

The text without accents and other combining marks: `"ação"` becomes `"acao"`.

#### `std.text.remove_chars`

```text
std.text.remove_chars(text: Text, chars: Text) -> Text
```

The text without every character that appears in `chars`.

#### `std.text.matches`

```text
std.text.matches(text: Text, pattern: Text) -> Bool
```

Whether the pattern (a regular expression) matches anywhere in the text. An invalid pattern is an error.

#### `std.text.find`

```text
std.text.find(text: Text, pattern: Text) -> Text?
```

The first part of the text that the pattern matches, or `None`.

#### `std.text.find_all`

```text
std.text.find_all(text: Text, pattern: Text) -> List[Text]
```

Every part of the text that the pattern matches, left to right, without overlaps.

#### `std.text.replace_all`

```text
std.text.replace_all(text: Text, pattern: Text, with: Text) -> Text
```

The text with every match replaced by `with`, where `$1` is the first group and `${name}` a named group.

#### `std.text.split`

```text
std.text.split(text: Text, sep: Text) -> List[Text]
```

Cuts the text at every `sep`. An empty `sep` is an error.

#### `std.text.to_text`

```text
std.text.to_text(value) -> Text
```

Any value as text, the way `print` shows it.

#### `std.text.format`

```text
std.text.format(value, spec: Text) -> Text
```

Formats a value with a format spec, the part after `:` in an f-string: `.2f`, `>8`, `05d`, `+.1e`, `,`.

#### `std.text.join`

```text
std.text.join(items: List[T], sep: Text) -> Text
```

Shows every item as text and joins them with `sep`.

#### `std.text.slice`

```text
std.text.slice(text: Text, start: Int, end: Int) -> Text
```

The characters from `start` up to, not including, `end`. A negative index counts from the end; an index past the end stops at the end; `start` after `end` gives an empty text.

#### `std.text.repeat`

```text
std.text.repeat(text: Text, n: Int) -> Text
```

The text `n` times. A negative `n` is an error.

### `string`

Deprecated: its functions are in `std.text` (`std.text.join`, `slice`, `repeat`). Edition 2027-preview removes it, and `telvax migrate` rewrites a program.

#### `std.string.join`

```text
std.string.join(items: List[T], sep: Text) -> Text
```

Deprecated: write `std.text.join`, the same function. Edition 2027-preview removes `std.string`, and `telvax migrate` rewrites it.

#### `std.string.slice`

```text
std.string.slice(text: Text, start: Int, end: Int) -> Text
```

Deprecated: write `std.text.slice`, the same function. Edition 2027-preview removes `std.string`.

#### `std.string.repeat`

```text
std.string.repeat(text: Text, n: Int) -> Text
```

Deprecated: write `std.text.repeat`, the same function. Edition 2027-preview removes `std.string`.

### `math`

Numbers. An `Int` argument gives an `Int` where that makes sense; a `Float` anywhere gives a `Float`.

#### `std.math.pow`

```text
std.math.pow(base, exp)
```

`base` to the power `exp`. Two integers give an `Int` (an overflow or a negative exponent is an error: write `2.0 ** -1`); otherwise a `Float`.

#### `std.math.floor_div`

```text
std.math.floor_div(a, b)
```

Division rounded down, so `floor_div(-7, 2)` is `-4`. Division by zero is an error.

#### `std.math.min`

```text
std.math.min(a, b)
```

The smaller of two numbers, or of two `Text` values (compared character by character).

#### `std.math.max`

```text
std.math.max(a, b)
```

The larger of two numbers, or of two `Text` values (compared character by character).

#### `std.math.abs`

```text
std.math.abs(x)
```

The absolute value. The smallest `Int` has none, which is an error.

#### `std.math.sqrt`

```text
std.math.sqrt(x) -> Float
```

The square root.

#### `std.math.ln`

```text
std.math.ln(x) -> Float
```

The natural logarithm. It follows IEEE 754: `ln(0)` is `-inf` and the logarithm of a negative number is `NaN`; neither is an error.

#### `std.math.log2`

```text
std.math.log2(x) -> Float
```

The base 2 logarithm. `log2(0)` is `-inf` and a negative number gives `NaN`, as for `ln`.

#### `std.math.log10`

```text
std.math.log10(x) -> Float
```

The base 10 logarithm. `log10(0)` is `-inf` and a negative number gives `NaN`, as for `ln`.

#### `std.math.exp`

```text
std.math.exp(x) -> Float
```

`e` raised to `x`. It is `inf` when the result does not fit in a `Float`, not an error.

#### `std.math.sin`

```text
std.math.sin(x) -> Float
```

The sine of an angle in radians.

#### `std.math.cos`

```text
std.math.cos(x) -> Float
```

The cosine of an angle in radians.

#### `std.math.tan`

```text
std.math.tan(x) -> Float
```

The tangent of an angle in radians.

#### `std.math.atan2`

```text
std.math.atan2(y, x) -> Float
```

The angle in radians, from `-pi` to `pi`, of the point `(x, y)`: the arc tangent of `y / x` in the right quadrant. `atan2(0.0, 0.0)` is `0.0`.

#### `std.math.pi`

```text
std.math.pi() -> Float
```

The number pi, the ratio of a circle to its diameter (`3.141592653589793`).

#### `std.math.ln_gamma`

```text
std.math.ln_gamma(x) -> Float
```

The natural logarithm of the absolute value of the gamma function, so `ln_gamma(5)` is `ln(24)`. It does not overflow for large `x`, as `ln(gamma(x))` would. At `0` and the negative whole numbers, where the gamma function has a pole, it is `inf`. Accurate to about 1e-13.

#### `std.math.digamma`

```text
std.math.digamma(x) -> Float
```

The digamma function, the derivative of `ln_gamma`: `digamma(1)` is minus the Euler-Mascheroni constant. At `0` and the negative whole numbers it is `NaN`. Accurate to about 1e-13.

#### `std.math.floor`

```text
std.math.floor(x) -> Float
```

The largest whole number not above `x`, as a `Float`.

#### `std.math.ceil`

```text
std.math.ceil(x) -> Float
```

The smallest whole number not below `x`, as a `Float`.

#### `std.math.round`

```text
std.math.round(x) -> Float
```

The nearest whole number, as a `Float`. Halves round away from zero.

#### `std.math.to_float`

```text
std.math.to_float(x) -> Float
```

A number as a `Float`.

#### `std.math.to_int`

```text
std.math.to_int(x) -> Int
```

A number as an `Int`, dropping the fraction (toward zero). `NaN` and infinity are errors.

### `iter`

Lists: `map`, `filter`, `fold` and the rest. Every function here is also a method of `List` (`xs.map(f)`).

#### `std.iter.map`

```text
std.iter.map(items: List[T], f: (T) -> U) -> List[U]
```

A list of `f` applied to every item.

#### `std.iter.filter`

```text
std.iter.filter(items: List[T], keep: (T) -> Bool) -> List[T]
```

The items for which `keep` is true, in order.

#### `std.iter.any`

```text
std.iter.any(items: List[T], f: (T) -> Bool) -> Bool
```

Whether `f` is true for at least one item.

#### `std.iter.all`

```text
std.iter.all(items: List[T], f: (T) -> Bool) -> Bool
```

Whether `f` is true for every item.

#### `std.iter.find`

```text
std.iter.find(items: List[T], f: (T) -> Bool) -> T?
```

The first item for which `f` is true, or `None`.

#### `std.iter.for_each`

```text
std.iter.for_each(items: List[T], f: (T) -> ())
```

Calls `f` on every item, in order.

#### `std.iter.fold`

```text
std.iter.fold(items: List[T], init: A, f: (A, T) -> A) -> A
```

Starts with `init` and replaces it with `f(acc, item)` for every item.

#### `std.iter.enumerate`

```text
std.iter.enumerate(items: List[T]) -> List[(Int, T)]
```

Pairs every item with its position, counting from 0.

#### `std.iter.zip`

```text
std.iter.zip(a: List[T], b: List[U]) -> List[(T, U)]
```

Pairs the items of two lists. It stops at the end of the shorter one.

#### `std.iter.sum`

```text
std.iter.sum(items: List[N]) -> N
```

The sum of a list of numbers: 0 for an empty list, a `Float` if any item is one. An `Int` overflow is an error.

#### `std.iter.min`

```text
std.iter.min(items: List[T]) -> T?
```

The smallest number or text, or `None` for an empty list.

#### `std.iter.max`

```text
std.iter.max(items: List[T]) -> T?
```

The largest number or text, or `None` for an empty list.

#### `std.iter.skip`

```text
std.iter.skip(items: List[T], n: Int) -> List[T]
```

The list without its first `n` items.

#### `std.iter.take`

```text
std.iter.take(items: List[T], n: Int) -> List[T]
```

At most the first `n` items.

#### `std.iter.count`

```text
std.iter.count(items: List[T]) -> u64
std.iter.count(items: List[T], f: (T) -> Bool) -> u64
```

The number of items, or with `f` how many of them make `f` true (the method `xs.count(f)`).

#### `std.iter.slice`

```text
std.iter.slice(items: List[T], start: Int, end: Int) -> List[T]
```

The items from `start` up to, not including, `end`, with the index rules of `std.text.slice`.

#### `std.iter.concat`

```text
std.iter.concat(a: List[T], b: List[T]) -> List[T]
```

A new list with the items of `a` and then those of `b`.

#### `std.iter.range`

```text
std.iter.range(end: Int) -> List[Int]
std.iter.range(start: Int, end: Int) -> List[Int]
```

The integers from `start` (0 if omitted) up to, not including, `end`.

#### `std.collect`

```text
std.collect(items: List[T]) -> List[T]
```

Returns the list it is given.

### `map`

Building a `Map[K, V]`. The methods of a map (`insert`, `get`, ...) are listed under Methods.

#### `std.map.new`

```text
std.map.new() -> Map[K, V]
```

An empty map. The key and value types come from the annotation: `let m: Map[Text, Int] = {}`.

#### `std.map.from_entries`

```text
std.map.from_entries(entries: List[(K, V)]) -> Map[K, V]
```

A map from `(key, value)` pairs. A key is a `Text`, an integer, a `Bool` or an enum without data; a later pair replaces an earlier one with the same key.

### `option`

Function forms of the methods of `T?`. `o.map(f)` and `std.option.map(o, f)` are the same call.

#### `std.option.map`

```text
std.option.map(o: T?, f: (T) -> U) -> U?
```

Function form of the `Option` method `map`.

#### `std.option.and_then`

```text
std.option.and_then(o: T?, f: (T) -> U?) -> U?
```

Function form of the `Option` method `and_then`.

#### `std.option.filter`

```text
std.option.filter(o: T?, keep: (T) -> Bool) -> T?
```

Function form of the `Option` method `filter`.

#### `std.option.ok_or`

```text
std.option.ok_or(o: T?, error: E) -> Result[T, E]
```

Function form of the `Option` method `ok_or`.

#### `std.option.unwrap_or_else`

```text
std.option.unwrap_or_else(o: T?, f: () -> T) -> T
```

Function form of the `Option` method `unwrap_or_else`.

#### `std.option.expect`

```text
std.option.expect(o: T?, message: Text) -> T
```

Function form of the `Option` method `expect`.

### `result`

Function forms of the methods of `Result[T, E]`. `r.map(f)` and `std.result.map(r, f)` are the same call.

#### `std.result.is_ok`

```text
std.result.is_ok(r: Result[T, E]) -> Bool
```

Function form of the `Result` method `is_ok`.

#### `std.result.is_err`

```text
std.result.is_err(r: Result[T, E]) -> Bool
```

Function form of the `Result` method `is_err`.

#### `std.result.unwrap`

```text
std.result.unwrap(r: Result[T, E]) -> T
```

Function form of the `Result` method `unwrap`.

#### `std.result.expect`

```text
std.result.expect(r: Result[T, E], message: Text) -> T
```

Function form of the `Result` method `expect`.

#### `std.result.unwrap_or`

```text
std.result.unwrap_or(r: Result[T, E], default: T) -> T
```

Function form of the `Result` method `unwrap_or`.

#### `std.result.unwrap_or_else`

```text
std.result.unwrap_or_else(r: Result[T, E], f: (E) -> T) -> T
```

Function form of the `Result` method `unwrap_or_else`.

#### `std.result.map`

```text
std.result.map(r: Result[T, E], f: (T) -> U) -> Result[U, E]
```

Function form of the `Result` method `map`.

#### `std.result.map_err`

```text
std.result.map_err(r: Result[T, E], f: (E) -> F) -> Result[T, F]
```

Function form of the `Result` method `map_err`.

#### `std.result.and_then`

```text
std.result.and_then(r: Result[T, E], f: (T) -> Result[U, E]) -> Result[U, E]
```

Function form of the `Result` method `and_then`.

#### `std.result.ok`

```text
std.result.ok(r: Result[T, E]) -> T?
```

Function form of the `Result` method `ok`.

#### `std.result.err`

```text
std.result.err(r: Result[T, E]) -> E?
```

Function form of the `Result` method `err`.

### `tensor`

Tensors. Shapes are checked before the program runs. Every function here is also a method (`a.matmul(b)`).

#### `std.tensor.zeros`

```text
std.tensor.zeros() -> Tensor[T, dims]
```

A tensor of zeros. The element type and shape come from the annotation: `let t: Tensor[f32, 2, 3] = std.tensor.zeros()`.

#### `std.tensor.from_vec`

```text
std.tensor.from_vec(values: List[T]) -> Tensor[T, dims]
```

A tensor with the values of a list. The shape comes from the annotation, and the number of values must fit it.

#### `std.tensor.from_rows`

```text
std.tensor.from_rows(rows: List[List[T]]) -> Tensor[T, R, C]
```

A matrix with one row per list, when its size is only known as the program runs (`from_vec` takes the size from the annotation). Every row must have the same length. The elements are numbers: `Float` rows give `Tensor[Float, R, C]`, and an annotation `Tensor[f32, R, C]` makes them `f32`.

#### `std.tensor.reshape`

```text
std.tensor.reshape(t: Tensor) -> Tensor[T, dims]
```

The same elements in another shape, written in the annotation. The number of elements must not change; a mismatch that can be seen before running is a compile error.

#### `std.tensor.to_vec`

```text
std.tensor.to_vec(t: Tensor[T, dims]) -> List[T]
```

The elements, row by row, as a list.

#### `std.tensor.add`

```text
std.tensor.add(a: Tensor, b: Tensor) -> Tensor
```

Element-wise sum. The two shapes and element types must agree.

#### `std.tensor.sub`

```text
std.tensor.sub(a: Tensor, b: Tensor) -> Tensor
```

Element-wise difference. The two shapes and element types must agree.

#### `std.tensor.mul`

```text
std.tensor.mul(a: Tensor, b: Tensor) -> Tensor
```

Element-wise product (not a matrix product: see `matmul`).

#### `std.tensor.div`

```text
std.tensor.div(a: Tensor, b: Tensor) -> Tensor
```

Element-wise quotient.

#### `std.tensor.scale`

```text
std.tensor.scale(t: Tensor, k: Float) -> Tensor
```

Every element times `k`.

#### `std.tensor.relu`

```text
std.tensor.relu(t: Tensor) -> Tensor
```

Replaces negative elements with 0.

#### `std.tensor.ln`

```text
std.tensor.ln(t: Tensor) -> Tensor
```

The natural logarithm of every element, in the same shape. An element of 0 gives `-inf` and a negative one gives `NaN`, as `std.math.ln` does.

#### `std.tensor.exp`

```text
std.tensor.exp(t: Tensor) -> Tensor
```

`e` raised to every element, in the same shape.

#### `std.tensor.sigmoid`

```text
std.tensor.sigmoid(t: Tensor) -> Tensor
```

`1 / (1 + e^-x)` for every element.

#### `std.tensor.softmax`

```text
std.tensor.softmax(t: Tensor) -> Tensor
```

Turns every row (the last axis) into probabilities that sum to 1.

#### `std.tensor.sum`

```text
std.tensor.sum(t: Tensor[T, dims]) -> T
```

The sum of all elements.

#### `std.tensor.mean`

```text
std.tensor.mean(t: Tensor[T, dims]) -> T
```

The mean of all elements.

#### `std.tensor.argmax`

```text
std.tensor.argmax(t: Tensor) -> u64
```

The position of the largest element when the tensor is read row by row. The first one wins a tie.

#### `std.tensor.dot`

```text
std.tensor.dot(a: Tensor[T, N], b: Tensor[T, N]) -> T
```

The dot product of two vectors of the same length.

#### `std.tensor.matmul`

```text
std.tensor.matmul(a: Tensor[T, M, K], b: Tensor[T, K, N]) -> Tensor[T, M, N]
std.tensor.matmul(a: Tensor[T, M, K], v: Tensor[T, K]) -> Tensor[T, M]
```

The matrix product. The inner sizes `K` must agree; a mismatch is a compile error.

#### `std.tensor.transpose`

```text
std.tensor.transpose(t: Tensor[T, M, N]) -> Tensor[T, N, M]
```

Swaps the two axes of a matrix.

### `table`

Tables on Arrow. A function that reads data needs the row type written down: `let t: Table[Row] = ...`. Most are also methods of `Table`.

#### `std.table.read_csv`

```text
std.table.read_csv(path: Text) -> Table[Row]
```

Reads a CSV file with a header line. The columns are read in the order of the fields of `Row`. An empty cell in a column that is not `T?` is an error. Needs `fs.read`.

#### `std.table.read_parquet`

```text
std.table.read_parquet(path: Text) -> Table[Row]
```

Reads a Parquet file. The columns are matched to the fields of `Row` by name and converted to their types. Not in `telvax-lite`. Needs `fs.read`.

#### `std.table.read_ipc`

```text
std.table.read_ipc(path: Text) -> Table[Row]
```

Reads an Arrow IPC file (Feather v2). The file is memory-mapped, so a file with one batch and no compression is not copied. Do not rewrite the file while the table is in use. Needs `fs.read`.

#### `std.table.read_flight`

```text
std.table.read_flight(url: Text, ticket: Text) -> Table[Row]
```

Fetches a table from an Arrow Flight server (`DoGet`). `url` is `grpc://host:port` or `grpc+tls://host:port`. Only in builds with the `flight` feature. Needs `net`.

#### `std.table.read_csv_batches`

```text
std.table.read_csv_batches(path: Text, n: Int) -> List[Table[Row]]
```

Reads a CSV file into tables of at most `n` rows, without building one joined copy. All of them are in memory. `n` must be at least 1. Needs `fs.read`.

#### `std.table.read_parquet_batches`

```text
std.table.read_parquet_batches(path: Text, n: Int) -> List[Table[Row]]
```

Like `read_csv_batches`, for a Parquet file. Not in `telvax-lite`.

#### `std.table.read_ipc_batches`

```text
std.table.read_ipc_batches(path: Text, n: Int) -> List[Table[Row]]
```

Like `read_csv_batches`, for an Arrow IPC file. The chunks share the memory-mapped file.

#### `std.table.sql`

```text
std.table.sql(t: Table[Row], query: Text) -> Table[Out]
```

Runs a SQL query on the table, which is named `t` in the query. The result type is written in the annotation and its fields are matched to the columns by name. Not in `telvax-lite`.

#### `std.table.from_rows`

```text
std.table.from_rows(rows: List[Row]) -> Table[Row]
```

A table from a list of values of the row type.

#### `std.table.to_rows`

```text
std.table.to_rows(t: Table[Row]) -> List[Row]
```

The rows as a list of values of the row type.

#### `std.table.len`

```text
std.table.len(t: Table[Row]) -> u64
```

The number of rows.

#### `std.table.write_csv`

```text
std.table.write_csv(t: Table[Row], path: Text)
```

Writes a CSV file with a header line. Needs `fs.write`.

#### `std.table.write_parquet`

```text
std.table.write_parquet(t: Table[Row], path: Text)
```

Writes a Parquet file. Not in `telvax-lite`. Needs `fs.write`.

#### `std.table.write_ipc`

```text
std.table.write_ipc(t: Table[Row], path: Text)
```

Writes an Arrow IPC file (Feather v2), which PyArrow, Polars and pandas read. Needs `fs.write`.

#### `std.table.write_flight`

```text
std.table.write_flight(t: Table[Row], url: Text, path: Text)
```

Sends the table to an Arrow Flight server (`DoPut`) under `path`. Only in builds with the `flight` feature. Needs `net`.

#### `std.table.head`

```text
std.table.head(t: Table[Row], n: Int) -> Table[Row]
```

The first `n` rows (all of them if there are fewer). `n` must not be negative.

#### `std.table.sample`

```text
std.table.sample(t: Table[Row], n: Int, seed: Int) -> Table[Row]
```

`n` rows of the table, chosen at random, none twice, in random order. The same `seed` gives the same rows on every machine and in every release (it is drawn with `std.random.Generator`). `n` from 0 to the number of rows; more is an error.

#### `std.table.with_column`

```text
std.table.with_column(t: Table[Row], name: Text, f: (Row) -> V) -> Table
```

The table with one more column. `name` is written in place as a text, it is not a column of `t` yet, and `f` is called with each row and gives the value, a number, a `Bool` or a `Text` (or an `Option` of one). The type of the rows of the result is worked out from `t` and `f`, as for `select`, so use the result as it is (`let u = t.with_column("polarity", r => score(r.text))`) and read `u.polarity`.

#### `std.table.dropna`

```text
std.table.dropna(t: Table[Row], columns: List[Text]) -> Table[Row]
```

The rows that have a value in every one of the `columns`: a row is dropped when one of them is empty (a missing value) or, in a `Float` column, `NaN`. The row type does not change. A name that is not a column is an error that lists the columns. With an empty list nothing is dropped.

#### `std.table.tail`

```text
std.table.tail(t: Table[Row], n: Int) -> Table[Row]
```

The last `n` rows.

#### `std.table.sort_by`

```text
std.table.sort_by(t: Table[Row], column: Text) -> Table[Row]
```

The rows ordered by a column, smallest first. A name that is not a column is an error that lists the columns.

#### `std.table.sort_by_desc`

```text
std.table.sort_by_desc(t: Table[Row], column: Text) -> Table[Row]
```

The rows ordered by a column, largest first.

#### `std.table.concat`

```text
std.table.concat(a: Table[Row], b: Table[Row]) -> Table[Row]
```

The rows of `a` and then those of `b`. Both need the same row type.

#### `std.table.select`

```text
std.table.select(t: Table[Row], columns: List[Text]) -> Table[{...}]
```

Only the named columns, in the order given. The names are written as a list of text in place, and the result row is worked out from them, so no annotation is needed. A name that is not a column, or one named twice, is an error.

#### `std.table.group_by`

```text
std.table.group_by(t: Table[Row], keys: List[Text], aggregations: List[Text]) -> Table[{...}]
```

One row per distinct value of the key columns, in order of first appearance, then one column `<column>_<function>` for each aggregation written `column:function`: `sum` (`Int` for an integer column, `Float` for a decimal one), `mean` (`Float`), `min` and `max` (the column type) and `count` (`Int`, any column). Everything is written as text in lists in place, so the result row is worked out from them and needs no annotation. An unknown column or function, a column typed `T?` and a result column named twice are errors.

#### `std.table.split`

```text
std.table.split(t: Table[Row], n: Int) -> List[Table[Row]]
```

Cuts the table into tables of at most `n` rows that share its buffers, so nothing is copied. `n` must be at least 1.

#### `std.table.filter`

```text
std.table.filter(t: Table[Row], keep: (Row) -> Bool) -> Table[Row]
```

The rows for which `keep` is true.

#### `std.table.take`

```text
std.table.take(t: Table[Row], rows: List[Int]) -> Table[Row]
```

The rows at those positions, in that order (the folds of `std.ml.k_fold`); a position may repeat. A position outside the table is an error.

### `task`

`Task[T]`, what an `async fn` gives. `await` runs one; these run several.

#### `std.task.gather`

```text
std.task.gather(tasks: List[Task[T]]) -> Task[List[T]]
```

A task that runs all of them at the same time and gives their values in the same order.

#### `std.task.run`

```text
std.task.run(task: Task[T]) -> T
```

Runs a task and waits for its value, which is what `await` does.

#### `std.task.spawn`

```text
std.task.spawn(task: Task[T], on_error: (Text) -> () = ...)
```

Starts a task and returns at once. The task goes on after the function that started it has returned, so an `@endpoint` can answer `202` and finish later. Its value is dropped. If it fails, `on_error` is called with the message (a job can then be marked as failed); without `on_error` the message is written to the standard error. At most 256 run at once. In `telvax run` and `telvax test` it waits for the task, as `run` does.

#### `std.task.sleep`

```text
std.task.sleep(ms: Int) -> Task[()]
```

A task that finishes after `ms` milliseconds.

### `time`

Clock and waiting.

#### `std.time.sleep`

```text
std.time.sleep(ms: Int)
```

Waits `ms` milliseconds, blocking the thread.

#### `std.time.now`

```text
std.time.now() -> Float
```

The seconds since 1970-01-01 00:00 UTC, with a fraction.

#### `std.time.format`

```text
std.time.format(t: Float, pattern: Text) -> Text
```

The time `t` (seconds since 1970-01-01 00:00 UTC, as `now` gives) written in UTC with a `strftime` pattern: `format(0.0, "%Y%m%d-%H%M%S")` is `"19700101-000000"`. A pattern with an unknown `%` code is an error. There is no local time zone and no date arithmetic.

#### `std.time.parse`

```text
std.time.parse(text: Text, pattern: Text) -> Result[Float, Text]
```

Reads a time written with a `strftime` pattern and gives it as seconds since 1970-01-01 00:00 UTC, the inverse of `format`. A pattern with only a date (`"%Y-%m-%d"`) is midnight. A pattern with a UTC offset (`%z`) is converted to UTC; without one the text is taken as UTC. The error says what could not be read.

#### `std.time.weekday`

```text
std.time.weekday(t: Timestamp | Date | Float) -> Int
```

The day of the week in UTC, Monday 0 to Sunday 6 (as pandas' `dayofweek`). `t` is a `Timestamp`, a `Date`, or seconds since 1970.

#### `std.time.hour`

```text
std.time.hour(t: Timestamp | Date | Float) -> Int
```

The hour of the day in UTC, 0 to 23.

#### `std.time.epoch`

```text
std.time.epoch(t: Timestamp | Date | Float) -> Float
```

Seconds since 1970-01-01 UTC, the number a model reads for a time.

### `cache`

A map with a size limit and an expiry.

#### `std.cache.new`

```text
std.cache.new(max: Int, ttl_seconds: Float) -> Cache
```

A cache that holds at most `max` entries and forgets an entry `ttl_seconds` after it was set. When it is full, `set` drops the entry that was used least recently. It is shared by every request like a top-level `Map`: each call is atomic. `max` and `ttl_seconds` must be above 0. The values are not type checked yet: `get` gives an `Option` of whatever was stored.

### `random`

Random values from the operating system (`uuid`, `int`), and `Generator`, a generator with a seed that gives the same numbers every time.

#### `std.random.uuid`

```text
std.random.uuid() -> Text
```

A random version 4 UUID as 32 lower case hexadecimal digits, from the operating system's random source. Fit for ids that must not be guessed. Needs no capability.

#### `std.random.int`

```text
std.random.int(low: Int, high: Int) -> Int
```

A random whole number from `low` to `high`, both included, all equally likely. `low` above `high` is an error. Needs no capability.

#### `std.random.Generator.new`

```text
std.random.Generator.new(seed: Int) -> Generator
```

A random number generator that starts from `seed`: the same seed gives the same numbers, on every machine and in every release. See `Generator` for the algorithm. It is not made for secrets or ids that must not be guessed: use `uuid` for those. Needs no capability.

### `model`

Saving a trained model (its weights and its fitted preprocessing, any struct of them) to a safetensors file, and loading it back as the type the program names. Loading never runs anything from the file.

#### `std.model.save`

```text
std.model.save(value: T, path: Text) -> Result[(), Text]
```

Writes `value` to a file in the safetensors format: the tensors as raw little-endian bytes, named by where they are in the value (`weights`, `layers[0].bias`, `words.indptr`), and the shape of the value in the metadata. A value can be made of numbers, `Text`, `Bool`, lists, tuples, options, structs, tensors of `f32`, `Float` or `Int`, `Sparse`, and the fitted steps of `std.ml` (`Features`, `Scaler`, `LabelEncoder`), so a struct that holds the preprocessing and the weights is saved as one file. Needs `fs.write`.

#### `std.model.load`

```text
std.model.load(path: Text) -> Result[T, Text]
```

Reads a file written by `save` as the type `T` the program asks for (`let m: Result[Model, Text] = std.model.load(path)`). A file of another type (another struct, a field more or less, a tensor of another element type or size), of a newer format, or damaged is an `Err` that says which; the type in the file is in the message. Loading parses the header and copies bytes, and runs nothing from the file. Needs `fs.read`.

#### `std.model.to_bytes`

```text
std.model.to_bytes(value: T) -> Result[Bytes, Text]
```

The bytes `save` would write, to send or store elsewhere.

#### `std.model.from_bytes`

```text
std.model.from_bytes(b: Bytes) -> Result[T, Text]
```

`load` from bytes instead of a file.

#### `std.model.describe`

```text
std.model.describe(path: Text) -> Result[Text, Text]
```

The type of the value in a model file, as text (`Model { features: Features, weights: Tensor[f32, 7, 1], ... }`), to log when a service starts. Needs `fs.read`.

### `ml`

What goes around a model on a table: train/test splits, metrics, and preprocessing steps fitted on the training rows only (`Scaler`, `LabelEncoder`, `Features`). The numbers are scikit-learn's.

#### `std.ml.split`

```text
std.ml.split(t: Table[Row], test_size: Float, seed: Int) -> (Table[Row], Table[Row])
```

The rows shuffled with the seed (`std.random.Generator`) and cut in two: the training rows and the test rows. `test_size` is the share of the test, above 0 and below 1, rounded up as scikit-learn's `train_test_split`, so 40 rows and 0.25 give 30 and 10. The same seed gives the same split everywhere; the rows are not those of scikit-learn for the same number. Both parts need a row.

#### `std.ml.split_stratified`

```text
std.ml.split_stratified(t: Table[Row], column: Text, test_size: Float, seed: Int) -> (Table[Row], Table[Row])
```

Like `split`, with each class of `column` cut in the same share (rounded to the nearest row), so a rare class is in both parts.

#### `std.ml.split_by_time`

```text
std.ml.split_by_time(t: Table[Row], test_size: Float) -> (Table[Row], Table[Row])
```

The last rows as the test, in the order of the table: sort it by time first (`sort_by`). Nothing is shuffled, so no row of the future is used to train.

#### `std.ml.k_fold`

```text
std.ml.k_fold(n: Int, k: Int, seed: Int) -> List[(List[Int], List[Int])]
```

The rows `0` to `n - 1` shuffled and cut in `k` folds, as scikit-learn's `KFold(k, shuffle=True)`: for each fold, the training rows and the test rows, each in increasing order, to give to `std.table.take`. The first `n % k` folds have one row more. At least 2 folds and a row for each.

#### `std.ml.mae`

```text
std.ml.mae(y_true: List[Float] | Tensor[T, N], y_pred: List[Float] | Tensor[T, N]) -> Float
```

The mean absolute error. Each argument is a list of numbers or a tensor of one dimension (a numeric column, the output of a model); they are as many.

#### `std.ml.mse`

```text
std.ml.mse(y_true, y_pred) -> Float
```

The mean squared error.

#### `std.ml.rmse`

```text
std.ml.rmse(y_true, y_pred) -> Float
```

The square root of the mean squared error.

#### `std.ml.r2`

```text
std.ml.r2(y_true, y_pred) -> Float
```

The coefficient of determination: 1 for a perfect prediction, 0 for one as good as the mean, below 0 for a worse one. For a constant `y_true`, 1 when it is predicted exactly and 0 otherwise, as scikit-learn.

#### `std.ml.accuracy`

```text
std.ml.accuracy(y_true: List[T], y_pred: List[T]) -> Float
```

The share of rows predicted right. The labels are a list or a tensor of one dimension, of any type that prints (a `Text`, an `Int`, a `Bool`).

#### `std.ml.precision`

```text
std.ml.precision(y_true: List[T], y_pred: List[T], average: Text) -> Float
```

Of the rows predicted as a class, the share that are of it, averaged over the classes as `macro` (each class the same), `weighted` (by how many rows each class has) or `micro` (over all the rows). A class never predicted counts 0, as scikit-learn's `zero_division=0`.

#### `std.ml.recall`

```text
std.ml.recall(y_true: List[T], y_pred: List[T], average: Text) -> Float
```

Of the rows of a class, the share predicted as it, averaged as for `precision`.

#### `std.ml.f1`

```text
std.ml.f1(y_true: List[T], y_pred: List[T], average: Text) -> Float
```

The harmonic mean of precision and recall of each class, averaged as for `precision` (`macro` is the macro-F1).

#### `std.ml.confusion_matrix`

```text
std.ml.confusion_matrix(y_true: List[T], y_pred: List[T]) -> (List[Text], List[List[Int]])
```

The classes in order (as numbers when they all are, otherwise as text) and the matrix: row `i`, column `j` counts the rows of class `i` predicted as class `j`.

#### `std.ml.classification_report`

```text
std.ml.classification_report(y_true: List[T], y_pred: List[T]) -> Text
```

The precision, recall, F1 and number of rows of each class, the accuracy and the macro and weighted averages, in the text of scikit-learn's `classification_report`.

#### `std.ml.roc_auc`

```text
std.ml.roc_auc(y_true: List[Bool], scores: List[Float]) -> Float
```

The area under the ROC curve: the chance that a positive row scores above a negative one, ties counting half. It needs both positive and negative rows.

#### `std.ml.Scaler.fit`

```text
std.ml.Scaler.fit(values: List[Float]) -> Scaler
```

Learns the mean and the standard deviation of the training values.

#### `std.ml.LabelEncoder.fit`

```text
std.ml.LabelEncoder.fit(values: List[Text]) -> LabelEncoder
```

Learns the classes of the training values, in order; a class becomes its position.

#### `std.ml.Features.fit`

```text
std.ml.Features.fit(train: Table[Row], target: Text, numeric: List[Text], categorical: List[Text]) -> Features
```

Learns, from the training rows only, how a row becomes the input of a model: each `numeric` column standardized with its training mean and deviation (a `Date` or `Timestamp` as seconds since 1970, a `Bool` as 1 or 0), then each `categorical` column one-hot with its training classes. `target` names the column the model predicts; giving it as a feature is an error, since the model would read its answer. A column with missing values is an error.

### `sparse`

`Sparse`, a matrix of `f32` where only the values that are not zero are stored (compressed sparse rows), for text features and other wide data.

#### `std.sparse.from_rows`

```text
std.sparse.from_rows(rows: List[List[(Int, Float)]], cols: Int) -> Sparse
```

A matrix with one row per item of `rows`, each a list of `(column, value)` in any order, and `cols` columns. Values given twice for one place are added; zeros are not stored. A column from `cols` on is an error.

#### `std.sparse.from_triplets`

```text
std.sparse.from_triplets(rows: Int, cols: Int, triplets: List[(Int, Int, Float)]) -> Sparse
```

A `rows` by `cols` matrix from `(row, column, value)` in any order. Values given twice for one place are added; a place outside the matrix is an error.

#### `std.sparse.from_dense`

```text
std.sparse.from_dense(t: Tensor[T, R, C]) -> Sparse
```

The values of a tensor of 2 dimensions that are not zero, as `f32`.

### `io`

Writing to the standard output.

#### `std.io.print`

```text
std.io.print(text: Text)
```

Writes the text to the standard output, without a new line.

#### `std.io.println`

```text
std.io.println(text: Text)
```

Writes the text and a new line. This is the `print` that needs no `std.` prefix.

### `fs`

Files. Needs the `fs.read` / `fs.write` permission.

#### `std.fs.read`

```text
std.fs.read(path: Text) -> Result[Text, Text]
```

The content of a file as text, or the reason it could not be read. Needs `fs.read`.

#### `std.fs.embed`

```text
std.fs.embed(path: Text) -> Text
```

The content of a file, read when the program is built and kept inside it. The path is a text literal, relative to the directory of the main file. Needs no capability.

#### `std.fs.write`

```text
std.fs.write(path: Text, text: Text) -> Result[(), Text]
```

Writes a text to a file, replacing it. Needs `fs.write`.

#### `std.fs.exists`

```text
std.fs.exists(path: Text) -> Bool
```

Whether the path exists. Needs `fs.read`.

#### `std.fs.create_dir`

```text
std.fs.create_dir(path: Text) -> Result[(), Text]
```

Creates a directory and the ones above it that are missing. It is not an error when the directory already exists. Needs `fs.write`.

#### `std.fs.list_dir`

```text
std.fs.list_dir(path: Text) -> Result[List[Text], Text]
```

The names of what is in a directory (files and directories, not the path), in alphabetical order, or the reason it could not be read. Needs `fs.read`.

#### `std.fs.walk`

```text
std.fs.walk(path: Text) -> Result[List[Text], Text]
```

The files under a directory, at any depth, as paths that start with `path`, in alphabetical order. Directories are not listed, only the files in them. Needs `fs.read`.

#### `std.fs.remove`

```text
std.fs.remove(path: Text) -> Result[(), Text]
```

Removes a file or an empty directory. A directory with something in it is an error, so it never removes a tree. Needs `fs.write`.

#### `std.fs.copy`

```text
std.fs.copy(from: Text, to: Text) -> Result[(), Text]
```

Copies a file to `to`, replacing it. The directory of `to` must exist. Needs `fs.read` and `fs.write`.

#### `std.fs.read_bytes`

```text
std.fs.read_bytes(path: Text) -> Result[Bytes, Text]
```

The content of a file as bytes, whatever it holds, or the reason it could not be read. Needs `fs.read`.

#### `std.fs.write_bytes`

```text
std.fs.write_bytes(path: Text, data: Bytes) -> Result[(), Text]
```

Writes the bytes to a file, replacing it. Needs `fs.write`.

### `path`

Paths as text: join, split and compare them. They only read the text, never the disk, and need no permission. A path is read as the operating system does (`/` on Linux and macOS).

#### `std.path.join`

```text
std.path.join(path: Text, part: Text) -> Text
```

`part` added to `path` with one `/` between them. `std.path` writes `/` on every system, Windows included (which reads it), so a program gives the same paths everywhere. When `part` is an absolute path, it replaces `path`. Join more than two with `join(join(a, b), c)`.

#### `std.path.parent`

```text
std.path.parent(path: Text) -> Text?
```

The path without its last part: `parent("a/b.txt")` is `"a"` and `parent("b.txt")` is `""`. A root (`"/"`) or an empty path has none, so `None`.

#### `std.path.file_name`

```text
std.path.file_name(path: Text) -> Text?
```

The last part of the path: `file_name("a/b.txt")` is `"b.txt"`. `None` when the path ends in `..` or is a root or empty.

#### `std.path.extension`

```text
std.path.extension(path: Text) -> Text?
```

The extension of the last part, without the dot: `extension("a/b.tar.gz")` is `"gz"`. `None` when it has none, as for `"a/b"` and `".hidden"`.

#### `std.path.relative`

```text
std.path.relative(path: Text, base: Text) -> Text?
```

`path` seen from `base`: `relative("a/b/c.txt", "a")` is `"b/c.txt"`. `None` when `path` is not under `base`. It compares parts, not characters, so `"ab/c"` is not under `"a"`, and it never reads the disk.

### `env`

Environment variables (`get`, which needs the `env` permission) and the words given to the program on the command line (`args`).

#### `std.env.args`

```text
std.env.args() -> List[Text]
```

The words after the function name in `telvax run app.tvx main ...`, as they were written (the flags that `run` itself reads, such as `--allow`, are not in the list). It is empty when the program is not started by `telvax run`. Needs no permission.

#### `std.env.get`

```text
std.env.get(name: Text) -> Text?
```

The value of an environment variable, or `None` if it is not set. Needs `env`.

### `process`

The running program.

#### `std.process.exit`

```text
std.process.exit(code: Int) -> Never
```

Ends the program at once with the status `code`, from 0 to 255 (0 is success). The output written so far is flushed. It does not return, so it can end any branch. A `main` that returns an `Err` ends the program with status 1 and the error on the standard error without it.

### `bytes`

`Bytes`: a sequence of bytes that does not change, for a file of any kind, a hash or a body. Made from a `Text` here, read from a file by `std.fs.read_bytes`, or by `std.encoding` and `std.crypto`.

#### `std.bytes.from_text`

```text
std.bytes.from_text(text: Text) -> Bytes
```

The UTF-8 bytes of a text. `to_text` is the way back.

### `encoding`

Bytes as text and back: hexadecimal and base64.

#### `std.encoding.hex`

```text
std.encoding.hex(data: Bytes) -> Text
```

The bytes as lower case hexadecimal, two digits for each byte: `hex(from_text("hi"))` is `"6869"`.

#### `std.encoding.from_hex`

```text
std.encoding.from_hex(text: Text) -> Result[Bytes, Text]
```

The bytes of hexadecimal text, in either case. An odd number of digits or a character that is not a digit is an error that says where.

#### `std.encoding.base64`

```text
std.encoding.base64(data: Bytes) -> Text
```

The bytes as base64 with the standard alphabet and `=` padding (RFC 4648): `base64(from_text("foo"))` is `"Zm9v"`.

#### `std.encoding.from_base64`

```text
std.encoding.from_base64(text: Text) -> Result[Bytes, Text]
```

The bytes of base64 text with the standard alphabet and the padding. It is strict: a line break, a space, a missing or misplaced `=` or a character outside the alphabet is an error.

### `crypto`

Hashes on `Bytes`. They need no capability. For a `Text`, make the bytes with `std.bytes.from_text`.

#### `std.crypto.sha256`

```text
std.crypto.sha256(data: Bytes) -> Bytes
```

The SHA-256 digest of the bytes, 32 bytes long. Use `std.encoding.hex` to write it as text. It is a hash, not a way to store a password.

#### `std.crypto.hmac_sha256`

```text
std.crypto.hmac_sha256(key: Bytes, data: Bytes) -> Bytes
```

The HMAC-SHA256 (RFC 2104) of `data` with `key`, 32 bytes long. The result is `Bytes`, so it can be the key of the next step, as a request signature needs.

### `plot`

Charts as SVG text: give the data and a title, get the image, and write it with `std.fs.write`. The same call gives the same text.

#### `std.plot.bar`

```text
std.plot.bar(labels: List[Text], values: List[Float], title: Text) -> Text
```

A bar chart: one bar for each label, with a grid and the numbers of the vertical axis, 640 by 400. `labels` and `values` have the same length and `values` are finite numbers (a negative one hangs below the zero line); a label longer than 10 characters is cut for the axis. An empty list is an error.

#### `std.plot.line`

```text
std.plot.line(xs: List[Float], ys: List[Float], title: Text) -> Text
```

One series as a line, `ys` against `xs`, with a marker on each point when there are 60 or fewer. The points are joined in the order of `xs`. For dates, use the seconds of a time. `xs` and `ys` have the same length and are finite numbers. An empty list is an error.

#### `std.plot.lines`

```text
std.plot.lines(xs: List[Float], series: Map[Text, List[Float]], title: Text) -> Text
```

Several series against the same `xs`, each in its own color, with a legend of their names; the series are drawn in the order of their names. Each series has as many values as `xs`. The daily average of two scores, for instance: `lines(days, {"textblob": a, "vader": b}, "Sentiment")`.

#### `std.plot.density`

```text
std.plot.density(values: List[Float], bins: Int, title: Text) -> Text
```

A histogram scaled so that its area is 1, with the kernel density estimate drawn over it: a Gaussian kernel with the bandwidth of Scott's rule (the standard deviation times `n^(-1/5)`), as seaborn's `histplot(kde=True)` and SciPy's `gaussian_kde` use by default. `bins` from 1 to 1000.

#### `std.plot.box`

```text
std.plot.box(groups: Map[Text, List[Float]], title: Text) -> Text
```

A box plot for each group, in the order of their names: the box from the first to the third quartile (linear interpolation, as NumPy's default), a line at the median, whiskers to the furthest values within 1.5 times the height of the box (Tukey), and the values beyond them as circles. Every group needs a value.

#### `std.plot.axis_titles`

```text
std.plot.axis_titles(svg: Text, x: Text, y: Text) -> Text
```

A chart of `std.plot` with a title under the horizontal axis and one beside the vertical axis: `axis_titles(bar(...), "region", "sales")`. An empty title is left out.

#### `std.plot.histogram`

```text
std.plot.histogram(values: List[Float], bins: Int, title: Text) -> Text
```

How the `values` are spread: `bins` bars (from 1 to 1000) of equal width between the smallest value and the largest, each as high as the number of values in it; the largest value is in the last bar. The values are finite numbers and there is at least one.

### `metrics`

Counters the program makes, listed by the server's `/metrics`.

#### `std.metrics.count`

```text
std.metrics.count(name: Text, labels: Map[Text, Text] = {}, by: Int = 1)
```

Adds `by` (1 if left out) to the counter called `name`. The server lists it in `/metrics` when it runs with `--metrics`, under its own name with its own `# TYPE`: `name`, or `name{label="value"}` with labels, so `std.metrics.count("jobs_total", {"status": "done"})` is `jobs_total{status="done"}`. A name is letters, digits, `_` and `:` and does not start with a digit (anything else is an error, because `/metrics` lists the name as it is), label names the same without `:`; at most 1,000 label sets per name. Needs no capability.

#### `std.metrics.set`

```text
std.metrics.set(name: Text, value: Float, labels: Map[Text, Text] = {})
```

Sets the gauge called `name` to `value`, an `Int` or a `Float`: a number that goes up and down, such as the entries of a cache. The last call wins. The server lists it in `/metrics` when it runs with `--metrics`, under its own name with `# TYPE name gauge`: `name value`, or `name{label="value"} value` with labels. A value that is not finite is listed as `NaN`, `+Inf` or `-Inf`. The name and the labels follow the rules of `std.metrics.count`, with at most 1,000 label sets per name, and a name is a counter or a gauge, not both (using one for the other is an error). Needs no capability.

### `log`

Log lines in the format and at the level of `telvax serve`.

#### `std.log.debug`

```text
std.log.debug(message: Text, fields: Map[Text, Text] = {})
```

Writes a line at the level `debug`, for what helps while a program is being written. The line goes to the standard error. With `--log-format json` it is `{"ts": ..., "level": ..., "message": ..., <fields>}`; otherwise (and under `telvax run` and `telvax test`) it is `level message key=value`. A field called `ts`, `level` or `message` is left out. Inside a request of `telvax serve` the line also has the `trace_id` of the request and its `request_id`, unless the fields have their own. A line below `--log-level` (`info` by default) is not written. Needs no capability.

#### `std.log.info`

```text
std.log.info(message: Text, fields: Map[Text, Text] = {})
```

Writes a line at the level `info`, for what happened in the normal way. The line goes to the standard error. With `--log-format json` it is `{"ts": ..., "level": ..., "message": ..., <fields>}`; otherwise (and under `telvax run` and `telvax test`) it is `level message key=value`. A field called `ts`, `level` or `message` is left out. Inside a request of `telvax serve` the line also has the `trace_id` of the request and its `request_id`, unless the fields have their own. A line below `--log-level` (`info` by default) is not written. Needs no capability.

#### `std.log.warn`

```text
std.log.warn(message: Text, fields: Map[Text, Text] = {})
```

Writes a line at the level `warn`, for what went wrong but did not stop the work. The line goes to the standard error. With `--log-format json` it is `{"ts": ..., "level": ..., "message": ..., <fields>}`; otherwise (and under `telvax run` and `telvax test`) it is `level message key=value`. A field called `ts`, `level` or `message` is left out. Inside a request of `telvax serve` the line also has the `trace_id` of the request and its `request_id`, unless the fields have their own. A line below `--log-level` (`info` by default) is not written. Needs no capability.

#### `std.log.error`

```text
std.log.error(message: Text, fields: Map[Text, Text] = {})
```

Writes a line at the level `error`, for what failed. The line goes to the standard error. With `--log-format json` it is `{"ts": ..., "level": ..., "message": ..., <fields>}`; otherwise (and under `telvax run` and `telvax test`) it is `level message key=value`. A field called `ts`, `level` or `message` is left out. Inside a request of `telvax serve` the line also has the `trace_id` of the request and its `request_id`, unless the fields have their own. A line below `--log-level` (`info` by default) is not written. Needs no capability.

### `json`

JSON text.

#### `std.json.stringify`

```text
std.json.stringify(value) -> Text
std.json.stringify(value, indent: Int) -> Text
```

The value as JSON text. `None` is `null` and `Some(x)` is just `x`. With `indent`, the text has one item per line, indented by that many spaces per level, as `json.dumps(value, indent=2)` of Python does; `indent` 0 or less gives the same compact text as without it.

#### `std.json.parse`

```text
std.json.parse(text: Text) -> Result[T, Text]
```

Reads JSON into the type written in the annotation: `let q: Result[Query, Text] = std.json.parse(body)`. The error says what did not fit.

### `test`

Assertions and mocks for `telvax test`.

#### `std.test.assert`

```text
std.test.assert(condition: Bool)
```

Fails the test, with `assertion failed`, if the condition is false.

#### `std.test.assert_eq`

```text
std.test.assert_eq(left, right)
```

Fails the test, showing both values, if they are not equal. Comparing two different types is a compile error.

#### `std.test.assert_msg`

```text
std.test.assert_msg(condition: Bool, message: Text)
```

Like `assert`, with your message.

#### `std.test.fail`

```text
std.test.fail(message: Text) -> Never
```

Fails the test with the message.

#### `std.test.mock`

```text
std.test.mock(target, replacement)
```

Replaces a function with another of the same type until `reset_mocks`. `target` is the function, or the path of a std function as `Text`.

#### `std.test.calls`

```text
std.test.calls(target) -> Int
```

How many times a mocked function was called. A function that was not mocked is an error.

#### `std.test.reset_mocks`

```text
std.test.reset_mocks()
```

Removes every mock.

### `debug`

Printing while a program is being written.

#### `std.debug.dbg`

```text
std.debug.dbg(value: T, label: Text) -> T
```

Writes `label = value` to the standard error and returns the value, so it can be put in the middle of an expression.

### `dist`

Running calls on other threads or machines.

#### `std.dist.map`

```text
std.dist.map(items: List[T], work: (T) -> U) -> List[U]
```

Like `std.iter.map`, but `work` must be the name of a top-level function. It runs on local threads, or on the machines of `--workers`, or on Ray from Python.

### `http`

HTTP status values for `@endpoint` functions, and `std.http.client`, to make requests: `get`, `head`, `delete`, `post` and `put`.

#### `std.http.client.get`

```text
std.http.client.get(url: Text)
std.http.client.get(url: Text, headers: Map[Text, Text]) -> Result[HttpResponse, Text]
```

Sends a `GET` and gives the answer. Any status is an answer (`Ok`), a 404 or a 500 included: look at `status()` or `ok()`. Only a request that got no answer (the connection, a timeout of 30 seconds, a URL that is not one) is an `Err`, with the reason. A redirect is not followed: the answer is the 3xx with its `location` header. The body that is read is limited to 256 MB. Inside a request of `telvax serve` it sends a `traceparent` header with the trace of the request and a new span, so the service called can join the trace (a `traceparent` in `headers` is kept instead). Needs `net`, in every mode (`--live` or not), so a test replaces it with `std.test.mock`.

#### `std.http.client.head`

```text
std.http.client.head(url: Text)
std.http.client.head(url: Text, headers: Map[Text, Text]) -> Result[HttpResponse, Text]
```

Like `get`, with a `HEAD`: the headers and the status, no body. Needs `net`.

#### `std.http.client.delete`

```text
std.http.client.delete(url: Text)
std.http.client.delete(url: Text, headers: Map[Text, Text]) -> Result[HttpResponse, Text]
```

Like `get`, with a `DELETE`. Needs `net`.

#### `std.http.client.post`

```text
std.http.client.post(url: Text, body: Text)
std.http.client.post(url: Text, body: Text, headers: Map[Text, Text]) -> Result[HttpResponse, Text]
```

Like `get`, with a `POST`. The body is a `Text` (sent as its UTF-8 bytes) or `Bytes`; no `content-type` is added, so put one in `headers` when the server wants it. Needs `net`.

#### `std.http.client.put`

```text
std.http.client.put(url: Text, body: Text)
std.http.client.put(url: Text, body: Text, headers: Map[Text, Text]) -> Result[HttpResponse, Text]
```

Like `post`, with a `PUT`. Needs `net`.

#### `std.http.Status.BadRequest`

```text
std.http.Status.BadRequest(message: Text) -> Status
```

An HTTP 400. Returning it as the `Err` of an `@endpoint` function answers with that status.

#### `std.http.Status.Unauthorized`

```text
std.http.Status.Unauthorized(message: Text) -> Status
```

An HTTP 401.

#### `std.http.Status.Forbidden`

```text
std.http.Status.Forbidden(message: Text) -> Status
```

An HTTP 403.

#### `std.http.Status.NotFound`

```text
std.http.Status.NotFound(message: Text) -> Status
```

An HTTP 404.

#### `std.http.Status.Unprocessable`

```text
std.http.Status.Unprocessable(message: Text) -> Status
```

An HTTP 422.

#### `std.http.Response.new`

```text
std.http.Response.new(body: T) -> Response
```

An answer with status 200, no headers and `body` written as JSON.

#### `std.http.Response.text`

```text
std.http.Response.text(body: Text) -> Response
```

An answer with status 200 whose body is the text as it is, not written as JSON, with `content-type: text/plain; charset=utf-8`. `.status` and `.header` work as for any `Response`; a `.header("content-type", ...)` replaces the content type.

#### `std.http.Response.html`

```text
std.http.Response.html(body: Text) -> Response
```

The same as `text`, with `content-type: text/html; charset=utf-8`: a page.

#### `std.http.Response.bytes`

```text
std.http.Response.bytes(body: Bytes, content_type: Text) -> Response
```

An answer with status 200 whose body is the bytes as they are, with the given `content-type`: an image, a PDF, a file to download (add `.header("content-disposition", "attachment; filename=report.csv")`). An empty content type is an error.

#### `std.http.Response.events`

```text
std.http.Response.events(producer: fn(Sink)) -> Response
```

An answer of `text/event-stream`. The server calls `producer` while the client reads; each `sink.send(...)` is one event. `.status` and `.header` work as for any `Response`.

#### `std.http.Status.Internal`

```text
std.http.Status.Internal(message: Text) -> Status
```

An HTTP 500.

### `ai`

Embeddings and language models. Without `--live` they are offline stand-ins that need no network.

#### `std.ai.embeddings.OllamaEmbedder.new`

```text
std.ai.embeddings.OllamaEmbedder.new(model: Text) -> OllamaEmbedder
```

An embedder for a model such as `nomic-embed-text`. With `--live` it asks an Ollama server (needs `net`); otherwise a deterministic stand-in makes the vector.

#### `std.ai.llm.OpenAIClient.new`

```text
std.ai.llm.OpenAIClient.new(model: Text) -> OpenAIClient
```

A client for a language model. With `--live` it calls the OpenAI API (needs `net`); otherwise it answers with a fixed stand-in text that quotes the question.

### `db`

A vector database. Without `--live` it is a store in memory.

#### `std.db.postgres.PostgresClient.connect`

```text
std.db.postgres.PostgresClient.connect(url: Text) -> PostgresClient
```

A SQL client for a PostgreSQL server. The connection is opened by the first statement. Needs `--live` and the `net` permission.

#### `std.db.pgvector.PgVectorClient.connect`

```text
std.db.pgvector.PgVectorClient.connect(url: Text) -> PgVectorClient
```

A vector store. With `--live` and a `postgres://` address it uses PostgreSQL with pgvector (needs `net`); otherwise it is a store in memory.

## Types

### `std.http.Request`

The request an `@endpoint` function is answering. Give the function a parameter of this type to read it; it is not part of the payload.

| Method | What it does |
|---|---|
| `header(name: Text) -> Text?` | The first value of the header, or `None`. The name is not case sensitive. |
| `method() -> Text` | The HTTP method, in capitals: `GET`, `POST`, ... |
| `path() -> Text` | The path of the URL, as it was sent, without the query string. |
| `body() -> Bytes` | The body as it came, whatever its content type (empty for `GET` and `DELETE`, and in a `@middleware`, which runs before the body is read). |
| `text() -> Result[Text, Text]` | The body read as UTF-8 text, or the position of the first byte that is not valid. |
| `form() -> Map[Text, Text]` | The text fields of a form body, `application/x-www-form-urlencoded` (an HTML form) or `multipart/form-data`; empty for any other body. A field sent twice keeps the last value. The same fields also fill the payload parameter of the endpoint, as a query string does. |
| `uploads() -> List[Upload]` | The files of a `multipart/form-data` body (an `<input type="file">`), in the order they came. The body of a request is limited to 2 MB. |

### `std.http.Upload`

A file of a `multipart/form-data` request, from `req.uploads()`.

| Method | What it does |
|---|---|
| `name() -> Text` | The name of the form field. |
| `filename() -> Text?` | The name of the file as the client sent it. It comes from the client: do not use it as a path without checking it. |
| `content_type() -> Text?` | The content type the client gave for the file, if any. |
| `bytes() -> Bytes` | The content of the file as it came. |
| `text() -> Result[Text, Text]` | The content of the file read as UTF-8 text, or the position of the first byte that is not valid. |
| `len() -> Int` | The size of the file in bytes. |

### `std.http.Response`

An answer with a status and headers, for an `@endpoint` function that returns more than a body with status 200: `Ok(Response.new(body).status(202).header("location", url))`. The body is written as JSON.

| Method | What it does |
|---|---|
| `status(code: Int) -> Response` | The same answer with this status, from 100 to 599. Any other value is an HTTP 500. |
| `header(name: Text, value: Text) -> Response` | The same answer with this header; it replaces one of the same name, `content-type` included. |

### `std.http.client.HttpResponse`

What `std.http.client` gives back: the status, the headers and the body. The body is kept as bytes, so a page, an image or a file are all fine: `text()` reads it as UTF-8 text and `bytes()` gives it as it came.

| Method | What it does |
|---|---|
| `status() -> Int` | The status code: 200, 404, ... |
| `ok() -> Bool` | Whether the status is from 200 to 299. |
| `header(name: Text) -> Text?` | The value of a header, whatever the case of `name`, or `None`. A header that came more than once is joined with a comma. |
| `headers() -> Map[Text, Text]` | Every header, with the names in lower case; one that came more than once is joined with a comma. |
| `text() -> Result[Text, Text]` | The body read as UTF-8 text, or the position of the first byte that is not valid (use `bytes()` then). |
| `bytes() -> Bytes` | The body as it came. |

### `std.bytes.Bytes`

Made by `std.bytes.from_text`, `std.fs.read_bytes`, `std.encoding` or `std.crypto`. A sequence of bytes that never changes: `slice` and `concat` give new ones. Printing one shows its length (`Bytes(3)`), not its content: use `std.encoding.hex` to see it.

| Method | What it does |
|---|---|
| `len() -> Int` | How many bytes there are. |
| `is_empty() -> Bool` | Whether there are no bytes. |
| `get(index: Int) -> Int?` | The byte at the index, from 0 to 255, or `None` when the index is negative or past the end. |
| `slice(from: Int, to: Int) -> Bytes` | The bytes from `from` up to, not including, `to`. A negative index counts from the end and one past either end stops at it, as for `xs[from:to]`. |
| `concat(other: Bytes) -> Bytes` | The bytes followed by `other`, as new bytes. |
| `to_text() -> Result[Text, Text]` | The bytes read as UTF-8 text, or the position of the first byte that is not valid. |
| `to_list() -> List[Int]` | Each byte as a whole number from 0 to 255. |

### `std.ml.Scaler`

Made by `std.ml.Scaler.fit(values)`: the mean and the population standard deviation of the training values, as scikit-learn's `StandardScaler` (a deviation of 0 scales by 1). `apply` uses them on other values, so the test rows are scaled with what the training rows gave.

| Method | What it does |
|---|---|
| `apply(values: List[Float]) -> List[Float]` | Each value minus the training mean, divided by the training deviation. |
| `inverse(values: List[Float]) -> List[Float]` | Undoes `apply`: a scaled prediction back in the units of the data. |
| `mean() -> Float` | The training mean. |
| `std() -> Float` | The training standard deviation (1 when it was 0). |

### `std.ml.LabelEncoder`

Made by `std.ml.LabelEncoder.fit(values)`: the classes of the training values, in order.

| Method | What it does |
|---|---|
| `encode(values: List[Text]) -> List[Int]` | Each value as the position of its class. A value that was not in training is an error that lists the classes. |
| `decode(ids: List[Int]) -> List[Text]` | Each position back as its class: the predictions of a model as labels. |
| `classes() -> List[Text]` | The classes, in order. |

### `std.ml.Features`

Made by `std.ml.Features.fit(train, target, numeric, categorical)`: everything learned from the training rows that turns a row into the input of a model. Because it is one value, the same scaling and encoding go wherever the model goes: an endpoint calls `transform` on its request, so a prediction cannot skip the preprocessing. `transform` gives the columns in the order of `names()`: the numeric ones, then one per class of each categorical one (`region=north`). A class not seen in training gives zeros, as scikit-learn's `OneHotEncoder(handle_unknown="ignore")`.

| Method | What it does |
|---|---|
| `transform(t: Table[Row]) -> Tensor[f32, N, D]` | The rows of `t` as a matrix with one row per row of `t` and `width()` columns, using what was learned in training. The table needs the feature columns; the target is not read. |
| `names() -> List[Text]` | The name of each column of `transform`: the numeric features, then `column=class` for each class of each categorical one. |
| `width() -> Int` | How many columns `transform` gives. |
| `target() -> Text` | The column the model predicts. |

### `std.sparse.Sparse`

Made by `std.sparse.from_rows`, `from_triplets` or `from_dense`. A matrix of `f32` in compressed sparse row form: each row keeps its values that are not zero, in order of column, so memory grows with the number of those and not with rows times columns. It does not change: each method gives a new matrix. The shape is not part of the type, so the sizes of `matvec` and `matmul` are checked when they run. The products, sums and scaling run in native code.

| Method | What it does |
|---|---|
| `rows() -> Int` | The number of rows. |
| `cols() -> Int` | The number of columns. |
| `nnz() -> Int` | How many values are stored: the ones that are not zero. |
| `get(row: Int, col: Int) -> Float` | The value at a place, 0 when nothing is stored there. A place outside the matrix is an error. |
| `row(i: Int) -> List[(Int, Float)]` | The `(column, value)` pairs of row `i`, in order of column. |
| `select_rows(rows: List[Int]) -> Sparse` | The rows in the order given (a train/test split); a row given twice appears twice. |
| `select_cols(cols: List[Int]) -> Sparse` | The columns in the order given (the kept features): column `cols[j]` becomes column `j`. |
| `row_sums() -> List[Float]` | The sum of each row. |
| `col_sums() -> List[Float]` | The sum of each column. |
| `row_counts() -> List[Int]` | How many values each row stores. |
| `col_counts() -> List[Int]` | How many rows have a value in each column: the document frequency of a word. |
| `normalize_rows() -> Sparse` | Each row divided by its length (L2 norm), so it has length 1; an empty row stays empty. |
| `transpose() -> Sparse` | Rows become columns. |
| `scale(k: Float) -> Sparse` | Every value times `k`. |
| `scale_rows(weights: List[Float]) -> Sparse` | Row `i` times `weights[i]`; the list has one weight per row. |
| `scale_cols(weights: List[Float]) -> Sparse` | Column `j` times `weights[j]` (the idf weights of TF-IDF); the list has one weight per column. |
| `matvec(x: Tensor[f32, C]) -> Tensor[f32, R]` | The matrix times a vector with one value per column. Another length is an error. |
| `matmul(b: Tensor[f32, C, K]) -> Tensor[f32, R, K]` | The matrix times a dense matrix with one row per column of this one. Other sizes are an error. |
| `to_dense() -> Tensor[f32, R, C]` | The whole matrix as a dense tensor, zeros included: rows times columns values, so only for a small one. |

### `std.random.Generator`

Made by `std.random.Generator.new(seed)`. The algorithm is documented and fixed: xoshiro256** with its four 64-bit words filled by splitmix64 from the bits of the seed; `float` is the top 53 bits of a draw divided by 2^53; a whole number below `n` is drawn by Lemire's multiply-and-reject method, so all are equally likely; `shuffle` and `sample` are a Fisher-Yates shuffle from the front. Changing any of this would change what a program prints for a seed, so it is a breaking change. The numbers are not those of NumPy: a program is reproducible within Telvax. A `Generator` is a handle: the places that hold it draw from one sequence, and two calls at once are taken one after the other.

| Method | What it does |
|---|---|
| `int(low: Int, high: Int) -> Int` | The next whole number from `low` to `high`, both included, all equally likely. `low` above `high` is an error. |
| `float() -> Float` | The next number from 0 up to, not including, 1, with 53 random bits. |
| `choice(items: List[T]) -> T` | One item of the list, chosen at random. An empty list is an error. |
| `shuffle(items: List[T]) -> List[T]` | A new list with the same items in a random order. The list that was given is not changed. |
| `sample(items: List[T], n: Int) -> List[T]` | `n` different items of the list, in random order (a list of `n` places, not a draw with repeats). `n` is from 0 to the length of the list. The list that was given is not changed. |

### `std.cache.Cache`

Made by `std.cache.new(max, ttl_seconds)`. Keys are `Text`. `get` and `set` count as a use of the key; an entry older than the ttl is not returned.

| Method | What it does |
|---|---|
| `get(key: Text) -> V?` | The value of the key, or `None` if it is not there or has expired. Counts as a use of the key. |
| `set(key: Text, value: V)` | Stores the value, replacing the old one. If the cache is full, the entry used least recently is dropped. |
| `remove(key: Text) -> V?` | Removes the key and gives its value, or `None`. |
| `clear() -> Int` | Removes every entry and says how many there were. |
| `len() -> Int` | How many entries there are (an expired one counts until it is met). |

### `std.http.Next`

The rest of the chain, as the second parameter of a `@middleware` function: `next(req)` runs the next middleware and the route and gives their `Response`, which the middleware may change (`.header(...)`, `.status(...)`) or return as it is. A middleware that does not call `next` answers with its own `Response` and the route does not run. `next` can be called once.

### `std.http.Sink`

What the producer of `Response.events` receives. `send(data)` writes one event, `event(name, data)` a named one and `comment(text)` a comment.

| Method | What it does |
|---|---|
| `send(data: Text)` | Writes one event. A `Text` is sent as it is, any other value as JSON. A text with several lines becomes several `data:` lines. If the client has gone, this is an error that ends the producer. |
| `event(name: Text, data: Text)` | Writes an event with a name (an `event:` line), such as `done`. |
| `comment(text: Text)` | Writes a comment line (starts with `:`). Clients ignore it; it keeps the connection open. |

### `std.db.postgres.PostgresClient`

Made by `PostgresClient.connect(url)`. Its methods `execute` and `query` run SQL on a PostgreSQL server. Needs `--live` and the `net` permission.

| Method | What it does |
|---|---|
| `execute(sql: Text, params) -> Result[Int, Text]` | Runs a statement and gives the number of rows it changed. `params` fill `$1`, `$2`, ... and are never written into the SQL: a list when they are of one type (`["a", "b"]`), a tuple when not (`("done", 3)`). `None` is `NULL`. |
| `query(sql: Text, params) -> Result[List[Row], Text]` | Runs a query and gives its rows as a list of a `type` of the program, which must be written down: `let rows: List[Job] = db.query(sql, [id])?`. A field is read from the column of the same name; `T?` allows `NULL`. `json` and `jsonb` come as `Text`; a `timestamptz`, `numeric` or `uuid` column must be cast in the SQL (`created_at::text`). |

### `std.http.Status`

An HTTP status with a message. The functions that make one are `std.http.Status.BadRequest` and the others listed under `http`.

### `std.ai.embeddings.OllamaEmbedder`

Made by `OllamaEmbedder.new(model)`. Its method `embed(text)` gives a `Tensor[f32, 768]`.

| Method | What it does |
|---|---|
| `embed(text: Text) -> Tensor[f32, 768]` | The embedding of the text. |

### `std.ai.llm.OpenAIClient`

Made by `OpenAIClient.new(model)`. `complete(prompt)` gives the answer to one prompt as `Text`; `chat(messages)` and `stream(messages, on_delta)` hold a conversation. The methods `with_system`, `temperature`, `max_tokens`, `stop`, `with_key` and `with_base_url` give a new client with that option, so a client can be set up once and shared. Live, it talks to any OpenAI-compatible server: `OPENAI_BASE_URL` or `with_base_url` (a local model, Gemini's OpenAI-compatible endpoint, ...).

| Method | What it does |
|---|---|
| `chat(messages: List[(Text, Text)]) -> Result[LlmReply, LlmError]` | Sends the conversation, a list of `(role, text)` with the roles `system`, `user` and `assistant` (the earlier turns are the memory), and gives the whole answer. The system prompt of `with_system` goes first. Offline it gives a fixed text that quotes the last message of the user. Needs `net` when live. |
| `stream(messages: List[(Text, Text)], on_delta: (Text) -> ()) -> Result[LlmReply, LlmError]` | Like `chat`, and calls `on_delta` with each piece of the answer as it comes, so a service can write it to the `Sink` of `Response.events` with no buffer. The whole answer is given at the end. An error of `on_delta` stops it. |
| `with_system(prompt: Text) -> OpenAIClient` | A client with this system prompt, sent first with each conversation. |
| `temperature(t: Float) -> OpenAIClient` | A client with this sampling temperature, from 0 to 2. |
| `max_tokens(n: Int) -> OpenAIClient` | A client whose answers are at most `n` tokens long. |
| `stop(words: List[Text]) -> OpenAIClient` | A client whose answers end before any of these texts. |
| `with_key(key: Text) -> OpenAIClient` | A client with this API key instead of `OPENAI_API_KEY`: the key a visitor gives, for instance. Without any key a live call is an `invalid_key` error. |
| `with_base_url(url: Text) -> OpenAIClient` | A client that talks to this OpenAI-compatible server instead of `OPENAI_BASE_URL` (`http://localhost:11434/v1` for Ollama). |
| `complete(prompt: Text) -> Text` | The model's answer to the prompt. |

### `std.ai.llm.LlmReply`

What `chat` and `stream` give: the text of the answer, why the model stopped, and the tokens it counted.

| Method | What it does |
|---|---|
| `text() -> Text` | The answer. |
| `stop_reason() -> Text` | Why the model stopped: `stop` (it was done), `length` (`max_tokens` was reached), ... |
| `input_tokens() -> Int` | The tokens of the conversation, as the server counted them (0 when it does not say). |
| `output_tokens() -> Int` | The tokens of the answer, as the server counted them (0 when it does not say). |

### `std.ai.llm.LlmError`

Why `chat` or `stream` failed, as a `kind` a program can test (`if e.kind() == "rate_limit"`): `invalid_key`, `model_not_found`, `rate_limit` (with `retry_after`), `refused` (the content filter), `timeout`, `server`, `network` or `bad_request`, and a `message` for people.

| Method | What it does |
|---|---|
| `kind() -> Text` | `invalid_key`, `model_not_found`, `rate_limit`, `refused`, `timeout`, `server`, `network` or `bad_request`. |
| `message() -> Text` | What happened, for a log or a person. |
| `retry_after() -> Float?` | For `rate_limit`, the seconds the server asked to wait, when it said. |

### `std.db.pgvector.PgVectorClient`

Made by `PgVectorClient.connect(url)`. Its methods store vectors with `insert` and build a search with `search_collection`, `with_vector` and `limit`; `execute` runs it and gives rows with `id`, `text` and `score`. The most similar come first; the default limit is 10.

| Method | What it does |
|---|---|
| `insert(collection: Text, id: Text, text: Text, vector: Tensor[f32, 768])` | Stores a document with its vector in a collection. |
| `search_collection(collection: Text) -> VectorQuery` | Starts a search in a collection. |
| `with_vector(vector: Tensor[f32, 768]) -> VectorQuery` | On a query: sets the vector to search for. A search needs one. |
| `limit(n: Int) -> VectorQuery` | On a query: sets how many rows to return; the default is 10. |
| `execute() -> List` | On a query: runs the search. The rows have `id`, `text` and `score`, the most similar first. |

### `std.db.pgvector.VectorQuery`

A search being built: the value of `search_collection`, `with_vector` and `limit`, and the argument of `execute`.

## Methods

The methods the language gives to its own types. `T`, `U`, `E` stand for any type.

### `Text`

| Method | What it does |
|---|---|
| `is_empty() -> Bool` | Whether the text has no characters. |
| `len() -> u64` | The length in bytes (a letter with an accent is more than one). |
| `trim() -> Text` | Removes whitespace at both ends. |
| `to_lowercase() -> Text` | The text in lower case. |
| `to_uppercase() -> Text` | The text in upper case. |
| `contains(part: Text) -> Bool` | Whether `part` appears in the text. |
| `starts_with(prefix: Text) -> Bool` | Whether the text begins with `prefix`. |
| `ends_with(suffix: Text) -> Bool` | Whether the text ends with `suffix`. |
| `replace(from: Text, to: Text) -> Text` | Every `from` replaced by `to`. |
| `split(sep: Text) -> List[Text]` | Cuts the text at every `sep`. An empty `sep` is an error. |
| `find(part: Text) -> u64?` | The byte position of the first `part`, or `None`. |
| `to_int() -> i64?` | The text as an `Int`, ignoring whitespace at the ends, or `None` if it is not one. |
| `to_float() -> f64?` | The text as a `Float`, ignoring whitespace at the ends, or `None`. |
| `chars() -> List[Text]` | The characters, each as a `Text`. |
| `lines() -> List[Text]` | The lines, without their line breaks. |
| `slice(start, end) -> Text` | The characters from `start` up to `end`; see `std.text.slice`. |

### `List`

| Method | What it does |
|---|---|
| `is_empty() -> Bool` | Whether the list has no items. |
| `len() -> u64` | The number of items. |
| `push(x: T)` | Adds an item at the end. Lists are shared: every name for the list sees it. |
| `contains(x: T) -> Bool` | Whether an item equal to `x` is in the list. |
| `first() -> T?` | The first item, or `None`. |
| `last() -> T?` | The last item, or `None`. |
| `pop() -> T?` | Removes and returns the last item, or `None`. |
| `get(i) -> T?` | The item at position `i`, or `None` if there is none. A negative `i` gives `None`. |
| `sort()` | Orders the list in place, smallest first. The items must be numbers, text or `Bool`. |
| `reverse()` | Reverses the list in place. |
| `map(f: (T) -> U) -> List[U]` | See `std.iter.map`. |
| `filter(f: (T) -> Bool) -> List[T]` | See `std.iter.filter`. |
| `take(n) -> List[T]` | See `std.iter.take`. |
| `skip(n) -> List[T]` | See `std.iter.skip`. |
| `count(f: (T) -> Bool) -> u64` | How many items make `f` true. |
| `any(f: (T) -> Bool) -> Bool` | See `std.iter.any`. |
| `all(f: (T) -> Bool) -> Bool` | See `std.iter.all`. |
| `find(f: (T) -> Bool) -> T?` | See `std.iter.find`. |
| `for_each(f: (T) -> ())` | See `std.iter.for_each`. |
| `fold(init: A, f: (A, T) -> A) -> A` | See `std.iter.fold`. |
| `enumerate() -> List[(u64, T)]` | See `std.iter.enumerate`. |
| `zip(other: List[U]) -> List[(T, U)]` | See `std.iter.zip`. |
| `sum() -> T` | See `std.iter.sum`. |
| `min() -> T?` | See `std.iter.min`. |
| `max() -> T?` | See `std.iter.max`. |
| `join(sep: Text) -> Text` | See `std.string.join`. |
| `slice(start, end) -> List[T]` | See `std.iter.slice`. |

### `Option`

| Method | What it does |
|---|---|
| `is_some() -> Bool` | Whether there is a value. |
| `is_none() -> Bool` | Whether there is no value. |
| `unwrap() -> T` | The value. `None` is an error. |
| `unwrap_or(default: T) -> T` | The value, or `default` for `None`. |
| `unwrap_or_else(f: () -> T) -> T` | The value, or the result of `f()` for `None`. |
| `expect(message: Text) -> T` | The value. `None` is an error with your message. |
| `map(f: (T) -> U) -> U?` | `Some(f(value))`, or `None` for `None`. |
| `and_then(f: (T) -> U?) -> U?` | `f(value)`, which is itself an option, or `None` for `None`. |
| `filter(f: (T) -> Bool) -> T?` | The option itself if there is a value and `f(value)` is true, otherwise `None`. |
| `ok_or(error: E) -> Result[T, E]` | `Ok(value)`, or `Err(error)` for `None`. |

### `Result`

| Method | What it does |
|---|---|
| `is_ok() -> Bool` | Whether it is an `Ok`. |
| `is_err() -> Bool` | Whether it is an `Err`. |
| `unwrap() -> T` | The value of an `Ok`. An `Err` is an error. |
| `expect(message: Text) -> T` | The value of an `Ok`. An `Err` is an error with your message. |
| `unwrap_or(default: T) -> T` | The value of an `Ok`, or `default` for an `Err`. |
| `unwrap_or_else(f: (E) -> T) -> T` | The value of an `Ok`, or `f(error)` for an `Err`. |
| `map(f: (T) -> U) -> Result[U, E]` | Applies `f` to the value of an `Ok`; an `Err` stays as it is. |
| `map_err(f: (E) -> F) -> Result[T, F]` | Applies `f` to the error of an `Err`; an `Ok` stays as it is. |
| `and_then(f: (T) -> Result[U, E]) -> Result[U, E]` | For an `Ok`, `f(value)`, which is itself a `Result` with the same error type; an `Err` stays as it is. |
| `ok() -> T?` | `Some(value)` for an `Ok`, otherwise `None`. |
| `err() -> E?` | `Some(error)` for an `Err`, otherwise `None`. |

### `Map`

| Method | What it does |
|---|---|
| `insert(key: K, value: V)` | Sets the value of a key, replacing the old one. Maps are shared like lists. |
| `get(key: K) -> V?` | The value of a key, or `None`. |
| `remove(key: K) -> V?` | Removes a key and returns its value, or `None`. |
| `contains_key(key: K) -> Bool` | Whether the key is in the map. |
| `len() -> u64` | The number of entries. |
| `is_empty() -> Bool` | Whether the map has no entries. |
| `keys() -> List[K]` | The keys, in key order. |
| `values() -> List[V]` | The values, in key order. |
| `entries() -> List[(K, V)]` | The `(key, value)` pairs, in key order. |

### `Table`

| Method | What it does |
|---|---|
| `len() -> u64` | The number of rows. |
| `is_empty() -> Bool` | Whether the table has no rows. |
| `to_rows() -> List[row]` | See `std.table.to_rows`. |
| `sql(query: Text) -> Table` | See `std.table.sql`. |
| `filter(keep: (row) -> Bool) -> Table` | See `std.table.filter`. |
| `head(n: Int) -> Table` | See `std.table.head`. |
| `tail(n: Int) -> Table` | See `std.table.tail`. |
| `sort_by(column: Text) -> Table` | See `std.table.sort_by`. |
| `sort_by_desc(column: Text) -> Table` | See `std.table.sort_by_desc`. |
| `concat(other: Table) -> Table` | See `std.table.concat`. |
| `split(rows: Int) -> List[Table]` | See `std.table.split`. |
| `select(columns: List[Text]) -> Table` | See `std.table.select`. |
| `dropna(columns: List[Text]) -> Table` | See `std.table.dropna`. |
| `sample(n: Int, seed: Int) -> Table` | See `std.table.sample`. |
| `with_column(name: Text, f: (row) -> T) -> Table` | See `std.table.with_column`. |
| `group_by(keys: List[Text], aggregations: List[Text]) -> Table` | See `std.table.group_by`. |

### `Tensor`

| Method | What it does |
|---|---|
| `add(other) -> Tensor` | See `std.tensor.add`. |
| `sub(other) -> Tensor` | See `std.tensor.sub`. |
| `mul(other) -> Tensor` | See `std.tensor.mul`. |
| `div(other) -> Tensor` | See `std.tensor.div`. |
| `scale(k) -> Tensor` | See `std.tensor.scale`. |
| `relu() -> Tensor` | See `std.tensor.relu`. |
| `sigmoid() -> Tensor` | See `std.tensor.sigmoid`. |
| `ln() -> Tensor` | See `std.tensor.ln`. |
| `exp() -> Tensor` | See `std.tensor.exp`. |
| `softmax() -> Tensor` | See `std.tensor.softmax`. |
| `sum() -> number` | See `std.tensor.sum`. |
| `mean() -> number` | See `std.tensor.mean`. |
| `argmax() -> u64` | See `std.tensor.argmax`. |
| `dot(other) -> number` | See `std.tensor.dot`. |
| `matmul(other) -> Tensor` | See `std.tensor.matmul`. |
| `transpose() -> Tensor` | See `std.tensor.transpose`. |
| `reshape(shape) -> Tensor` | See `std.tensor.reshape`. |
| `to_vec() -> List` | See `std.tensor.to_vec`. |

