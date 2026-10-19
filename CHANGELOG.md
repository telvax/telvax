# Changelog

## Unreleased

* **`std.log`:** `std.log.debug|info|warn|error(message, fields)` writes a line to the standard error in the format and at the
  level of `telvax serve` (`--log-format`, `--log-level`), so the lines of the program and the lines of the server read
  alike. The fields are a `Map[Text, Text]`.
* **A level for the log of a request:** the line has a `level` (`info` under 400, `warn` for 4xx, `error` for 5xx) and
  `telvax serve --log-level debug|info|warn|error`, `TELVAX_LOG_LEVEL` or `log_level` in the `serve:` block leaves out the
  lines below it (`info` by default). In `text` the line now starts with the level.
* **A `serve:` block for the options of the server:** the entry file can say how it is served, with fixed values
  (`serve:` then `metrics: True`, `cors: "*"`, `rate_limit: "100/min"`, ...), so the Makefile, the Dockerfile and the
  deploy workflow do not have to repeat the flags. A `TELVAX_*` variable or a flag still wins, `telvax check` reports a
  wrong name, type or value, and `telvax build --serve` keeps the block in the binary.
* **`std.cache`:** `std.cache.new(max, ttl_seconds)` makes a cache with a size limit and an expiry, shared by all requests:
  `get`, `set`, `remove`, `clear` and `len` on `Text` keys. When it is full the least recently used entry goes (per shard), and
  an entry older than the ttl is not returned. The values are not type checked yet.
* **Counters with labels, and the time of a request:** `std.metrics.count("jobs_total", {"status": "done"})` is listed in
  `/metrics` as `jobs_total{status="done"}` with its own `# TYPE`, so the names and labels of a dashboard can be kept
  (a counter without labels is still `telvax_counter_total{name="..."}`). A counter name has at most 1,000 label sets.
  `telvax serve --process-time-header` (or `TELVAX_PROCESS_TIME_HEADER=1`) adds `x-process-time-ms` to every answer.
* **`telvax fmt` and a negative number on its own line:** `-1` or `-a` as the last line of a block stays as written; it
  became `- 1`.
* **`len()` next to an `Int`:** `xs.len() > limit` and `xs.len() - 1 > i` work when the other side is a signed number
  (`Int`, or the `i32` of a literal); the two are read as `Int`. They failed with `mismatched operand types: u64 and
  i32`. Two `u64` values, or a `u64` and an unsigned type, are unchanged.
* **A failed background task can be told about:** `std.task.spawn(work(id), failed)` calls `failed(message)` when the task
  fails, so a job can be marked as failed. Without the second argument the message still goes to the standard error.
* **Faster services:** a request that scores 100 texts takes about a quarter of the time, and several requests at once
  scale with the cores. The compiled patterns of `std.text` are kept per thread and reused (copying one made a new
  search cache each time), `std.metrics.count` no longer takes one lock for all threads, the globals are read without a
  lock once the program has started, locals and function names are looked up with a cheaper hash, and a method call, an
  index or a field read on a name no longer copies the shared list, map or struct it looks at.
* **Stopping a server:** on `SIGTERM` or `Ctrl-C`, `telvax serve` (and a bundled binary) stops taking connections, finishes
  the requests in progress and waits for the tasks of `std.task.spawn`, for at most `--shutdown-wait N` seconds (30, or
  `TELVAX_SHUTDOWN_WAIT`). `/healthz` answers `503` meanwhile. A second signal leaves at once; leaving with work
  unfinished is exit status 1.
* **Errors as FastAPI writes them:** `telvax serve --error-format detail` (or `TELVAX_ERROR_FORMAT=detail`) answers the
  errors of the server as `{"detail": ...}`, a body or query that does not fit its type as `422` with the list of
  problems, and an unknown route as `{"detail": "Not Found"}`, so clients of a service that moved from FastAPI keep
  working. The OpenAPI document says `422` too. The default (`telvax`) is unchanged.
* **The Dockerfile from `build --serve`:** it copies the whole directory of the program, so a program of several modules
  and embedded files builds, and it starts from `ghcr.io/telvax/telvax` (or `--build-arg TELVAX_IMAGE=...`) instead of
  compiling Telvax from source, which took about 25 minutes.
* **Health check of a bundled server:** `service --healthcheck [/path]` asks the server on this machine (`/healthz` by
  default) and exits 0 on a 2xx answer and 1 otherwise, so a distroless container can check itself. The Dockerfile that
  `build --serve --dockerfile` writes has a `HEALTHCHECK` that uses it.
* **Constants between modules:** `from util import LIMIT, names` imports a top-level `let`. A list or a map is the same
  one in every module, so a change made in one is seen in the others. A name that starts with `_` stays private.
* **Constants in capitals:** `LIMITS.get(key)`, `NAMES.push(x)` and `CONFIG.size` work on a top-level `let` whose name
  starts with a capital. They were read as a type and failed with `cannot find LIMITS.get in this scope`.
* **Random values:** `std.random.uuid()` is a random version 4 identifier as 32 hexadecimal digits, for ids that must
  not be guessed, and `std.random.int(low, high)` a random whole number, both ends included. They come from the
  operating system and need no capability.
