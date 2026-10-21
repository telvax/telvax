# Command line

`telvax` is one program with several commands: `telvax <command> [options]`. `telvax --version` prints the
version and `telvax --help` prints the list below. `telvax doctor` says what the build you have can do.

## Synopsis

```text
usage:
  telvax check [--json] [file.tvx...]                       no file: the package in this directory
  telvax lint <file.tvx> [--allow|--warn|--deny rule]... [--deny-warnings] [--list-rules]
                                                            unused variables and functions, unreachable cases
  telvax fmt [--check] [--diff] [file.tvx|dir]...             format in place (`-` reads stdin; no path: this directory)
  telvax schema <file.csv|.parquet|.arrow> [--name Row]      the `type` for the columns of a data file
  telvax doc [--public] [-o out.md] [file.tvx|dir]...      Markdown reference from the types, signatures and `#` comments
  telvax test [file.tvx|dir] [name] [--allow list] [--coverage] [--fail-under N]
                                                            run the `test_*` functions (a directory: `test_*.tvx`, `*_test.tvx`)
  telvax run <file.tvx> [function] [args...] [--allow list] [--quantize int8|int8-channel|int4-channel] [--device gpu]
  telvax profile <file.tvx> [function] [args...] [--allow list] [--top N] [--json f] [--collapsed f] [--speedscope f]
                                                            time per function, line and std operation, and memory
  telvax doctor                                             what this build can do and how it is set up
  telvax repl [file.tvx...] [--allow list]                  type definitions, variables and expressions and see the result
  telvax dap                                                debug adapter over stdio (breakpoints, stepping, variables)
  telvax lsp                                                language server over stdio
  telvax worker [--listen host:port] [--token T]            run calls for a coordinator
  telvax serve <file.tvx> [--port N] [--host H] [--seed docs.jsonl] [--live] [--allow list] [--quantize int8|int8-channel|int4-channel] [--device gpu] [--metrics] [--log-format json|text] [--log-level debug|info|warn|error] [--cors origins] [--rate-limit N/min] [--security-headers] [--process-time-header] [--shutdown-wait N] [--error-format telvax|detail]
  telvax build --serve <file.tvx> [-o out] [--dockerfile]   single-binary server (+ Dockerfile)
  telvax jit <file.tvx> <function> [args...]                 (native, needs --features llvm)
  telvax build <file.tvx> [-o out] [--emit exe|obj|llvm-ir|mlir|ptx|stablehlo] [-O 0-3] [--sm sm_80]
                                                    (all but stablehlo need --features llvm)

