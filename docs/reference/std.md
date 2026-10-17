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

### `string`

More functions on `Text`, and `join` for lists.

#### `std.string.join`

```text
std.string.join(items: List[T], sep: Text) -> Text
```

Shows every item as text and joins them with `sep`.

#### `std.string.slice`

```text
std.string.slice(text: Text, start: Int, end: Int) -> Text
```

The characters from `start` up to, not including, `end`. A negative index counts from the end; an index past the end stops at the end; `start` after `end` gives an empty text.

#### `std.string.repeat`

```text
std.string.repeat(text: Text, n: Int) -> Text
```

The text `n` times. A negative `n` is an error.

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
```

The number of items.

#### `std.iter.slice`

```text
std.iter.slice(items: List[T], start: Int, end: Int) -> List[T]
```

The items from `start` up to, not including, `end`, with the index rules of `std.string.slice`.

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

### `env`

Environment variables. Needs the `env` permission.

#### `std.env.get`

```text
std.env.get(name: Text) -> Text?
```

The value of an environment variable, or `None` if it is not set. Needs `env`.

### `json`

JSON text.

#### `std.json.stringify`

```text
std.json.stringify(value) -> Text
```

The value as JSON text. `None` is `null` and `Some(x)` is just `x`.

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

HTTP status values for `@endpoint` functions.

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

#### `std.db.pgvector.PgVectorClient.connect`

```text
std.db.pgvector.PgVectorClient.connect(url: Text) -> PgVectorClient
```

A vector store. With `--live` and a `postgres://` address it uses PostgreSQL with pgvector (needs `net`); otherwise it is a store in memory.

## Types

### `std.http.Request`

Declared so that `import std.http.Request` works. It has no members yet.

### `std.http.Response`

Declared so that `import std.http.Response` works. It has no members yet.

### `std.http.Status`

An HTTP status with a message. The functions that make one are `std.http.Status.BadRequest` and the others listed under `http`.

### `std.ai.embeddings.OllamaEmbedder`

Made by `OllamaEmbedder.new(model)`. Its method `embed(text)` gives a `Tensor[f32, 768]`.

| Method | What it does |
|---|---|
| `embed(text: Text) -> Tensor[f32, 768]` | The embedding of the text. |

### `std.ai.llm.OpenAIClient`

Made by `OpenAIClient.new(model)`. Its method `complete(prompt)` gives the answer as `Text`.

| Method | What it does |
|---|---|
| `complete(prompt: Text) -> Text` | The model's answer to the prompt. |

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
| `slice(start, end) -> Text` | The characters from `start` up to `end`; see `std.string.slice`. |

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
| `sql(query: Text) -> Table` | See `std.table.sql`. |
| `filter(keep: (row) -> Bool) -> Table` | See `std.table.filter`. |
| `head(n: Int) -> Table` | See `std.table.head`. |
| `tail(n: Int) -> Table` | See `std.table.tail`. |
| `sort_by(column: Text) -> Table` | See `std.table.sort_by`. |
| `sort_by_desc(column: Text) -> Table` | See `std.table.sort_by_desc`. |
| `concat(other: Table) -> Table` | See `std.table.concat`. |
| `split(rows: Int) -> List[Table]` | See `std.table.split`. |
| `select(columns: List[Text]) -> Table` | See `std.table.select`. |
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
| `softmax() -> Tensor` | See `std.tensor.softmax`. |
| `sum() -> number` | See `std.tensor.sum`. |
| `mean() -> number` | See `std.tensor.mean`. |
| `argmax() -> u64` | See `std.tensor.argmax`. |
| `dot(other) -> number` | See `std.tensor.dot`. |
| `matmul(other) -> Tensor` | See `std.tensor.matmul`. |
| `transpose() -> Tensor` | See `std.tensor.transpose`. |
| `reshape(shape) -> Tensor` | See `std.tensor.reshape`. |
| `to_vec() -> List` | See `std.tensor.to_vec`. |