* **Optional fields in a request:** a field with a default written as a literal (`language: Text = "en"`) or typed
  `T?` may be left out of the JSON body or the query string; it takes the default, or `None`. The OpenAPI document no
  longer lists such a field as required and shows the default.
* **Work after the answer:** `std.task.spawn(task)` starts a task and returns at once, so an `@endpoint` can answer
  `202` and finish the work while the client polls. The task's error goes to the standard error; at most 256 run at a
  time. `telvax run` and `telvax test` wait for the task, as `std.task.run` does.
* **Patterns in text:** `std.text.matches(text, pattern)`, `find` (the first match, `Text?`), `find_all` and
  `replace_all(text, pattern, with)` use regular expressions (the Rust `regex` syntax: no look-around and no
  backreferences, which keeps a match from taking exponential time). An invalid pattern is an error that says why.
  Compiled patterns are kept, so calling one in every request is cheap.
* **SQL on PostgreSQL:** `std.db.postgres.PostgresClient.connect(url)` has `execute(sql, params)` (the rows changed) and
  `query(sql, params)`, whose rows come back as a list of your own `type`: `let rows: List[Job] = db.query("SELECT id,
  status FROM jobs WHERE status = $1", ["queued"])?`. Parameters are bound, not pasted into the SQL. It needs `--live`
  and `net`; `json`/`jsonb` columns are `Text`, and other types such as `timestamptz` are cast in the SQL
  (`created_at::text`). `?` now passes the type a context expects on to the call it is applied to.
* **CORS, a rate limit and security headers:** `telvax serve --cors "*"` (or a list of origins), `--rate-limit 100/min`
  (`429` with `retry-after` over the limit; `--trust-proxy` reads the client from `x-forwarded-for`) and
  `--security-headers`. A binary from `build --serve` takes `TELVAX_CORS`, `TELVAX_RATE_LIMIT`,
  `TELVAX_SECURITY_HEADERS` and `TELVAX_TRUST_PROXY`.
* **Metrics, logs and a request id:** `telvax serve --metrics` serves `GET /metrics` (Prometheus text: requests per
  method, route and status; a latency histogram; the counters made with the new `std.metrics.count(name, by)`).
  `--log-format json|text` writes one line per request to the standard error. With either, every answer has an
  `x-request-id` header. A binary from `build --serve` takes the same flags, or `TELVAX_METRICS` and `TELVAX_LOG_FORMAT`.
* **OpenAPI and a docs page:** every server answers `GET /openapi.json` (OpenAPI 3.0.3, built from the types of the
  endpoints: path and query parameters, body, answer, error) and `GET /docs` (a page that lists the routes). An
  endpoint of the program at one of those paths takes their place.
* **Files inside the binary:** `std.fs.embed("lexicons/vader.txt")` is a `Text` read when the program is built, so
  `telvax build --serve` makes one binary with its data files in it. The path is a literal, relative to the main file;
  a missing file or a computed path is an error when the program is checked.
* **Server-sent events:** `Response.events(producer)` answers with `text/event-stream`. The producer is a function
  that receives a `Sink`; `sink.send(data)`, `sink.event(name, data)` and `sink.comment(text)` write to the client
  as they are called. An error in the producer ends the stream with an `error` event; a client that goes away ends it
  at the next `send`.
* **State shared between requests (documented):** a top-level `let` that holds a `List` or a `Map` is shared by every
  request, and each `push`, `insert` or `remove` is atomic. This was already so; it is now in the tour and tested with
  concurrent calls. A top-level `var` is still not allowed.
* **Text normalisation:** `std.text.normalize(text, "NFKD")` writes the text in a Unicode form (`NFC`, `NFD`, `NFKC` or
  `NFKD`; another name is an error), `std.text.strip_accents(text)` removes accents (`"ação"` is `"acao"`) and
  `std.text.remove_chars(text, chars)` removes every character that is in `chars`.

* **Status and headers of the answer:** an `@endpoint` function can return `Ok(Response.new(body).status(202).header("location", url))`
  (`import std.http.Response`). `.status(code)` takes 100 to 599, any other value is an HTTP 500, and `.header` replaces a
  header of the same name, `content-type` included. A function that returns the value itself still answers `200`.

* **Request headers:** an `@endpoint` function can take a parameter of type `Request` (`import std.http.Request`).
  `req.header("x-admin-token")` is the first value of that header as `Text?` (the name is not case sensitive), and
  `req.method()` and `req.path()` are `Text`. The `Request` is not the payload, so it can come with a path
  parameter and a payload, and a `GET` or `DELETE` that has only a `Request` needs nothing else.

* **Route parameters:** `@endpoint(GET, "/v1/results/{job_id}")` reads `{job_id}` from the URL into the parameter
  of that name (`Text`, a number type or `Bool`). A value that does not convert is an HTTP 400. The parameter
  that is not in the path stays the payload, so `PUT "/v1/jobs/{id}"` still takes a JSON body, and a `POST` with
  only path parameters needs none. The path is checked: a name without a parameter, a parameter of a `type`,
  a name twice or two routes that differ only in a parameter name are compile errors.

* **VS Code extension icon:** the extension now has the Telvax logo as its icon.