--allow takes a list of fs.read, fs.write, fs, env, net, all, none. Without it everything is allowed.
```

This block is the output of `telvax --help`, and a test checks that it still is.

## Exit status

| Status | Meaning |
|---|---|
| 0 | The command did what it was asked. |
| 1 | It failed: a program with errors, a failing test, a file that needs formatting, a missing file. The reason is printed after `error:`. |
| 2 | It was called wrongly: an unknown command (with a suggestion when one is close) or options that do not fit. |

## Options several commands share

`--allow list`
: The permissions the program has: `fs.read`, `fs.write`, `fs` (both), `env`, `net`, `all` or `none`, separated by commas
  (`--allow fs.read,net`). A function that needs one that was not given stops with an error. Without `--allow`
  everything is allowed. Taken by `run`, `test`, `profile`, `repl` and `serve`. (`lint` has its own
  `--allow rule`, see below.)

`--quantize int8 | int8-channel | int4-channel | none`
: Runs `f32` matrix products with integer arithmetic. See the section Quantization of the [tour](../guide/tour.md). Taken by `run`, `serve` and `build`.

`--device cpu | gpu`
: Where tensor operations run; `gpu` needs a build with the `gpu` feature and a GPU (`telvax doctor` says if the build has it). Taken by `run` and `serve`.

`--workers host:port,host:port`
: Sends the calls of `std.dist.map` to machines that run `telvax worker`. The environment variable `TELVAX_WORKERS` is the
  default. Taken by `run` and `serve`.

## Commands

### `telvax check`

Parses and type-checks the files named, or, with none, the package in the current directory (the one with a
`telvax.toml`). Prints each error with its place, and fails if there is any.
`--json` prints the diagnostics as JSON, one object each with `file`, `line`, `column`, `end_line`, `end_column`,
`severity`, `message` and `notes`, for editors and scripts.

### `telvax lint`

Checks a file, then reports code that is not used: variables, functions, cases that can never be reached.
`--list-rules` prints every rule with what it finds. The level of a rule (`allow`, `warn`, `deny`) comes from the
`[lint]` table of `telvax.toml` and then from the flags `--allow rule`, `--warn rule` and `--deny rule`.
`--deny-warnings` makes a warning fail the command.

### `telvax fmt`

Rewrites files in the one format Telvax has, and prints `formatted <file>` for each one it changed. A folder means
every `.tvx` file under it; no path means the current directory; `-` reads the source from the standard input and
writes the result to the standard output. `--check` changes nothing and fails if a file would change, which is what a
CI job wants. `--diff` shows what would change.

### `telvax schema`

Reads a `.csv`, `.parquet` or `.arrow` file and prints the `type` for its columns, to paste above
`let t: Table[Sales] = read_csv("sales.csv")`. A CSV is read in full: its header gives the names and its values the
types (`Int`, `Float`, `Bool`, `Date`, `Timestamp`, else `Text`), and a column with an empty value is written `T?`. For
Parquet and Arrow the file's own schema is used, including which columns may be empty. The type is named after the file
(`sales_2024.csv` gives `Sales2024`); `--name Row` picks another name. A column whose name is not a valid field name
(`unit price`) is reported, not renamed.

### `telvax doc`

Writes a Markdown reference of the functions, types, enums and traits in the files named (a folder means every `.tvx`
file under it), with the `#` comment lines written directly above each one. `--public` leaves out the names that
start with `_`. `-o out.md` writes a file instead of the standard output.

### `telvax test`

Runs the functions named `test_...` in a file; a test passes when it returns and fails when it stops with an error
(`assert`, `assert_eq`, `fail`, any runtime error). A folder runs every `test_*.tvx` and `*_test.tvx` file under it. A
second word, `name`, runs only the tests whose name contains it. It prints `N passed, M failed` and fails if any test
did. `--coverage` also reports which lines of the program the tests ran; `--fail-under N` (0 to 100) turns that into a
gate: the command fails if fewer than `N` percent of the lines ran. The functions of `std.test`, such as
`mock`, are in the [library reference](std.md).

### `telvax run`

Calls a function of a program: `telvax run app.tvx main`, or just `telvax run app.tvx` to call `main`. The words after the
function name are its arguments, in order. A `Text` parameter takes the word as it is; any other type takes JSON (`3`,
`true`, `[1, 2]`, `{"a": 1}`). The value the function returns is printed, unless it returns nothing.

### `telvax profile`

Runs a function like `run` and reports where the time and the memory went: per function, per line and per `std`
operation. `--top N` limits the report to the `N` statements that took the longest. `--json file`, `--collapsed file` (for flame graph tools) and
`--speedscope file` also write the report in those formats.

### `telvax doctor`

Prints the version, the syntax editions it reads, what this build can do (`tables`, live services, GPU, native
backend) and the environment variables that matter (`TELVAX_LIVE`, `OLLAMA_HOST`, `OPENAI_API_KEY`,
`TELVAX_WORKERS`, `TELVAX_CLUSTER_TOKEN`). A key or a token is shown as `set` or `not set`, never its value. Put the
output in a bug report.

### `telvax repl`

Starts an interactive session: type definitions, variables and expressions, and see the result. Files named on the
command line are loaded first. The same session is in the playground. In the session, `:help` lists its commands.

### `telvax dap`

The debug adapter (breakpoints, stepping, variables), speaking the Debug Adapter Protocol over the standard input and
output. Editors start it; the VS Code extension does. You do not run it by hand.

### `telvax lsp`

The language server, speaking the Language Server Protocol over the standard input and output. Editors start it; the
VS Code extension does. You do not run it by hand.

