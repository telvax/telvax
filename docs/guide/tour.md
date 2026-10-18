# A tour of Telvax

Telvax is easy to read for anyone who knows Python: blocks are made by indentation, a block opens
with `:`, and there are no braces or semicolons. It keeps its own words (`fn`, `let` / `var`,
`type`, `x => x + 1`, `Int?`) and it checks before the program runs what Python finds out
halfway through: every name has a type, written or inferred.

The code in this guide is tested: each block that is not marked `ignore` is checked, formatted and,
when it has `fn main`, run. Run them from the root of the repository.

## Functions, types and inference

```telvax
type Point:
    x: Float
    y: Float

fn norm(p: Point) -> Float:
    p.x * p.x + p.y * p.y

fn main() -> i32:
    let p = Point(x: 3.0, y: 4.0)
    var total = 0  # an untyped literal: its type comes from the uses
    for i in range(10):
        total += i
    print(f"norm = {norm(p)}, total = {total}")
    total
```

* A `let` outside a function may be written in capitals (`MAX_TEXTS`), and its methods and fields work as for any name.
* `let x = 1` makes a name that cannot change; `var x = 1` makes one that can (`x += 1`).
  `let x: Int = 1` writes the type down. The last expression of a function is its value, so
  `return` is only needed to leave early.