* **Docker image:** every release publishes `ghcr.io/telvax/telvax` for `linux/amd64` and `linux/arm64`, tagged with
  the version and `latest`. It holds `telvax` and `tvpm`; `docker run --rm -v "$PWD:/work" -w /work ghcr.io/telvax/telvax run app.tvx main`.
  The Dockerfile that `build --serve` writes is unchanged.

* **`telvax schema`:** `telvax schema sales.csv` prints the `type` for the columns of a CSV, Parquet or Arrow file,
  to paste above `let t: Table[Sales] = read_csv("sales.csv")`. For a CSV the values give the types and a column
  with an empty value is written `T?`; `--name Row` picks the name. The compiler still does not read data files:
  `let t = read_csv(...)` without an annotation is an error.

* **Table `group_by`:** `t.group_by(["region"], ["units:sum", "price:mean"])` makes one row per distinct key, in
  order of first appearance, with a column `<column>_<function>` for each aggregation (`sum`, `mean`, `min`, `max`,
  `count`). The names are checked at compile time and the result row type is inferred, so it needs no annotation.
  It works in `telvax-lite` too.

* **Table `select`:** `t.select(["id", "name"])` keeps only those columns. The names are checked at compile time
  and the row type of the result is inferred from them, so it needs no annotation. It works in `telvax-lite` too.

* **Documentation versions:** the site keeps one folder per version, `dev` (the `main` branch) and one for each
  series of release (`0.30`), with a version picker in the header, like the manual of other languages. The root
  of the site goes to the newest series, and a link without a version is sent there. Every release publishes the
  documentation of its series.
* **Command line reference:** `docs/reference/cli.md` (and the site) describes every `telvax` command and option.
  A test checks that its synopsis is the output of `telvax --help` and that each command has a section.
* **Site, like the Python manual:** "What's new" (this changelog), an About section (reporting problems and
  contributing, security, code of conduct, the licenses), and the date each page last changed.
* **Standard library reference:** `docs/reference/std.md` (and the site) lists every `std` function, type and
  built-in method with its signature and what it does. A test fails when an item has no description, when a
  signature has a different number of parameters than the type checker takes, or when the page is out of date.
* **Documentation site:** the guide and the library reference are built into a website with Starlight and published to
  GitHub Pages (`site/`). `telvax` code blocks are highlighted with the VS Code grammar. Pull requests that touch
  the documentation build the site and check its links.
* **Arrow Flight client:** `std.table.read_flight(url, ticket)` and `std.table.write_flight(table, url, path)`
  fetch and send tables over gRPC (`grpc://` or `grpc+tls://`). They need the `net` permission and the `flight`
  feature (`cargo install telvax --features flight`), which is off by default because it adds gRPC; without it they
  report how to enable it. Client only.
* **Table chunks:** `t.split(n)` cuts a table into tables of at most `n` rows that share its buffers.
  `std.table.read_csv_batches`, `read_parquet_batches` and `read_ipc_batches` (`path`, `n`) read a file into a
  `List[Table[Row]]` without building one joined copy. The chunks are all in memory; there is no lazy reader yet.
* **Arrow IPC / Feather:** `std.table.read_ipc(path)` and `std.table.write_ipc(table, path)`. Reading maps the
  file into memory, so the columns are not copied (one batch, no compression). They also work in `telvax-lite`.
* **`Date` and `Timestamp` columns:** table row types can use `Date` (Arrow `Date32`) and `Timestamp` (Arrow
  `Timestamp(µs, UTC)`). Values are ISO-8601 `Text` in the program; Parquet, SQL and PyArrow see real temporal
  types. Date64 and other timestamp units are read too. No date arithmetic yet.
* **Optional table columns:** a field typed `T?` in a table's row type is a nullable Arrow column. Empty CSV
  cells, Parquet nulls and Arrow nulls from Python become `None`; `from_rows` writes `None` as null. A required
  column that finds an empty value now says to declare it `T?`.
* **Table methods:** `t.head(n)`, `t.tail(n)`, `t.sort_by("col")`, `t.sort_by_desc("col")`, `t.filter(r => ...)` and
  `a.concat(b)` return a table of the same row type. They use Arrow directly, so they work in `telvax-lite` too.
* **`telvax doc`:** writes a Markdown reference from the code: functions with their signatures, `type` fields
  and defaults, `enum` variants, `trait` and `extend` methods, and the `#` comment lines directly above each
  item. `telvax doc src/ -o REFERENCE.md`; `--public` skips names that start with `_`. It is the library
  `telvax_doc`.
* **Notebooks:** `pip install "telvax[kernel]"` adds a Jupyter kernel for Telvax (over `ipykernel`): each cell
  is Telvax, definitions and variables last from one cell to the next, the value of the last expression is
  shown (a table or a tensor as HTML), what the program prints is the cell's output, an error shows the line,
  Tab completes and Shift+Tab shows the type. `%load_ext telvax` adds `%%telvax` to a Python notebook. In Python,
  `telvax.Session` is the engine of both (`eval`, `type_of`, `complete`, `hover`, `variables`, `reset`).
  `python -m telvax.kernel install` registers the kernel when pip's data files are not used.
* **`telvax repl` completion:** Tab in the middle of a line, or on an empty line before the cursor, now completes
  at the cursor instead of four columns off.