### `telvax worker`

Runs calls for a coordinator, so that `--workers` can spread `std.dist.map` over machines. It listens on
`--listen host:port` (default `127.0.0.1:7077`) and, when `--token T` or the environment variable
`TELVAX_CLUSTER_TOKEN` is set, answers only to a coordinator that has the same token.

### `telvax serve`

Serves the `@endpoint` functions of a program over HTTP, with the request validated and the answer written as JSON,
until it is stopped (Ctrl-C or SIGTERM). It listens on `--host` (default `127.0.0.1`) and `--port` (default `8080`). A program
without `@endpoint` functions is an error. `--live` uses the real Ollama, OpenAI and PostgreSQL services instead of the
offline stand-ins (and needs `net`). `--seed docs.jsonl` fills the vector store before the first request: one JSON
object per line with the text fields `collection`, `id` and `text`; each text is embedded and stored.

`--metrics` serves `GET /metrics` in the Prometheus format: `telvax_http_requests_total{method,route,status}`, the
histogram `telvax_http_request_duration_seconds{method,route}` and the counters the program makes with
`std.metrics.count(name)` as `telvax_counter_total{name}`. `route` is the path as written in the program, so the number
of series does not grow with the ids in the URLs. `--log-format json` (or `text`) writes one line per request to the
standard error: time, level, request id, method, path without the query string, status and duration. The level is
`info` for a status under 400, `warn` for 4xx and `error` for 5xx; `--log-level` (or `TELVAX_LOG_LEVEL`) is the lowest one
that is written, `info` by default, so `--log-level warn` keeps only the requests that failed. The lines of
`std.log` follow the same format and level. With either flag every
answer has an `x-request-id` header, the one the client sent if it is short and plain, otherwise a new one; the
function can read it with `req.header("x-request-id")`. A program started with `telvax build --serve` takes the same
flags, or `TELVAX_METRICS=1` and `TELVAX_LOG_FORMAT=json`; a flag wins over the variable.

A function that fails at run time (an index out of bounds, a division by zero) answers `500` with `Internal server error`, in both
error formats, because the interpreter's message names the inside of the program. The message goes to the standard error
with the request id, as a line `error GET /path 500 request_id=...: runtime error: ...` (a JSON object with `--log-format json`).
A function that returns `Err(Status.Internal(message))` keeps sending its `message`, because the program chose it.

Three more options go around every answer. `--cors ORIGINS` (`*`, or origins separated by commas) lets a browser page of
an allowed origin read the answer: it adds `access-control-allow-origin` and answers a preflight `OPTIONS` with `204`;
credentials are not supported. `--rate-limit N/unit` (`second`, `minute` or `hour`) allows `N` requests from one client in
each window, and answers `429` with `retry-after` after that; the client is the address of the connection, or the first
address of `x-forwarded-for` with `--trust-proxy`. `/healthz` is never limited. `--shutdown-wait N` (or `TELVAX_SHUTDOWN_WAIT`) is the number of seconds a server that was stopped
waits for its requests and background tasks (30 by default). `--security-headers` adds
`x-content-type-options: nosniff`, `x-frame-options: DENY`, `referrer-policy: no-referrer` and `cache-control: no-store`
to the answers that do not set them. The variables are `TELVAX_CORS`, `TELVAX_RATE_LIMIT`, `TELVAX_SECURITY_HEADERS`
and `TELVAX_TRUST_PROXY`. The counts of the rate limit are kept in memory, per process.

These options can also be written in the program, in a `serve:` block of the entry file:

```
serve:
    metrics: True
    log_format: "json"
    log_level: "info"
    cors: "*"
    rate_limit: "100/min"
    security_headers: True
    process_time_header: True
    trust_proxy: False
    shutdown_wait: 30
    error_format: "detail"
    on_start: [init_db]
```

