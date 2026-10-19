# Status against the engineering specification

What exists for each part of the original specification, how it was checked, and what is still open.
"Tested" means an automated test in this repository; "measured" means a benchmark run on an Apple
M-series laptop. Things that could not be exercised on this hardware are called out.

## 1. Setup and tooling

| Item | State |
|---|---|
| Rust nightly, edition 2024 | done (`rust-toolchain.toml`) |
| `melior` / `inkwell` | done, against LLVM/MLIR **22** (the specification asks for 18+) |
| `arrow` + `datafusion` 52+ | done: Arrow 59, DataFusion 55 |
| `pyo3` 0.21+ | done: PyO3 0.29 |
| Test runner for `.tvx` programs | done and tested: `telvax test` runs `test_*` functions in a file or a directory, with fixtures (`fixture_*`, `teardown_*`), cases (`cases_*`), mocks (`std.test.mock`) and line coverage (`--coverage`, `--fail-under`). Coverage counts lines, not branches; mocks do not reach method calls (`x.len()`); no parallel runs or test timeouts |
| Workspace crates | all present: `telvax_lexer`, `_parser`, `_ast`, `_hir` (`compiler/semantic`), `_mlir` (`compiler/ir`), `_codegen`, `_runtime`, `_server`, `_cli`; plus `_diagnostics`, `_modules`, `_pkg`, `_lsp`, `_cluster`, `_wasm`, `_bench` |

## 2. Frontend

| Item | State |
|---|---|
| `logos` lexer, `chumsky` parser with recovery | done; recovery keeps later items after an error (tested) |
| Pipeline operator | done; desugared to a call in the HIR |
| Tensor types with shape enforcement | done: shapes are static types, `matmul`/`add`/`reshape`/... check dimensions at compile time (tested) |
| Relational types | done as `Table[Row]`: the schema is a class, columns are checked, `t.col` is a typed column. Columns are primitives; there are no joins at the type level (use SQL). Methods that keep the schema: `head`, `tail`, `sort_by`, `sort_by_desc`, `filter`, `concat` (tested), `select` and `group_by` (`sum`, `mean`, `min`, `max`, `count`), whose result row types are inferred from the names. Joins and the rest of the pandas API are not there yet. The schema is never read from a file by the compiler: `telvax schema data.csv` prints the `type` to paste (CSV, Parquet, Arrow) |
| AST for pipelines, tensors, endpoints | done |

## 3. Execution backend and interoperability

| Item | State |
|---|---|
| Tensors and tables backed by Arrow | done in the runtime (`ArrayRef` / `RecordBatch`). Reading a table column as a tensor shares the buffer (tested by pointer equality) |
| Ingestion from CSV, Parquet, memory | done and tested (CSV and Parquet round trips, `from_rows`, JSON). CSV and Parquet files are decoded into Arrow arrays; Arrow IPC (Feather v2) files are memory-mapped (`read_ipc`, `write_ipc`). Arrow Flight (client, `read_flight` / `write_flight`) is an optional build feature, tested against an in-memory server only. Tables can be cut into chunks (`split`, `read_*_batches`), but a table is still one in-memory batch and there is no lazy batch reader. Columns typed `T?` hold nulls; other columns reject them. `Date` and `Timestamp` columns are Arrow temporal types, but their values are ISO-8601 `Text` in the program (no date arithmetic yet) |
| Arrow C Data Interface | done through the Arrow PyCapsule protocol (`__arrow_c_array__`, `__arrow_c_stream__`): PyArrow and Polars in and out without copies (tested) |
| PyO3 bridge, NumPy / PyTorch | done: NumPy `__array_interface__`, PyTorch through NumPy, pointer equality checked (tested with real NumPy, PyTorch, Polars) |
| `cuda_array_interface` | protocol implemented (`DeviceTensor` adopts and re-exports a GPU buffer by pointer) and tested **with stand-in objects only**. It has not run against a physical GPU, and Telvax has no GPU kernels: device tensors cannot be passed to Telvax functions |
| Scalar bridging overhead | measured: 0.3-0.6 microseconds per call |
| GPU execution (Metal / Vulkan / DX12) | done and **run on real hardware** (Apple M4 over Metal): `--device gpu` runs `f32` `add sub mul div scale relu sigmoid softmax matmul sum mean dot` as WGSL kernels through `wgpu` (`runtime` feature `gpu`). Tests compare against the CPU path within `f32` tolerance; other operations, other dtypes and int8 matmul stay on the CPU. Each call copies its inputs to the device and the result back, so it pays off for large matrices, not small ones. Operations nested in one expression (`softmax(matmul(relu(matmul(x, w1)), w2))`) keep their intermediate results on the GPU and copy only the leaves up and the final result down (tested by counting transfers); splitting the same code over `let` statements copies each intermediate through the host. Measured on the M4 for a 3-layer network (512x1024 input): 44 ms chained, 47 ms split, 9.0 s in the interpreter on the CPU. The M4 shares memory with the CPU, so avoiding the copies saves little here; discrete GPUs have not been measured |
| NVIDIA GPUs (PTX) | `telvax build --emit ptx [--sm sm_80]` compiles kernels for the `std.tensor` operations a program uses (same set as above) with LLVM's NVPTX backend. The LLVM verifier accepts them and the PTX text is checked, but **it has never been executed on an NVIDIA GPU** (none available) and there is no CUDA host launcher |
| TPUs (StableHLO) | `telvax build --emit stablehlo` translates straight-line tensor functions (assignments, `std.tensor` calls, `+ - * /`, calls between such functions) to StableHLO text; functions with loops, branches or other types are reported as skipped. The output is **not run through XLA/PJRT or on a TPU** |