* **HTML for tables and tensors:** `telvax_runtime::rich::html` shows a table as an HTML table (the first 50
  rows) and a tensor as a grid, for notebooks and pages.
* **A console in the playground:** under the editor, the same session as `telvax repl` runs in the browser.
  Definitions and variables last from one input to the next, a block ends with an empty line, Tab completes names,
  fields and methods, the arrow keys go through earlier lines, and `:type`, `:vars`, `:defs`, `:load`, `:reset`,
  `:clear` and `:help` are commands. **Load the program** enters the functions, types and variables of the editor.
  It has no files, environment or network, and a runaway input stops at the step limit.
* **`telvax repl`:** an interactive session. Type a definition (`fn`, `type`, `enum`, `trait`, `extend`,
  `import`), a variable (`let`, `var`), a statement or an expression and see the value. Definitions and
  variables last from one input to the next; a name can be defined again; a type error shows the line and
  leaves the session as it was. Blocks span lines and end with an empty line; Tab completes names, fields and
  methods; `:type`, `:vars`, `:defs`, `:load`, `:reset`, `:help` and `:quit`; history in `~/.telvax_history`;
  `--allow` as for `telvax run`. The session is the library `telvax_repl`, which the playground will use.
* **Debugging in VS Code:** the extension contributes a `telvax` debugger that starts `telvax dap`. F5 on a
  `.tvx` file debugs it without a `launch.json`; the commands **Telvax: Debug Current File** and **Telvax: Run
  Current File Without Debugging** are in the run menu of Telvax files; a launch configuration takes
  `program`, `function`, `args`, `stopOnEntry`, `noDebug` and `allow`. The test that runs inside a real VS
  Code now sets a breakpoint and reads the stack and variables.
* **`telvax dap`, a debug adapter:** speaks the Debug Adapter Protocol on standard input and output, so an
  editor can run a `.tvx` file in the interpreter and stop it at breakpoints (a breakpoint on a line with no
  statement moves to the next one), step over, into and out, pause, look at the calls in progress, open
  variables (lists, maps, types, variants and tuples expand), hover over a name or a field such as `p.x`, and
  see what the program prints in the debug console. The launch configuration takes `program`, `function`,
  `args`, `stopOnEntry`, `noDebug` and `allow`. The VS Code extension does not use it yet.
* **`telvax profile`:** runs a function and reports the time and calls per function (in the function
  itself and with its calls), the hits and time per statement, the time, rows and bytes of each `std`
  operation (`std.tensor.matmul`, `std.table.sql`, ...), and the memory (peak, bytes and number of
  allocations, and the bytes each function allocated). `--top N` limits the statements, `--json`,
  `--collapsed` (flame graph stacks) and `--speedscope` write files. It profiles the interpreter, not
  `telvax build` programs. In the library, `telvax_runtime::profile` and `telvax_runtime::alloc_count`.
* **Debugger hook in the interpreter:** `telvax_runtime::debug` lets a debugger be called before each
  statement, with the calls in progress, the variables of each call (name, type, value) and a way to
  look into lists, maps, types and variants. It is the base for a debug adapter; there is no `telvax debug`
  command or editor support yet. Without a debugger the cost is one flag check per statement.
* **References and rename across files:** find references and rename now follow `import` and
  `from .. import` into the other files of the project, for functions, types, enums, traits, variants,
  fields and methods. One rename edits every file that uses the name; a name imported `as` another keeps
  that alias. Files open in the editor count with their unsaved text. A rename is refused when a file
  has a syntax error, when the name belongs to a dependency, or when the project would have more errors
  after it (a collision). Go to definition on an imported type or variant opens the file that declares it.
* **Go to definition, references and rename for more than variables:** they now work on `type`s, `enum`s,
  `trait`s and aliases (in annotations, `impl`, constructions and patterns), enum variants
  (`Shape.Circle`, `case Shape.Circle(r)`), fields (`p.x`, `Point(x: 1)`, `Point(x: a)` patterns), methods
  (`p.norm()`, `Point.new()`) and top-level `let` globals. Renaming a method also renames it in the trait
  and in the types that implement it. While the file has a syntax error, definition and references keep
  working from the last good analysis, with positions moved past the edit; rename waits until the file parses.
  References no longer take the first match for the declaration, and the check that a rename does not
  collide with another name analyses the file with its imports.
* **Completion in context:** the language server now offers types after `:` and `->`, inside `List[..]`,
  `Map[..]` and `Table[..]`, after `type Id =` and for the fields of a `type` (built-in types, your
  `type`s, `enum`s, `trait`s, aliases and the type parameters in scope). After a dot it knows the methods
  of `Text`, `List`, `Map`, `Option`, `Result`, `Tensor` and `Table`, the methods of your types and the
  defaults of the traits they implement, and the methods of a trait-typed value; it follows calls in a
  chain (`s.trim().`, `xs.first().unwrap().`). `Shape.` lists the variants of an enum and `Point.` the
  functions without `self`. In a closure that is still being typed, the parameter has the type of the
  elements (`xs.map(p => p.`, `std.table.filter(t, r => r.`). Inside a string, the columns of a table are
  offered for `t["`, `t.sql("..` and `std.table.f(t, "..`; other strings and comments offer nothing.
  Items fit the type the code wants first (the parameter of the call, a `return`, the variable being
  assigned), then names in scope, then declarations, then keywords, and carry a detail and a short
  description. The method lists come from one table that the type checker also uses for its suggestions.