* Types start with a capital, like the types you write yourself: `Int` (64 bits), `Float`
  (64 bits), `Bool`, `Text`, `List[T]`, `Map[K, V]`, `T?` (a value or `None`), `Result[T, E]`,
  `Tensor[T, dims]` and `Table[Row]` (a table's columns may also be `Date` and `Timestamp`). When the size matters: `i8`, `i16`, `i32`, `u8` to `u64`,
  `f32`, `f64`.
* A function without `->` returns the type of its last expression: `fn double(x: Int): x * 2` returns
  an `Int`. It works for methods too (`fn area(self): self.r * self.r`), and a method that implements a
  trait returns what the trait declares for it. Write the `->` for `main`, tests, endpoints and generic
  functions, and when the function has an error.
* A name that starts with `_` is private to its file; everything else can be imported.
* Control flow: `if` / `elif` / `else`, `while`, `for x in items`, `for i in range(n)`,
  `for i in range(a, b)`, `break`, `continue`, `pass` and `return`. `a if cond else b` is an
  expression. `and`, `or`, `not`, `in` and `not in` are the operators.
* Comments start with `#`. Strings use `"` or `'`, and `f"..."` interpolates `{expr}`.

## Named arguments and defaults

A call can name its arguments, and a parameter or a field can have a default, so a configuration
reads like a sentence and the call that needs only the common case stays short:

```telvax
type Query:
    text: Text
    top_k: Int = 5

fn search(q: Query, limit: Int = 10, rerank: Bool = False) -> List[Text]:
    []

let q = Query(text: "tensor shapes")  # top_k is 5
let hits = search(q, rerank: True)  # limit is 10
```

Names are checked against the function: a misspelled or repeated name is a compile error.

## Pipelines

`x |> f(a)` means `f(x, a)`, so stages read left to right. A line that starts with `|>` continues
the line before it:

```telvax ignore
let answer = prompt |> llm.complete() |> std.text.trim()

let docs = query
    |> embed()
    |> store.search(3)
```

The rule is one: `x |> stage` is the call `stage` with `x` as its first argument, and then it is a call
like any other.

* The stage is a function (`x |> f`, `x |> std.text.trim`), a call (`x |> f(a)`) or a closure
  (`x |> (n => n + 1)`).
* Names and defaults count the value the pipe provides: with `fn scale(x: Int, k: Int = 2)`, both
  `1 |> scale` and `1 |> scale(k: 3)` work.
* A suffix after the stage applies to the result of the call: `s |> parse()?` is `parse(s)?`,
  `n |> make().field` is `make(n).field`, and `n |> items()[0]` is `items(n)[0]`.
* `|>` binds looser than every operator: `a + b |> f` pipes the sum, and `x |> f == 1` pipes into
  `f == 1`, which is an error. Write `(x |> f()) == 1`.

## Tensors and tables

```telvax
fn forward(x: Tensor[f32, 2, 3], w: Tensor[f32, 3, 2]) -> Tensor[f32, 2, 2]:
    x |> std.tensor.matmul(w) |> std.tensor.relu()  # [2,3] x [3,2] -> [2,2], checked

type Row:
    id: Int
    score: f32

fn best(t: Table[Row]) -> Table[Row]:
    t |> std.table.sql("SELECT * FROM t WHERE score > 0.5 ORDER BY score DESC")

fn mean_score(t: Table[Row]) -> f32:
    std.tensor.mean(t.score)  # a zero-copy column
```

`std.table.read_csv(path)`, `read_parquet` and `read_ipc` need the result type to be written down
(`let t: Table[Row] = ...`), as do `std.tensor.zeros()`, `from_vec` and `reshape`.

A table has methods that keep its rows' type. They do not need SQL, so they also work in `telvax-lite`:

```telvax
type Row:
    id: Int
    score: f32

fn top_two(t: Table[Row]) -> Table[Row]:
    t.sort_by_desc("score").head(2)

fn passed(t: Table[Row]) -> Table[Row]:
    t.filter(r => r.score > 0.5)
```

`head(n)` and `tail(n)` take the first or last `n` rows, `sort_by(column)` and `sort_by_desc(column)` order by one
column, `filter(keep)` keeps the rows for which `keep` is true, and `a.concat(b)` appends the rows of `b`
(both tables need the same row type). A column name that does not exist is a runtime error that lists the columns.

`t.select(["name", "id"])` keeps only those columns, in that order. The names are checked while the program is
checked, and the row type of the result is worked out from them, so no annotation is needed:

```telvax
type Person:
    id: Int
    name: Text
    age: Int

fn names(t: Table[Person]) -> List[Text]:
    var out: List[Text] = []
    for r in t.select(["name", "id"]):
        out.push(f"{r.id}: {r.name}")
    out
```

The names must be written as text in a list in place (not variables), and a column that does not exist, or is
named twice, is a compile error.

`t.group_by(keys, aggregations)` makes one row per distinct value of the key columns, in order of first appearance.
Each aggregation is written `column:function` and becomes a column named `<column>_<function>`:

```telvax
type Sale:
    region: Text
    units: Int
    price: f64

fn report(t: Table[Sale]) -> List[Text]:
    var out: List[Text] = []
    for r in t.group_by(["region"], ["units:sum", "price:mean"]).sort_by_desc("units_sum"):
        out.push(f"{r.region}: {r.units_sum} units, {r.price_mean} on average")
    out
```

The functions are `sum` (an `Int` for an integer column, a `Float` for a decimal one), `mean` (a `Float`), `min` and
`max` (the type of the column) and `count` (an `Int`, for any column). The result row type is worked out from the
names, as for `select`. An unknown column or function, a column typed `T?`, a sum or mean of text and a result
column named twice are compile errors. The same query in SQL still works; `group_by` also runs in `telvax-lite`.

A column typed `T?` may hold empty values: `temp: f64?` reads CSV cells that are empty, Parquet nulls and
`None` from `from_rows` as `None`, and `t.temp` is then a `List[f64?]` (not a tensor). A column without `?`
that finds an empty value is an error that names the column and says to write `?`.

```telvax
type Reading:
    id: Int
    temp: f64?

fn warm(t: Table[Reading]) -> Table[Reading]:
    t.filter(r => r.temp.unwrap_or(0.0) > 20.0)
```

`Date` and `Timestamp` are column types for dates and times. In the table they are Arrow `Date32` and
`Timestamp(µs, UTC)`, so Parquet, SQL and PyArrow see real dates. In the program a value is a `Text` in ISO-8601:
`"2026-10-17"` and `"2026-10-17T16:53:03Z"`. `from_rows` and CSV accept any offset (`+02:00`) and store UTC; a
text that is not a date is an error that says to write `YYYY-MM-DD`. There is no date arithmetic yet: compare
the text, sort the table, or use SQL.

```telvax
type Event:
    id: Int
    day: Date
    at: Timestamp?

fn latest(t: Table[Event]) -> Table[Event]:
    t.sort_by_desc("day").head(1)
```

`std.table.read_ipc(path)` and `write_ipc(table, path)` read and write an Arrow IPC file (Feather v2), which PyArrow,
Polars and pandas also read and write. `read_ipc` maps the file into memory and the columns point into it, so a
file with one batch is not copied. Do not rewrite or truncate a file while a table read from it is in use. Files
compressed with LZ4 or Zstd are decoded (copying) in the full `telvax`; `telvax-lite` reads uncompressed files.

A table can be cut into chunks. `t.split(n)` gives a `List[Table[Row]]` of tables with at most `n` rows each;
they share the buffers of `t`, so nothing is copied. `std.table.read_csv_batches(path, n)`,
`read_parquet_batches` and `read_ipc_batches` read a file straight into such chunks (no joined copy of the whole
file is built), and need the result type written down: `let parts: List[Table[Row]] = ...`. Chunks are all in
memory at once; a reader that hands them out one at a time does not exist yet.

```telvax
type Row:
    id: Int
    score: f32

fn total_score(path: Text) -> f32:
    let parts: List[Table[Row]] = std.table.read_parquet_batches(path, 100000)
    var sum = 0.0
    for part in parts:
        sum += part.score.sum()
    sum
```

`std.table.read_flight(url, ticket)` fetches a table from an Arrow Flight server (`DoGet`), and
`std.table.write_flight(table, url, path)` sends one (`DoPut`). The address is `grpc://host:port`, or
`grpc+tls://host:port` for TLS. Both need the `net` permission and a build with the `flight` feature
(`cargo install telvax --features flight`); the released binaries do not include it, and without it they report
that. There is a client only, no Flight server.

## HTTP endpoints

```telvax ignore
@endpoint(POST, "/v1/score")
fn score(input: In) -> Result[Out, Status]:
    ...
```

The parameter type becomes the validated JSON body (GET/DELETE read it from the query string).
Returning `Err(Status.BadRequest("..."))` becomes an HTTP 400.

A field with a default written as a literal (`language: Text = "en"`) or typed `T?` may be left out of the
body; it takes the default, or `None`. Any other field is required.

A `{name}` segment in the path is read from the URL into the parameter with that name:

```telvax ignore
@endpoint(GET, "/v1/results/{job_id}")
fn result(job_id: Text) -> Result[Job, Status]:
    ...

@endpoint(PUT, "/v1/jobs/{id}/steps/{step}")
fn update(id: Int, step: Text, input: Patch) -> Result[Job, Status]:
    ...
```

A path parameter is `Text`, a number type or `Bool`. A value that does not convert is an HTTP 400:
`invalid path parameter id: expected i64, found `x``. The parameter that is not in the path is the
payload, and there is at most one. `/a/{x}` and `/a/{y}` are the same route.

A parameter of type `Request` receives the request. It is not part of the payload, and there is at most one:

```telvax ignore
import std.http.Request

@endpoint(DELETE, "/v1/cache")
fn clear(req: Request) -> Result[Cleared, Status]:
    if req.header("x-admin-token") != Some(token):
        return Err(Status.Unauthorized("invalid token"))
    ...
```

`req.header(name)` is the first value of the header, or `None`; the name is not case sensitive.
`req.method()` and `req.path()` are `Text`.

Values that live outside a function are shared by every request. A top-level `let` that holds a `List` or a
`Map` can be changed from the endpoints, and each `push`, `insert` or `remove` is atomic, so a cache or a table of
jobs needs nothing else:

```telvax ignore
let cache: Map[Text, Scores] = {}
let stored_at: Map[Text, Float] = {}

fn lookup(text: Text) -> Scores?:
    match stored_at.get(text):
        case Some(t):
            if std.time.now() - t < 300.0:
                cache.get(text)
            else:
                None
        case None:
            None
```

A top-level `var` is not allowed, so a counter is a `Map` with one key. Two calls in a row on the same value are
not atomic together: another request can come between `get` and `insert`. There is no size limit or expiry; remove
old keys yourself.

An endpoint answers `200` with the value as JSON. To choose the status or add headers, return a `Response`:

```telvax ignore
import std.http.Response

@endpoint(POST, "/v1/analyze_batch")
fn submit(input: Batch) -> Result[Response, Status]:
    let job = start(input)
    Ok(Response.new(Accepted(job_id: job.id))
        .status(202)
        .header("location", "/v1/results/" + job.id))
```

`Response.new(body)` is a `200`; `.status(code)` takes a number from 100 to 599 (any other is an HTTP 500) and
`.header(name, value)` sets a header, replacing one of the same name, `content-type` included.
`Err(Status...)` still gives the error statuses.

### Work after the answer

`std.task.spawn(task)` starts an `async fn` and returns at once; the task goes on after the endpoint has answered, so
a service can reply `202` and finish later while the client polls:

```telvax ignore
let jobs: Map[Text, Text] = {}

async fn work(id: Text, texts: List[Text]):
    # ... the slow part ...
    jobs.insert(id, "done")

@endpoint(POST, "/v1/jobs")
fn start(input: Batch) -> Result[Response, Status]:
    let id = new_id()
    jobs.insert(id, "running")
    std.task.spawn(work(id, input.texts))
    Ok(Response.new(Accepted(job_id: id)).status(202))
```

* The value of the task is dropped. An error in it is written to the standard error as `background task failed: ...`,
  unless a second argument says what to do with it: `std.task.spawn(work(id), failed)` calls `failed(message)`, a
  function of one `Text`, so the job can be marked as failed instead of staying `running`. A task that must report a
  result writes it where the program reads it, as `jobs` above.
* At most 256 tasks run at once; one more is an error in the endpoint that started it.
* The task shares the program's top-level `List` and `Map` (see the note on shared state above). It ends with the
  process: stopping the server stops it.
* Only a server keeps the task going. In `telvax run` and `telvax test`, `spawn` waits for the task, as `std.task.run` does.

### Server-sent events

`Response.events(producer)` answers with `text/event-stream`. The server calls `producer` with a `Sink` while the
client reads, and each `sink.send(...)` is one event, sent at once:

```telvax ignore
import std.http.Response
import std.http.Sink

@endpoint(POST, "/v1/analyze_stream")
fn stream(input: Batch) -> Result[Response, Status]:
    fn produce(sink: Sink):
        for text in input.texts:
            sink.send(score(text))
        sink.event("done", "{}")
    Ok(Response.events(produce))
```

* `send(data)` takes a `Text`, sent as it is, or any other value, sent as JSON. `event(name, data)` adds an
  `event:` line and `comment(text)` writes a line that clients ignore, to keep the connection open.
* The producer is a function written inside the endpoint (a closure is one line), so it can use the parameters.
* The status and headers are sent first, so an error in the producer cannot change them: the stream ends with
  `event: error` and the message. A client that disconnects makes the next `send` fail, which ends the producer.
* `telvax serve` and `build --serve` stream; the native backend does not serve HTTP.

## SQL on PostgreSQL

`std.db.postgres.PostgresClient` runs SQL on a PostgreSQL server. `execute` gives the number of rows a statement
changed, and `query` gives the rows as a list of a `type` you write:

```telvax ignore
import std.db.postgres.PostgresClient

type Job:
    id: Text
    status: Text
    attempts: Int
    result: Text?

let db = PostgresClient.connect(std.env.get("DATABASE_URL").unwrap())

fn finish(id: Text, result: Text) -> Result[Int, Text]:
    db.execute("UPDATE jobs SET status = $1, result = $2 WHERE id = $3", ("done", result, id))

fn queued() -> Result[List[Job], Text]:
    let rows: List[Job] = db.query("SELECT id, status, attempts, result FROM jobs WHERE status = $1", ["queued"])?
    Ok(rows)
```

* The values for `$1`, `$2` are given apart from the SQL, as a list when they are of one type and as a tuple when
  they are not. They are bound by the server, so a value cannot change the statement. `None` is `NULL`.
* A field is read from the column of the same name, and `Text?` allows `NULL`. A column that is not a field is
  ignored; a field without a column is an error. `json` and `jsonb` columns are `Text`. A `timestamptz`, `numeric` or
  `uuid` column is an error that tells you to cast it in the SQL (`created_at::text`).
* An error of the server is `Err(Text)` with its message, such as the name of the constraint that failed.
* The connection is opened by the first statement and kept. There is one per client, used one statement at a time,
  and no transactions yet.
* It needs `--live` and the `net` permission: there is no offline stand-in for SQL.

### Metrics and logs

`telvax serve app.tvx --metrics --log-format json` serves `GET /metrics` (requests and latency per route) and writes one
JSON line per request with a request id. A program counts what only it knows with `std.metrics.count`, which the
server lists as `telvax_counter_total`:

```telvax ignore
fn lookup(key: Text) -> Text?:
    match cache.get(key):
        case Some(v):
            std.metrics.count("cache_hit")
            Some(v)
        case None:
            std.metrics.count("cache_miss")
            None
```

`--cors "*"`, `--rate-limit 100/min` and `--security-headers` put the usual protections around every answer
without a reverse proxy. The details are in the [command line reference](../reference/cli.md).

### The API document

The types of the endpoints already describe the API, so the server publishes it. `GET /openapi.json` is an OpenAPI 3.0
document: every route with its path parameters, the query parameters or the JSON body, the type of the answer, and
the error answer (`{"error": "..."}`). `GET /docs` is a page that lists them. Both are on every server, and if the
program has its own endpoint at one of those paths, that one is served instead. `GET /healthz` answers `ok`.

A function that returns a `Response` has no typed answer, so its `200` has no schema. A field of type `T?` or
`Date` is described as it is checked, with no more than its JSON type.

## Modules and packages

```telvax ignore
from util import clamp  # ./util.tvx
from mathx.vec import dot  # dependency `mathx`, file vec.tvx
```

A top-level `let` is imported the same way (`from settings import LIMIT`); it is one value for the whole program, so a
`List` or `Map` changed in one module is changed in all. `import util.clamp` brings in the same name. A name can be renamed with `as`:
`from util import clamp as limit`, `import util.clamp as limit`, `from std.text import trim as strip`.
Names from `std` need no import: `std.text.trim(s)`. A module is not a name: `import util` is an error
that says to write `from util import name`.

```sh
tvpm new app && cd app
tvpm add mathx --registry ~/registry     # or --path ../mathx
tvpm install --registry ~/registry       # resolves, writes telvax.lock, downloads
tvpm run                                  # runs src/main.tvx with the dependencies
tvpm update                               # newest versions that fit; `tvpm update mathx` moves only mathx
```

## Distribution

```telvax ignore
fn work(item: Job) -> Done:
    ...

fn run_all(jobs: List[Job]) -> List[Done]:
    std.dist.map(jobs, work)
```

`std.dist.map` takes the *name* of a top-level function. It runs on local threads by default;
`telvax run app.tvx run_all --workers host1:7077,host2:7077` sends the calls to machines running
`telvax worker`, and `telvax.ray.RayExecutor` (Python) sends them to Ray.

## Quantization

`--quantize int8` (on `run`, `serve`, `jit`, `build`) makes `f32` matrix products use symmetric
per-tensor int8 arithmetic with `i32` accumulation. The interpreter and the native backend give
identical results.

`--quantize int8-channel` and `--quantize int4-channel` (`run` and `serve`; the native backend
refuses them) give every row of the activations and every column of the weights its own scale, so
a column that is much larger or smaller than the rest no longer loses its precision. The int4 mode
stores the weights in 4 bits. Scales are computed on each call; there is no calibration step.

## Accelerators

* `--device gpu` (on `run` and `serve`, needs `cargo build --features gpu`) runs `f32` tensor operations on the GPU through `wgpu`: Metal on macOS, Vulkan on Linux, DX12 on Windows.
* `telvax build file.tvx --emit stablehlo` prints StableHLO for the tensor functions in the file, for XLA / PJRT.
* `telvax build file.tvx --emit ptx [--sm sm_80]` prints PTX kernels for the tensor operations the file uses (needs `--features llvm`).

PTX and StableHLO are generated and checked, not executed: see `docs/design/status.md`.

## Enums, `match`, `break` and `continue`

```telvax
enum Shape:
    Circle(Float)
    Rect(Float, Float)
    Empty

fn area(s: Shape) -> Float:
    match s:
        case Shape.Circle(r):
            r * r * 3.14159
        case Shape.Rect(w, h):
            w * h
        case Shape.Empty:
            0.0

fn first_over(limit: i32) -> i32:
    var found = -1
    for i in range(1, 1000):
        if i * i > limit:
            found = i
            break
    found
```

`match` must cover every case (the compiler names the missing variants) and also works on
`Result` (`Ok(v)` / `Err(e)`), integers, strings, booleans and `_`. Enums travel as JSON: a unit
variant is its name (`"Empty"`), one with values is `{"Circle": 2.0}` or `{"Rect": [2.0, 3.0]}`.
Enums run in the interpreter; the native backend does not compile them yet.

## Limiting what a program may do

A program can read and write files, read environment variables and reach the network. `--allow`
on `run`, `test` and `serve` lists what it may do; without the flag everything is allowed.

```sh
telvax run job.tvx main --allow fs.read          # may read files, nothing else
telvax test app.tvx --allow none                 # may touch nothing outside itself
telvax serve app.tvx --live --allow net,env
```

| Word | Allows |
|---|---|
| `fs.read` | `std.fs.read`, `std.fs.exists`, `read_csv`, `read_parquet`, `read_ipc` and their `_batches` forms |
| `fs.write` | `std.fs.write`, `write_csv`, `write_parquet`, `write_ipc` |
| `fs` | `fs.read` and `fs.write` |
| `env` | `std.env.get` |
| `net` | the Ollama, OpenAI and PostgreSQL clients, when `--live`, and `read_flight` / `write_flight` |
| `all`, `none` | everything, nothing |

A call that is not allowed stops the program and says which word to add:
`` `std.fs.read` needs the `fs.read` capability; run with `--allow fs.read` ``. The limit is checked when
the program runs, not when it is compiled, and `std.io.print` and `std.time` are always allowed.

### Files inside the binary

`std.fs.embed("path")` is a `Text` with the content of a file, read when the program is built and kept inside it.
The path is a text literal, relative to the directory of the main file, so `telvax build --serve` makes one binary
that needs no data files next to it:

```telvax
let lexicon = std.fs.embed("lexicons/vader.txt")
```

It needs no capability, because the file is read at build time, not by the running program. A file that does not
exist, or a path that is not a literal (`"data/" + name`), is an error when the program is checked.

## Tests

A function called `test_...` is a test. `assert(cond)` and `assert_eq(a, b)` stop
it with a message when they fail:

```telvax
fn double(x: Int) -> Int:
    x * 2

fn test_double():
    assert_eq(double(2), 4)
```

```sh
telvax test file.tvx           # run them all, exit 1 if one fails
telvax test file.tvx double    # only the tests whose name contains `double`
telvax test tests/             # every `test_*.tvx` and `*_test.tvx` file under a directory
telvax test                    # the same, in this directory
```

### Fixtures

A test that takes parameters gets them from fixtures. `fn test_total(order: Order)` calls
`fixture_order()` and passes the result; a fixture can take other fixtures the same way. Each test
gets new values. A `teardown_order(order: Order)` function, if there is one, runs after the test,
whether it passed or not.

```telvax
fn fixture_base() -> Int:
    10

fn fixture_total(base: Int) -> Int:
    base * 2

fn teardown_total(total: Int):
    print(f"done with {total}")

fn test_total(base: Int, total: Int):
    assert_eq(total, base * 2)
```

A parameter with no `fixture_...` function, or a fixture that returns another type, fails the test
and says what to write.

### Cases

To run one test on several values, write a `cases_...` function with the name of the test without
`test_`. It returns a list; the test runs once for each item. A tuple gives the first parameters of
the test, and the parameters left over are still fixtures.

```telvax ignore
fn cases_double() -> List[(Int, Int)]:
    [(1, 2), (2, 4), (3, 6)]

fn test_double(x: Int, want: Int):
    assert_eq(double(x), want)
```

Each run is listed as `test_double[(2, 4)]`. A `cases_...` function that is empty, or whose items do
not fit the parameters, fails the test and says why.

### Mocks

`std.test.mock(target, replacement)` makes a test call another function in place of one. The target is
one of your functions, written without quotes, or a std function by its path in text. A function
target must have the same type as its replacement; a std target is checked when the test runs.
`std.test.calls(target)` says how many times the replacement was called. Every mock is removed when
the test ends.

```telvax
fn price(item: Text) -> Int:
    100

fn total(item: Text) -> Int:
    price(item) * 2

fn fake_read(path: Text) -> Result[Text, Text]:
    Ok("from the test")

fn test_total():
    std.test.mock(price, item => 7)
    assert_eq(total("a"), 14)
    assert_eq(std.test.calls(price), 1)

fn test_read():
    std.test.mock("std.fs.read", fake_read)
    assert_eq(std.fs.read("any.txt"), Ok("from the test"))
```

A replacement cannot call the function it replaces. Calls through methods (`x.len()`) cannot be
mocked, and `std.test.*` itself cannot.

### Coverage

`--coverage` lists, for each file of the project, the lines with code and how many of them the tests
ran, and the numbers of the lines that did not run. `--fail-under 90` also fails the run, with exit
code 1, when fewer than 90% of the lines ran, so a pipeline can hold the line.

```sh
telvax test tests/ --coverage
telvax test tests/ --fail-under 90
```

```text
coverage (lines that ran / lines with code):
  src/price.tvx  18/20  90.0%   missing 14-15
  total  18/20  90.0%
```

A line counts when it has a statement, the last expression of a block or the body of a `case`. The
`test_`, `fixture_`, `teardown_` and `cases_` functions are not counted, nor are files outside the
project (the directory with `telvax.toml`, or the one you test). The count is of lines, not of
branches: a line with an `if` and its two results written on it counts once.

## Macros

A macro is written `name!(...)`. The compiler has five and you cannot define more. Each one is
rewritten into a call of a `std` function before the program is checked.

| Macro | What it does |
|---|---|
| `assert!(cond)` | like `assert`, and the message quotes the condition: `assertion failed: x > 3` |
| `assert!(cond, "message")` | stops with your message when `cond` is false |
| `dbg!(value)` | prints `value_as_written = result` to stderr and gives `value` back |
| `todo!()` | stops with `not yet implemented`; it can stand where any type is expected |
| `unreachable!()` | stops with `unreachable code was reached` |
| `panic!("message")` | stops with the message |

```telvax
fn pick(n: Int) -> Int:
    if n > 0:
        n
    else:
        todo!()

fn test_pick():
    let x = dbg!(pick(4))  # stderr: pick(4) = 4
    assert!(x == 4)
```

Macros run in the interpreter, as `assert` does; the native backend does not compile them.

## The REPL

```sh
telvax repl                    # an interactive session
telvax repl app.tvx            # start with the definitions and variables of a file
```

```text
>>> let xs = [3, 1, 2]
>>> xs.len()
3
>>> fn total(v: List[Int]) -> Int:
...     v.sum()
...
>>> total(xs)
6
>>> var n = 0
>>> for x in xs:
...     n = n + x
...
>>> n
6
>>> :type xs
List[Int]
```

You can type a definition (`fn`, `type`, `enum`, `trait`, `extend`, `import`), a variable (`let`, `var`), a
statement or an expression; the value of an expression is shown. A block ends with an empty line. Names last
until `:reset`: a name can be defined again (`let x = x + 1`, or a new `fn` of the same name), and a `let`
cannot be assigned to, as in a file. Each input is checked with everything defined before it, so a type error
shows the line and leaves the session as it was. A number written alone (`let n = 4`) is an `Int`, so that
functions that take an `Int` accept it. A closure needs the type of its parameters to be kept
(`let inc = (n: Int) => n + 1`).

Commands start with a colon: `:type <expression>`, `:vars`, `:defs`, `:load <file>`, `:reset`, `:help` and
`:quit`. Tab completes names, fields and methods, and the arrow keys go through earlier inputs (kept in
`~/.telvax_history`; `TELVAX_HISTORY` names another file). `--allow` limits what the session may do, as for
`telvax run`. Each input is run once: what it prints or writes happens once, and values are not computed again
when the next input is entered.

## Debugging

```sh
telvax dap       # a debug adapter on standard input and output
```

In VS Code, F5 on a Telvax file debugs it (see the extension's README). `telvax dap` speaks the [Debug Adapter Protocol](https://microsoft.github.io/debug-adapter-protocol/), so an
editor that has a client for it can run a program in the interpreter, stop it at breakpoints and step through
it. Its launch request takes `program` (the `.tvx` file), `function` (default `main`), `args` (the words
you would give `telvax run`), `stopOnEntry`, `noDebug` and `allow` (the same list as `--allow`). It stops
before each statement, so a breakpoint goes on a line that starts a statement; on any other line it moves
to the next one that does. While stopped it lists the calls in progress and their variables, and a name or
a field of one (`p.x`, `xs.0`) can be evaluated; other expressions cannot. It debugs the interpreter, not
programs from `telvax build`, and has no conditional breakpoints.

## Profiling

```sh
telvax profile app.tvx                       # runs `main` and reports where the time and memory went
telvax profile app.tvx handler 3 "text"      # another function, with its arguments
telvax profile app.tvx --top 30 --json r.json --collapsed stacks.txt --speedscope p.json
```

The report has four parts: the wall time and the memory (the peak, the bytes allocated and how many
allocations); the functions, with calls, the time in the function itself (`self`) and with the calls it
makes (`total`), and the bytes it allocated; the statements, with how often each ran and how long it took
until the next one of the same call started; and the `std` functions (`std.tensor.matmul`,
`std.table.sql`, ...), with calls, time, the rows or elements that went in and came out, and the bytes of the
result. `--json` writes all of it to a file, `--collapsed` writes call stacks for `flamegraph.pl` or
`inferno`, and `--speedscope` writes a file for <https://www.speedscope.app>.

Times are measured by the interpreter around what the program does, so they show where the *program*
spends its time and include the time of the interpreter itself. Memory counts the whole process, so with
several threads a function also counts what the other threads allocate meanwhile. Programs built with
`telvax build` are not profiled.

## Documentation

`telvax schema data.csv` prints the `type` for the columns of a CSV, Parquet or Arrow file, so you do not
write it by hand: a column with an empty value comes out as `T?`, and `--name Row` picks the type's name.

`telvax doc` writes a Markdown reference from the code: each function with its signature, each `type` with
a table of fields (and defaults), each `enum` with its variants, each `trait`, and the methods under `extend`.
The `#` comment lines written directly above an item (no blank line between) become its description.

```sh
telvax doc file.tvx                # print the reference of one file
telvax doc src/ -o REFERENCE.md    # every `.tvx` file under the directory, written to a file
telvax doc --public src/           # leave out names that start with `_`
```

```tvx
# Sum of the first `n` squares.
fn squares(n: Int) -> Int:
    n * (n + 1) * (2 * n + 1) / 6
```

## Formatting and linting

```sh
telvax fmt file.tvx            # rewrite in place
telvax fmt src/ tests/         # every `.tvx` file under the directories (no path: this directory)
telvax fmt --check             # list files that would change (exit 1), for CI
telvax fmt --diff src/         # show the lines that would change, write nothing (exit 1)
telvax lint file.tvx           # unused variables, unused private functions, unreachable cases
telvax lint --list-rules       # the rules and what they find
```

Each lint rule has a name (`unused-variable`, `unused-function`, `unreachable-case`) and a level:
`allow` (say nothing), `warn` (the default) or `deny` (an error, so `telvax lint` fails). Set the
levels for a package in `telvax.toml`, or for one run with `--allow rule`, `--warn rule` and
`--deny rule`; a flag wins over the file. `--deny-warnings` makes any warning fail the run.

```toml
[lint]
unused-variable = "deny"
unused-function = "allow"
```

Indentation is part of the syntax, so `telvax fmt` never moves a line between blocks: it sets four
spaces per level, spaces inside a line, continuation lines and blank lines. Directories skip hidden
folders, `target`, `node_modules` and `.tvpm`.

The language server completes names as you type: types after `:` and `->`, the fields, columns and
methods after a dot (also after calls such as `s.trim().`), the variants after `Shape.`, the parameter of
a closure you are writing (`xs.map(p => p.`) and the columns of a table inside `t.sql("..`. What fits the
type the code expects comes first.

The language server formats documents too, so VS Code can format on save. It also finds all
references to a variable, function, global, type, enum variant, field or method and renames them (a
rename is refused when the new name would collide with another one in scope, when a file has a syntax
error, or when the name belongs to a dependency). They follow imports into the other files of the project.

## Strings and lists

```telvax
fn demo(s: Text) -> u64:
    let words = s.split(" ")  # List[Text]
    words.push("extra")
    if s.contains("telvax") and not s.starts_with("#"):
        return words.len()
    0
```

`Text`: `len`, `is_empty`, `trim`, `to_lowercase`, `to_uppercase`, `contains`, `starts_with`,
`ends_with`, `replace`, `split`. `List`: `len`, `is_empty`, `push`, `contains`. `len` is a `u64`, and next to a signed number (`xs.len() > limit`, `xs.len() - 1`) both are read as `Int`. `x in xs` is
`xs.contains(x)`. A method that does not exist is a compile error with a "did you mean" hint.
These methods run in the interpreter; the native backend still only has `len` and `is_empty`.

## Random values

`std.random.uuid()` is a new identifier each time, such as `"3f2b8c1e9a4d4b7f8e6a0c5d1b2a3f4e"`, and
`std.random.int(1, 6)` a whole number from 1 to 6, both included. They come from the operating system, so an id is
fit to name a job whose result only its owner should read. They need no capability, and there is no seed.

## Optional values, maps and small functions

```telvax
fn demo(text: Text) -> Int:
    let counts: Map[Text, Int] = std.map.new()
    for word in text.split(" "):
        let seen = counts.get(word).unwrap_or(0)  # Int?
        counts.insert(word, seen + 1)
    match counts.get("telvax"):
        case Some(n):
            n
        case None:
            0
```

* `T?` is `Some(value)` or `None`; `match` must cover both. Methods: `is_some`, `is_none`,
  `unwrap` (an error on `None`), `unwrap_or`, `unwrap_or_else(() => v)`, `expect("message")`,
  `map`, `and_then`, `filter` and `ok_or(error)`, which turns it into a `Result`. Several methods return one: `List.first`, `last`,
  `pop`, `get`, `Text.find`, `to_int`, `to_float`, `Map.get`, `remove`. As JSON, `None` is
  `null` and `Some(x)` is just `x`.
* `Map[K, V]` has `Text`, integer or `Bool` keys. It is created with `std.map.new()` and a
  type annotation, and has `insert`, `get`, `remove`, `contains_key`, `len`, `is_empty`, `keys` and
  `values`. Like `List`, copies share the same entries. Keys are kept in order; JSON objects use
  string keys.
* `List` also has `sort` (numbers, strings, bools) and `reverse`; `Text` has `chars` and `lines`.
* Functions are values. The type of one is written like a header without the name:
  `(Int, Int) -> Int`, `() -> Text`, `(Int) -> ()` for one that returns nothing. A name can hold a
  function (`let f = twice`), and a parameter, a field, a list element or a return value can too:
  `fn apply(f: (Int) -> Int, x: Int) -> Int: f(x)`, `fn adder(n: Int) -> (Int) -> Int: x => x + n`.
  A closure parameter can say its type: `(x: Int) => x + 1`. Calling a function with the wrong
  number or types of arguments is a compile error that shows both types.
* `x => x + 1` and `(a, b) => a + b` are one-line functions. For more, write an `fn` inside the
  function: `fn add(a: Int): a + k`. Both capture variables by value, so assigning to a variable
  from outside is a compile error; shared `List` and `Map` values can still be changed from
  inside one.
* These run in the interpreter; the native backend does not compile them yet.

## Tables and tensors, the short way

```telvax
type Sale:
    id: Int
    region: Text
    units: Int
    price: Float

fn main():
    let sales: Table[Sale] = read_csv("examples/dataframe/sales.csv")
    print(f"mean price: {sales.price.mean()}")

    let by_region: Table[{region: Text, revenue: Float}] = sales.sql("SELECT region, SUM(units * price) AS revenue FROM t GROUP BY region")
    for row in by_region:
        print(f"{row.region}: {row.revenue}")
```

* Every function and method of the standard library, with its type, is in [the reference](../reference/std.md).
* `print`, `read_csv`, `read_parquet` and `read_ipc` need no `std.` prefix. `print` ends the line;
  `std.io.print` writes without one.
* Methods on tensors are the `std.tensor` functions: `x.matmul(w).relu().softmax()`,
  `sales.price.mean()`. A table has `sql`. They are the same calls underneath, so
  `std.tensor.relu(x)` still works.
* Operators on tensors: `a + b`, `a - b`, `a * b`, `a / b` (same shape) and `a * k`, `k * a` to
  scale by a number.
* `Table[{region: Text, revenue: Float}]` writes the row type in place; no throwaway type for
  the result of a query. The same columns mean the same type everywhere.
* `fn main():` may return nothing.

See `examples/dataframe/`. The pandas version of the same program is about as long, but the
Telvax one checks the column names and types before it runs.

## Errors, tuples and iterators

```telvax
fn split_pair(line: Text) -> Result[(Text, Text), Text]:
    let parts = line.split("=")
    if parts.len() != 2:
        return Err(f"bad line: {line}")
    Ok((parts[0].trim(), parts[1].trim()))

fn port_of(line: Text) -> Result[Int, Text]:
    let key, value = split_pair(line)?  # `?` returns the Err early
    match value.to_int():
        case Some(n):
            Ok(n)
        case None:
            Err(f"not a number: {value}")

fn main():
    let xs = [3, 1, 4, 1, 5]
    let big = xs.filter(x => x > 2).map(x => x * 10)  # [30, 40, 50]
    for i, x in xs.enumerate():
        print(f"{i}: {x}")
    let stack = [1, 2, 3]
    while stack.pop() is Some(top):
        print(f"{top}")
    if xs.first() is Some(first):
        print(f"first = {first}")
```

* `Result[T, E]` has `is_ok`, `is_err`, `unwrap` and `expect("message")` (both stop the program
  with the error), `unwrap_or(v)`, `unwrap_or_else(e => v)`, `map`, `map_err`, `and_then`, and `ok()` /
  `err()`, which give an `Option`. `and_then` must return a `Result` with the same error type; use
  `map_err` to change it first. A method that does not exist is a compile error.
* `expr?` on a `Result` or `Option` returns the error (or `None`) from the current function; the
  function's return type must be a `Result` or `T?` too, and the error types must match.
* `if x is Pattern:` and `while x is Pattern:` match one pattern and bind its names; they work
  with `break` and `continue`, and `else:` runs when the pattern does not match.
* Tuples: the type `(A, B)`, values `(1, "a")`, fields `.0` and `.1`, and destructuring in
  `let a, b = pair`, `for` loops and `((a, b)) => ...` (one tuple argument taken apart).
* `List` methods `map`, `filter`, `any`, `all`, `find`, `enumerate`, `zip`, `sum`, `min`, `max`,
  `fold`, `for_each`, `skip` and `join`, and `Map.entries`. `min`, `max` and `find` return an
  `Option`. These run in the interpreter; the native backend does not compile them yet.

## Types, methods, traits and generics

```telvax
trait Shape:
    fn area(self) -> Float: ...

    fn describe(self) -> Text:
        f"area {self.area()}"

type Circle(Shape):
    r: Float

    fn area(self) -> Float:
        self.r * self.r * 3.14159

type Stack[T]:
    items: List[T]

    fn push(self, x: T):
        self.items.push(x)
```

A `type` lists its fields, then its methods. `type Circle(Shape)` says the type implements the
trait; a method without a body (`: ...`) is required, one with a body is a default. `extend Circle:`
adds methods to a type declared elsewhere. Call a function that has no `self` as `Circle.new(1.0)`.
`[T]` after a name declares a type parameter; `T: Shape` limits it to types that implement `Shape`.
A method can have type parameters of its own, besides those of its type:
`fn map[U](self, f: (T) -> U) -> Stack[U]`. They are inferred from the call (`s.map(x => x * 2)`), and
each different choice makes its own instance. A method of a trait cannot have them.
A generic function is called with its types inferred from the arguments. When they cannot be (a
function that takes nothing), or to be explicit, write them before the parentheses:
`empty[Text]()`, `pair[Int, Text](1, "a")`. The types are names such as `Int` or `List[Text]`.
Three limits need no trait of your own: `T: Num` (`Int`, `Float` and the sized numbers), `T: Ord`
(numbers and `Text`, the values `<` and `>` compare) and `T: Eq` (everything but functions). More than
one: `T: (Num, Ord)`. A type that does not meet a limit is an error at the call that uses it, and a
trait of yours named `Num`, `Ord` or `Eq` takes precedence.

See [use cases](use-cases.md) for complete programs.