## 4. MLOps and deployment

| Item | State |
|---|---|
| `@endpoint` -> routing table, validation, JSON | done (`telvax_server`); validation derives from the types, including tensor shapes (tested). `{name}` segments of the path are read into parameters (tested); the request (method, path, headers) is read through a `Request` parameter (tested); the status and headers of the answer are set with `Response` (tested) |
| `std.task.spawn` | done: a task outlives the request that started it under `telvax serve` (tested end to end); `on_error` hears of a failure (tested); no handle, no cancellation, no queue |
| `std.task.spawn` | done: a task outlives the request that started it under `telvax serve` (tested end to end); no handle, no cancellation, no queue |
| `std.cache` | done: limit, expiry and least-recently-used per shard (tested); the type of the values is not checked, `Cache[V]` of RFC 0014 is not there yet |
| Speed | measured on a service of two modules: one request of 100 texts takes 3.6 ms (it took 15 ms), 10 clients at once give 1,170 requests per second on 10 cores; the interpreter still allocates for every call and every `Text` |
| Counters with labels | done: `std.metrics.count(name, labels, by)` and `--process-time-header` (tested; no gauges or histograms of the program) |
| Stopping `serve` | done: `SIGTERM` finishes the requests, waits for the background tasks up to `--shutdown-wait`, `/healthz` says 503 meanwhile (tested with a real signal on Unix; not tried on Windows) |
| `serve:` block | done: the options of `serve` (`metrics`, `log_format`, `log_level`, `cors`, `rate_limit`, `security_headers`, `process_time_header`, `trust_proxy`, `shutdown_wait`, `error_format`) written in the entry file; `check` reports wrong names, types and values; a variable and a flag win over it; a bundled binary keeps it (tested). Values are literals; none from the environment |
| Error format | done: `--error-format detail` gives `{"detail": ...}` and `422` for a body that does not fit, and the OpenAPI document follows (tested end to end); the default is unchanged |
| `std.log` | done: `debug`, `info`, `warn`, `error` with `Text` fields, in the format and at the level of `serve` (tested); no logger objects, fields are text only, and the request id is not added to the lines of the program |
| `std.random` | done: `uuid` (version 4) and `int(low, high)` from the operating system (tested); no seed, no generator type |
| `@endpoint` -> routing table, validation, JSON | done (`telvax_server`); validation derives from the types, including tensor shapes (tested). `{name}` segments of the path are read into parameters (tested); the request (method, path, headers) is read through a `Request` parameter (tested); the status and headers of the answer are set with `Response` (tested); `Response.events` streams `text/event-stream` (tested); a field with a literal default or typed `T?` may be left out of the body (tested) |
| `--cors`, `--rate-limit`, `--security-headers` | done (tested). The rate limit is a fixed window per client, in memory, so it is per process; CORS has no credentials |
| `--metrics`, `--log-format` | done: `/metrics` in the Prometheus format, a log line per request with a level that `--log-level` filters, `x-request-id` (tested); no tracing, and the histogram buckets are fixed |
| OpenAPI | done: `/openapi.json` and `/docs` come from the endpoint types (tested); no named components, no `T?`/enum detail, no `Response` body schema |
| Embedded Axum server | done, but the handlers run in the **interpreter**; native binaries do not serve HTTP |
| `telvax build --serve` | done: one binary containing the program (all modules, no source tree needed at run time) and the files named by `std.fs.embed` (tested) |
| Dockerfile | generated (multi-stage, starts from the published `telvax` image, copies the program's whole directory, distroless runtime) and its content tested. A CI job (`docker`) builds it and calls the container; it passes on `main`. Docker is not available on the development machine |
| Docker image | `Dockerfile` at the root builds `telvax` and `tvpm`; the release workflow pushes it to `ghcr.io/telvax/telvax` (amd64 and arm64, tagged with the version and `latest`). The CI job `docker` builds it and runs `--version`; the publishing jobs only run on a release and have not run yet |
| `std.db.postgres` | done: `execute` and a typed `query` with bound parameters, tested against a real PostgreSQL 17 (`live` feature, `net`); one connection per client, no transactions or pool, and `timestamptz`, `numeric`, `uuid` and arrays must be cast to text |
| Real services | Ollama, OpenAI-compatible chat, PostgreSQL+pgvector: HTTP clients tested against local stand-in servers; pgvector tested against a real PostgreSQL 17; Ollama exercised against a real local Ollama (`nomic-embed-text`). The OpenAI client was only tested against a stand-in server (no API key) |

## 5. Alpha 0.1.0 targets (`benchmarks/`)

| Target | Measured |
|---|---|
| RAG overhead < 15 ms | with the offline stand-ins: p99 0.88 ms (1 connection), 6.0 ms (8 connections), 2,000 documents. With a real Ollama and a real PostgreSQL+pgvector (500 documents, sequential, LLM stubbed): about 1.9 ms mean overhead, ~6 ms p99, 14 MB RSS (see `benchmarks/README.md`) |
| Base RSS < 30 MB | 17.8 MB idle |
| High-throughput vector ingestion | CSV 19.7 M rows/s, Parquet read 88 M rows/s, 84 K embedded documents/s into the in-memory store |

## 6. Launch

| Item | State |
|---|---|
| VS Code extension with LSP | done: snippets and a language server (diagnostics, hover, completion, definition, references and rename for variables, functions, globals, types, variants, fields and methods, across the files of a project (not into dependencies), formatting, inferred-type hints, module aware; completion is contextual: types, members of built-in and user types, enum variants, closure parameters, table columns in strings, ordered by the expected type) tested over real stdio, and the extension is exercised **inside a real VS Code** (downloaded by `@vscode/test-electron`: language mapping, diagnostics, hover, definition, references, rename, completion, formatting; run locally on macOS and as the `vscode-host` CI job). The TextMate grammar is tested with the real tokenizer. The `.vsix` is attached to each release; it is not yet on the Marketplace or Open VSX (the release publishes there once `VSCE_PAT` and `OVSX_PAT` exist, see `editors/vscode/README.md`) |
| Debugger | done for the interpreter: `telvax dap` is a debug adapter (Debug Adapter Protocol) for the interpreter: breakpoints by line, step over, into and out, pause, the calls in progress, variables that expand (lists, maps, types, variants, tuples), hover/evaluate of a name or a field, and the program's output in the debug console; tested by driving it over stdio like an editor. The VS Code extension uses it (F5, a launch configuration, breakpoints in the gutter), exercised inside a real VS Code that sets a breakpoint and reads the stack and the variables. No conditional breakpoints, no evaluation of other expressions, no stop on a runtime error. The native backend has no debug information |
| Profiler | done for the interpreter: `telvax profile` reports the time and calls of each function (itself and with its calls), the hits and time of each statement, the time, size of inputs and results of each `std` function, and the memory (peak, bytes allocated, allocations; bytes allocated by each function), and writes JSON, flame graph stacks and speedscope files (tested). The time is wall-clock inside the interpreter, so it includes the interpreter's own work; memory counts the whole process, so with several threads a function also counts what other threads allocate. Methods of `Text`, `List` and `Map` are not listed as operations. Programs from `telvax build` (native) are not profiled |
| REPL | done in the terminal and the browser: `telvax repl` keeps definitions (`fn`, `type`, `enum`, `trait`, `extend`, `import`) and variables (`let`, `var`) from one input to the next, shows the value of an expression, checks each input with what came before and leaves the session unchanged on an error, completes with Tab, has history, and loads files (tested as a library and through the binary, and the terminal in a pty by hand), and the same session is a console in the browser playground (tested as the `.wasm` under Node and in a real Chromium). Each input is checked as a function over the variables, so a closure kept in a variable needs the types of its parameters, a number written alone is an `Int`, and `await` is not available |
| Notebooks | done for Jupyter: `pip install "telvax[kernel]"` registers a Telvax kernel over `ipykernel` (cells, shared definitions and variables, the value of the last expression with tables and tensors as HTML, printed output, errors with the line, Tab, Shift+Tab, `:type` and `:vars`), and `%%telvax` runs Telvax cells in a Python notebook; tested against a real kernel over ZeroMQ sockets, in a fresh virtualenv from the built wheel, and in IPython. Not tried in a notebook front end (JupyterLab, VS Code). Output appears when a cell ends, a running cell cannot be interrupted, and cells are highlighted as Python. No `.ipynb` tooling (`telvax fmt` and `check` do not read notebooks) |
| WebAssembly playground | done: the engine is tested as a real `.wasm` under Node (check, run, hover, step limit) and the page itself runs in a **real Chromium** through Playwright (load, run, Ctrl+Enter, type errors, runtime errors, examples, share link; run locally and as the `playground-browser` CI job). Only Chromium has been tried |
| Documentation with runnable examples | partly done: the code blocks of the README, the tour and the use cases, and every file in `examples/`, are checked, formatted and (with `fn main`) run by `cargo test` (`cli/tests/docs.rs`). Blocks that are pieces of a program are marked `ignore`. `telvax doc` writes a Markdown reference from signatures, types and the `#` comments above each item (tested as a library and through the binary). It has no HTML output, does not link between types, and does not run examples found in comments; `docs/reference/` is empty |
| Dual Apache-2.0 / MIT | done |

## 7. Ecosystem

| Item | State |
|---|---|
| `tvpm` package manager | done: manifests, semver resolution with backtracking, lockfile, directory and HTTP registries, checksums, yank, `import` across files and packages (tested end to end) |
| Distributed backends | `std.dist.map` runs on local threads, on `telvax worker` processes (coordinator with retries and token auth; node loss tested with workers on one machine) or on **Ray** (tested against a real local Ray cluster). Multi-machine runs were not tried |
| Quantization passes | `--quantize int8` lowers `f32` `matmul` to a symmetric per-tensor int8 kernel with i32 accumulation, in both the interpreter and the MLIR lowering; the two agree bit for bit (tested). `--quantize int8-channel` and `--quantize int4-channel` (interpreter only; the native backend refuses them) use one scale per row of the activations and per column of the weights, the latter with 4-bit weights. With columns 1000x apart in magnitude the worst relative error is 100% per-tensor, 0.75% per-channel int8 and 18% per-channel int4 (tested). Only `matmul`; scales are computed per call, so there is **no calibration** against sample data |

## Known gaps

* The full `telvax` is about 51 MB because of Arrow, DataFusion and Parquet. `telvax-lite` (about 10 MB) leaves out DataFusion and Parquet: `std.table.sql` and `read_parquet` report an error there, while in-memory tables, CSV and everything else work.
* The native backend supports a subset (see the README); tables, SQL, closures and most of `std` are interpreter-only.
* Strings, vectors and tensors allocated by native code are never freed.
* The interpreter is a tree walker: fine for serving glue, not for heavy numeric loops (use `telvax build`).
* Enums and `match` run in the interpreter only; the native backend reports them as unsupported.
* `telvax fmt` takes files and directories (`--check`, `--diff`) and keeps the line breaks you wrote (it fixes indentation, spacing and blank lines); it does not re-wrap long lines and has no settings. `telvax lint` has three rules: unused variables, unused private functions (names that start with `_`) and unreachable `case`s. Each can be set to `allow`, `warn` or `deny` in `telvax.toml` or with flags; there are no comments to silence one line.
* `cargo test --workspace` runs on a Windows runner in CI and passes; it has not been tried on a physical Windows machine, and the native backend is not built there. Each published release is installed and run on Windows, Linux and macOS by the `verify` job of the release workflow.
* PTX and StableHLO output have not been executed on NVIDIA or TPU hardware.