* **VS Code README:** the install commands pointed at `ntsation/telvax`, which does not exist; they use `telvax/telvax`.
* **VS Code snippets and a fuller README:** the extension adds snippets for `fn`, `main`, `type`, `enum`,
  `match`, `for`, `test`, `endpoint` and `trait`. Its README lists references, rename and formatting and
  explains how the release publishes to the Marketplace and Open VSX.
* **The documentation is tested:** every ```` ```telvax ```` block in the README, the tour and the use cases is
  checked, checked for formatting and, when it has `fn main`, run; every file under `examples/` is checked
  and checked for formatting. A block that is only a piece of a program is marked ```` ```telvax ignore ````.
  The tour and the use cases now format cleanly and read `examples/dataframe/sales.csv` from the repository root.
* **Lint rules you can configure:** the checks of `telvax lint` now have names (`unused-variable`,
  `unused-function`, `unreachable-case`) and a level, `allow`, `warn` (the default) or `deny`. Set them in
  a `[lint]` table in `telvax.toml` or with `--allow`, `--warn` and `--deny <rule>`; the flags win.
  `telvax lint --list-rules` lists them. A `deny`ed rule is an error and makes the command fail.
* **`telvax fmt` takes directories and shows a diff:** a directory means every `.tvx` file under it (hidden
  folders, `target`, `node_modules` and `.tvpm` are skipped), and no path means this directory.
  `--diff` prints the lines that would change as `@@ line N @@` blocks and writes nothing; like `--check`,
  it exits 1 when something would change. An unknown option is an error.
* **Coverage in `telvax test`:** `--coverage` prints, for each file of the project, the lines with code that
  the tests ran and the ones they did not; `--fail-under N` fails the run when fewer than N% ran. It
  counts lines, not branches, and leaves out the `test_`, `fixture_`, `teardown_` and `cases_` functions.
* **Mocks in `telvax test`:** `std.test.mock(price, fake_price)` or `std.test.mock("std.fs.read", fake_read)`
  replaces a function until the test ends; `std.test.calls(target)` counts the calls. A replacement for
  one of your functions must have its type.
* **Cases in `telvax test`:** `fn cases_double() -> List[(Int, Int)]` runs `test_double(x, want)` once for
  each item and lists each run as `test_double[(2, 4)]`. A tuple fills the first parameters; the rest are
  fixtures. An empty list or items of the wrong type fail the test with a message.
* **Fixtures in `telvax test`:** a test can take parameters; `fn test_x(db: Db)` gets `fixture_db()`, made
  again for each test, and `teardown_db(db)` runs after it even when the test fails. Fixtures can take
  other fixtures. `telvax lint` no longer calls `test_`, `fixture_` and `teardown_` functions unused.
* **`telvax test` takes a directory:** it runs every `test_*.tvx` and `*_test.tvx` file under it (no
  path means this directory), prints each file, and ends with one total. Files with no tests are skipped.
* **`tvpm new` writes valid Telvax, and `tvpm publish` checks first:** the files `new` and `init` create
  used `def` and `str` and failed `telvax check`; they now use `fn` and `Text`. `publish` type-checks the
  package (with its dependencies) and refuses to publish one that has errors, showing them. `tvpm check`,
  `run`, `serve` and `build` in a package with no dependencies no longer ask for `tvpm lock`.
* **`tvpm update [name...]`:** moves the locked versions to the newest that the requirements allow and
  says what changed (`updated mathx 1.0.0 -> 1.2.0`, `added`, `removed`, or `everything is up to date`),
  then downloads what is missing. With names, only those packages move and the others stay at their
  locked version unless a requirement forces them. Before, the only way to pick up a new version was to
  delete `telvax.lock`.
* **`telvax run` reads its arguments by type:** the words after the function are read as the types it
  declares, so `Text` keeps `123` as text and a `List[Int]` takes `[1,2,3]`. A wrong word says which
  argument and which type (`argument `b` of `add` is Int, and `x` is not a value of that type`), a
  wrong count lists the parameters, and an unknown function lists the ones the file has. The function
  name is optional: `telvax run app.tvx` runs `main`. Only `telvax check` prints "ok" now; `run`,
  `test` and `serve` no longer do.
* **`telvax check` takes many files, a package and `--json`:** `telvax check a.tvx b.tvx` checks each
  and says how many failed; `telvax check` with no file checks the entry of the package around the
  current directory (`telvax.toml`); `telvax check --json` prints one JSON array on stdout with, for
  each diagnostic, `file`, `line`, `column`, `end_line`, `end_column` (1-based), `severity`, `message`
  and `notes`. The exit code is 1 when any file has an error, in both modes.
* **`telvax doctor`:** prints the version, the syntax editions it reads, which features this binary was
  built with (tables, live services, GPU, native backend) and the environment variables that change
  its behavior; keys are shown as set or not set, never their value. An unknown command now says
  `unknown command `chek`; did you mean `check`?` instead of printing the whole usage.