Each line is one of the options above (`--log-format` is `log_format`), with a fixed value: `True` or `False`, a whole
number of seconds, or a text. A line can instead read a variable when the server starts, `env("NAME")` or
`env("NAME", "default")`, as in `cors: env("CORS_ORIGINS", "*")`: the value is the text of the variable, or the default when
the variable is unset or empty (with no default the line is left out), read as the flag reads it. A `True or False` option
is on for `1` or `true`; the default of one is `1`, `true`, `0` or `false`. `telvax check` refuses a default that the option
would refuse; a variable that the option refuses (`LOG_LEVEL=loud`) stops the start with
`serve: LOG_LEVEL="loud" is not a value of `log_level`: ...`. Nothing is converted: `RATE_LIMIT_PER_MINUTE=100` is not
`100/min`. `telvax check` reports a name that is not in the list, a value of the wrong type or one the
flag would refuse, a name written twice, a second `serve:` block, and a block in a file that is not the entry file. A
variable (`TELVAX_METRICS`, ...) wins over the block and a flag wins over the variable. `--port`, `--host`, `--live`,
`--allow`, `--seed`, `--quantize` and `--device` are not in the block: they depend on where the program runs. A binary
from `telvax build --serve` keeps the block.

`middleware: [names]` lists functions marked `@middleware`, of the form `fn name(req: Request, next: Next) -> Response`
(`Request`, `Response` and `Next` from `std.http`), outermost first. They run inside the built-in options and wrap every
request, the routes and `/health`, `/metrics`, `/openapi.json` and `/docs` too. A middleware that does not call `next`
answers by itself; one that calls it gets the `Response` of the rest of the chain, whose status and headers it may change
(the body is passed on as it was), and `next` can be called once. A middleware that fails answers with `500`. `telvax check`
refuses a name that is not a function marked `@middleware`, a wrong signature and a name written twice, and warns about a
`@middleware` of the entry file that the list does not mention. They run in the interpreter, which costs more than a built-in
option.

`on_start: [names]` is the one line that is not a literal: a list of top-level functions with no parameters, each giving
nothing or a `Result[(), Text]`, which run once, in the order written, before the port is opened (so `/health` does not
answer until they are done). They run with the permissions of the program (`--live`, `--allow`). A function that returns
an `Err`, or fails, stops the start: the message goes to the standard error as `on_start: NAME failed: MESSAGE` and the
exit status is 1. `telvax check` reports a name that is not a function, a function with parameters or another return type,
and a name written twice. A function can come from another file (`from db import init_db`). They do not run under `telvax run`
or `telvax test`.

### `telvax build --serve`

Makes one executable that contains the program and the server: `telvax build --serve app.tvx -o app`. It copies the
`telvax` that runs the command and appends the program, so it is as large as `telvax`. When started it
serves the program: it listens on `0.0.0.0`, on `PORT` if that is set, and `--port` and `--host` still win. `HOST` sets
the address, and `TELVAX_LIVE=1` turns on `--live`. `app --healthcheck [/path]` asks the server of this machine for
`/path` (`/healthz` if left out; the port is `--port`, `PORT` or 8080) and exits 0 on a 2xx answer, 1 otherwise: a
container has no `curl` for that. `--dockerfile` also writes a `Dockerfile` for it next to the program: it copies the
whole directory (the other modules and the files read by `std.fs.embed` have to be there; a `.dockerignore` keeps out
the rest), builds from `ghcr.io/telvax/telvax` (or the image in `--build-arg TELVAX_IMAGE=...`) and has a `HEALTHCHECK`
that calls `--healthcheck`.

### `telvax build`

Compiles a program with the native backend (LLVM and MLIR), so it needs a `telvax` built with `--features llvm`; the
published binaries are not, and `telvax doctor` says. `--emit` picks the output: `exe` (default, a program named after
the file), `obj` (an object file), `llvm-ir` and `mlir` (printed), `ptx` (printed; `--sm sm_80` picks the GPU
generation) and `stablehlo` (printed). `-o` names the output file. `-O 0` to `-O 3` is the optimisation level
(default 2). `--emit ptx` and `--emit stablehlo` do not need the native backend.

### `telvax jit`

Compiles a function with the native backend and calls it at once: `telvax jit app.tvx norm 3.0 4.0`. Needs `--features llvm`.