* **The syntax is stable from here on:** programs in `tests/editions/2026/` are frozen and must keep
  passing `check`, `fmt --check` and their tests; a test fails if one is edited. `telvax.toml` takes
  `edition = "2026"` (`tvpm new` writes it; an unknown edition is an error). A change that would break
  an old program becomes a new edition. The rules are in `docs/guide/stability.md`.
* **`--allow` limits what a program may do:** `telvax run`, `test` and `serve` take
  `--allow fs.read,fs.write,fs,env,net,all,none`. A call that needs a capability that was not listed
  stops with a message that names the word to add. Without the flag everything is allowed, as before.
  The check happens when the program runs; it is not part of the types.
* **Built-in macros:** `assert!(cond)` quotes the condition when it fails, `assert!(cond, "message")`,
  `dbg!(value)` prints `value = result` to stderr and gives the value back, and `todo!()`,
  `unreachable!()` and `panic!("message")` stop the program (and fit any type). An unknown `name!(...)`
  names the macros that exist. Users cannot define macros. The formatter and the VS Code grammar know
  the `name!(` form.
* **The `|>` operator follows one rule:** `x |> stage` is the call `stage` with `x` as its first argument.
  Names and defaults now count that argument (`1 |> scale(k: 3)` and `1 |> scale` used to fail with
  "missing the argument" and "expected 2 argument(s)"), and a suffix after the stage applies to the
  result of the call (`s |> parse()?`, `n |> make().field`, `n |> items()[0]`; the first two were errors).
  `|>` binds looser than every operator, and the error for `x |> f == 1` says to write `(x |> f()) == 1`.
* **Inferred return type, more places:** a method without `->` (in a `type`, an `extend`, or an
  associated function) returns the type of its last expression; a method that implements a trait returns
  what the trait declares for it; and a global can call a function that has no `->`. `main`, tests,
  endpoints and generic functions still need it.
* **A generic that grows forever is an error:** `fn f[T](x: T): f([x])` is instantiated for `T`, then
  `List[T]`, then `List[List[T]]`, and so on; the checker never finished and used all the memory. A type
  of more than 200 parts in an instance is now an error that says the types keep growing.
* **Deeply nested code no longer hangs or crashes the compiler:** parentheses inside parentheses were
  read twice per level, so twenty levels took seconds and forty never finished; they now take linear
  time. Blocks nested past 64 levels and brackets past 200 are an error that says so, and the parser and
  the checker run on a stack of their own, so a long sum or calls inside calls are checked instead of
  overflowing the stack and ending the process.
* **Methods with type parameters of their own:** `fn map[U](self, f: (T) -> U) -> Stack[U]` in a generic
  or a plain `type`, or in an `extend`. The parameters are inferred from the call and may have limits
  (`[N: Num]`). Before, they were an error ("not supported yet").
* **Type arguments in a call:** `empty[Text]()`, `pair[Int, Text](1, "a")`. It makes a generic function
  that cannot infer its types from the arguments usable, and the "cannot infer" error now says so.
* **Built-in limits for type parameters:** `T: Num`, `T: Ord` and `T: Eq` (and `T: (Num, Ord)`) without
  declaring a trait. A call with a type that does not meet a limit is reported once, at the call, instead
  of again inside the generic function's body.
* **Function types:** `(Int, Int) -> Int` is a type, so a function can be a parameter, a field, a list
  element or a return value (`fn adder(n: Int) -> (Int) -> Int: x => x + n`). `() -> ()` stands for no
  value. A field that holds a function can be called (`handler.run(5)`), and a closure parameter can
  carry its type: `(x: Int) => x + 1`.
* **Imports can be renamed:** `from util import clamp as limit`, `import util.clamp as limit`. Importing
  a module as a whole (`import util`) is an error that says to write `from util import name`, and the
  messages for the modules of your own project no longer say `crate`.
* **Syntax errors say what to write:** a `{ }` body, a parameter without a type, `: Type` for a return
  type, and `use`, `const` and `function` now get the same hint as `def` and `pub`. The extra
  "unexpected `def`" that came before the hint is gone.
* **Methods on `Result` and `Option`:** `Result` has `is_ok`, `is_err`, `unwrap`, `expect`,
  `unwrap_or`, `unwrap_or_else`, `map`, `map_err`, `and_then`, `ok` and `err`; `Option` gains `map`,
  `and_then`, `filter`, `ok_or`, `unwrap_or_else` and `expect`. They are checked: a method that does not
  exist on a `Result` is an error (it used to pass without a type).
* **Inferred return type:** a top-level function without `->` returns the type of its last expression
  (`fn double(x: Int): x * 2` returns an `Int`). `main`, tests, endpoints, generic functions and
  methods still need the `->`.
* **Generic calls and literals:** `let a = id(3)` no longer fixes `a` to `i32` at the call; a later use
  such as `let n: Int = a` decides the type, and the default applies only if nothing does.

## 0.1.0

**Breaking: a new syntax.** Telvax is now easy to read for anyone who knows Python (blocks by
indentation, no braces or semicolons) and keeps its own words and ideas. The Rust-style syntax is gone.
The design is in `docs/design/sintaxe-pythonica.md`; the checker, the interpreter and the native
backend are unchanged except where noted.

```telvax
type Query:
    text: Text
    top_k: Int = 5

fn embed(text: Text) -> Tensor[f32, 768]:
    text
        |> std.text.clean_whitespace()
        |> std.text.truncate(512)
        |> embedder.embed()

@endpoint(POST, "/v1/rag/query")
fn handle(q: Query) -> Result[Reply, Status]:
    let docs = search(embed(q.text), limit: q.top_k)
    ...
```

### Language
* **Blocks:** a line that ends with `:` opens an indented block; `#` starts a comment.
* **Functions and variables:** `fn name(a: Int) -> Int:`; the last expression is the value, `return`
  leaves early. `let x = 1` cannot change, `var x = 1` can; a plain `x = 2` never declares.
* **Types:** `Int`, `Float`, `Text`, `Bool`, `List[T]`, `Map[K, V]`, `T?` for a value that may be `None`,
  `Result[T, E]`, `Tensor[f32, 2, 3]` (the dimensions follow the element type) and `Table[Row]`; sized
  numbers keep `i8`..`i64`, `u8`..`u64`, `f32`, `f64`. Error messages use the same names.
* **Your own types:** `type Point:` with fields and methods in one block, `type Circle(Shape)` to implement
  a trait, `extend Point:`, `enum Shape:` with one variant per line, `trait Shape:` and `type Stack[T]:`.
* **Named arguments and defaults:** `Query(text: "x")`, `search(q, limit: 3)`; a field or a parameter can
  have a default (`top_k: Int = 5`). Names are checked against the function.
* **Functions as values:** `x => x + 1`, `(a, b) => a + b`; an `fn` inside a function is a closure with
  several statements.
* **Words and operators:** `and`, `or`, `not`, `in`, `elif`, `True` / `False` / `None`, `a if c else b`,
  `for i in range(n)`, `for i, x in items`, `pass`. `x is Some(v)` replaces `if let` and `while let`.
* **Modules:** `.` replaces `::`; `from util import clamp`, `import util.clamp`. A name that starts with `_`
  is private; there is no `pub`.
* `@endpoint(POST, "/path")`; `print` ends the line (`std.io.print` does not); values print as
  `True`, `None`, `Point(x=1, y=2)`.
* Kept: `|>` (a line may start with it), `?`, `Some`, `Ok`, `Err`.

### Tools
* `telvax fmt` is rebuilt for indentation and checks that the tokens are unchanged.
* `telvax lint` no longer reports "needless `mut`"; an unused private function is one whose name starts
  with `_`.
* The language server, the VS Code grammar and the playground follow the new syntax.
* Syntax errors say what to write: `def` -> `fn`, a missing `:`, `;`, `..`, `x = 1` without `let`.
* `telvax-migrate` (`tools/migrate`) converts files written in the old Rust-style syntax and keeps their
  comments: `cargo run -p telvax_migrate -- file.tvx`. It will be removed in a later release.

### Moving a program
Run `telvax-migrate` on each file. What it cannot carry over: an `if` or `match` used as a value (write a
conditional expression, or assign inside the branches), blocks used as expressions, closed ranges
(`0..=n` is `range(0, n + 1)`), and typed closure parameters.

## 0.0.5

The project is now called **Telvax** (it was Novar). Everything was renamed: the binaries are `telvax`
and `tvpm`, source files end in `.tvx`, environment variables start with `TELVAX_`, the manifest is
`telvax.toml`, and the repository is `telvax/telvax`. Releases before this one keep the old names.

### Language
* **Errors and control flow:** `expr?` passes the error of a `Result` (or the `None` of an `Option`) up
  to the caller, and `if let` / `while let` match a single pattern.
* **Tuples:** the type `(A, B)`, values, `.0` / `.1`, patterns, and destructuring in `let`, `for` and
  closure parameters.
* **Iterator methods on `Vec`:** `map`, `filter`, `any`, `all`, `find`, `enumerate`, `zip`, `sum`, `min`,
  `max`, `fold`, `for_each`, `skip`, `join`; `Map::entries`.
* **`impl` blocks, traits and generics:** methods on structs and enums, `trait` with
  `impl Trait for Type`, trait-typed values dispatched at run time, and generic functions, structs,
  enums and impls with trait bounds.
* The new syntax runs in the interpreter; the native backend does not compile it yet.

### Docs and examples
* A use-cases guide (`docs/guide/use-cases.md`) and four new runnable examples: word count, a
  shape-checked network, an HTTP service that needs no model, and error handling with `?`.

### Fixes and security
* `rustls` 0.23.46 (RUSTSEC-2026-0349).
* The table tests no longer share a scratch file, and the bundle test waits for the server to answer.

### Releases
* Every pull request merged into `main` is now released automatically with the next patch version,
  so no file needs editing. The VS Code extension is packaged and attached to each release, and
  published to the Marketplace and Open VSX when their tokens are configured.

## 0.0.4

### Language
* **Friendlier syntax** (all additive): `println`, `print`, `read_csv` and `read_parquet` work
  without `std::`; tensors have methods (`x.matmul(w).relu().softmax()`, `sales.price.mean()`) and
  operators (`a + b`, `a * 2.0`); tables have `.sql(...)`; a row type can be written in place
  (`Table<{ region: String, revenue: f64 }>`); `fn main()` may return nothing.
* `Option<T>` (`Some`, `None`, `match`, `unwrap`, `unwrap_or`, ...) and `Map<K, V>` (`std::map::new()`,
  `insert`, `get`, `remove`, `contains_key`, `keys`, `values`, ...), also across the Python and JSON
  boundaries. `Vec` gains `first`, `last`, `pop`, `get`, `sort`, `reverse`; `String` gains `find`,
  `to_int`, `to_float`, `chars`, `lines`.
* Closure parameters can be annotated (`|x: i64| x + 1`), and assigning to a captured variable is a
  compile error (closures capture by value).

### Tools
* The language server finds references and renames variables and functions (a rename that would
  collide with another name is refused). `telvax fmt` indents the line after a trailing `=`.
* Quantization: `--quantize int8-channel` and `--quantize int4-channel` (interpreter only).

### GPU
* Nested tensor operations (`softmax(matmul(relu(matmul(x, w1)), w2))`) keep their intermediate
  results on the GPU. On the M4 a 3-layer network takes 44 ms against 9.0 s in the CPU interpreter;
  the chaining itself saves little there because memory is shared.

### Verification
* The playground page is tested in a real Chromium (Playwright), the extension inside a real VS
  Code, and every published release is installed and run on Linux, macOS and Windows by the release
  workflow. The LLVM job is blocking.
* CI also runs a build of the ~10 MB `lite` binary.


## 0.0.3

### Size
* Two builds per system: the full `telvax` (about 51 MB, down from 58 with full LTO) and a new
  `telvax-lite` (about 10 MB) without SQL and Parquet (tables in memory and CSV still work). `TELVAX_LITE=1` makes the install scripts
  pick it. Build it yourself with `--no-default-features --features live`.

### Language and errors
* `String`: `contains`, `starts_with`, `ends_with`, `replace`, `split`. `Vec`: `push`, `contains`.
* A method that does not exist on a built-in type is a compile error (it used to fail only when
  run), and misspelled names, types, fields and methods get a "did you mean ...?" note.
* A missing `;` is reported right after the previous token instead of listing every token that
  could follow.

### Dependencies
* `wgpu` 30 (tested on a real GPU), `lsp-server` 0.10, `vscode-languageclient` 10 and TypeScript 7
  (the extension now needs VS Code 1.91), newer GitHub Actions and maturin.

### Fixes
* The Dockerfile written by `build --serve` did not create `/out`; it builds now (CI builds it).


## 0.0.2

### Language
* `break` and `continue` in `while` and `for` loops (interpreter and native backend).
* Enums with tuple and unit variants, and `match` over enums, `Result`, integers, strings and
  booleans, with exhaustiveness checking. Enums cross module boundaries and travel as JSON.
  They run in the interpreter; the native backend reports them as unsupported.

### Tools
* `telvax fmt [--check]`: a token-based formatter that keeps comments and your line breaks. The
  language server formats documents too.
* `telvax lint [--deny-warnings]`: unused variables, needless `mut`, unused private functions.
* `telvax --version` and `--help`.

### Platforms and releases
* Windows: CI builds and tests it, releases include a `.zip`, and `scripts/install.ps1` installs
  it. Windows builds have not been run on a real machine by the author.
* Release binaries for Linux arm64 and macOS Intel; pushing a higher `version` to `main` tags and
  publishes a release automatically.
* Dependabot covers Cargo, pip, npm and GitHub Actions; the `time` advisories are fixed (Rust
  1.88 or newer is required).

### Measurements and docs
* The RAG endpoint was measured against a real Ollama and PostgreSQL+pgvector: about 1.9 ms mean
  overhead (`benchmarks/README.md`).
* `CONTRIBUTING.md`, issue templates, a CI job that builds the generated Dockerfile (not run yet).

## 0.0.1 - first public alpha

A first version for trying the language out. Expect rough edges and breaking changes.

### Works
* Front end: lexer, parser with error recovery, typed HIR, literal type inference, tensor shapes
  checked at compile time, `Table<Row>`, loops, `let mut`, arrays, f-strings, modules and `import`.
* `telvax check | run | serve | jit | build | lsp | worker`.
* Interpreter on Arrow: tensors, tables, SQL (DataFusion), CSV and Parquet.
* `@endpoint` functions served over HTTP, with JSON validation; `build --serve` bundles the
  service into one executable and can write a Dockerfile.
* `--live` talks to real Ollama, OpenAI-compatible APIs and PostgreSQL + pgvector.
* Native backend (`--features llvm`, LLVM 22): ints, floats, strings, structs, static-shape `f32`
  tensors, int8 quantized `matmul`.
* `--device gpu` (`--features gpu`): `f32` tensor operations on the GPU through wgpu.
* `--emit ptx` and `--emit stablehlo` for NVIDIA and TPU toolchains (generated, **not executed on
  that hardware**).
* `tvpm` package manager, language server, VS Code extension, Wasm playground, Python bindings
  (zero-copy NumPy, Arrow, Polars, PyTorch), Ray executor, cluster workers.

### Known limits
See [`docs/design/status.md`](docs/design/status.md). Notably: native executables do not serve HTTP
yet, PTX / StableHLO / CUDA-array interchange have not run on real NVIDIA or TPU hardware, the
Dockerfile was never built, and there are no enums, `break` or `continue`.
