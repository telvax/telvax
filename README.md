# Telvax

Telvax is a statically typed, compiled language for data processing, ML pipelines and AI serving.
It is easy to read for anyone who knows Python (blocks by indentation, no braces or semicolons)
and has its own words: `fn`, `let` / `var`, `type`, `x => x + 1`, `Int?`.
Tensor shapes are part of the type system, a pipeline operator (`|>`) composes stages, and a
function marked `@endpoint` becomes an HTTP route with validation and JSON handling generated
for it.

```telvax
import std.ai.embeddings.OllamaEmbedder
import std.http.Status

let embedder = OllamaEmbedder.new("nomic-embed-text")

type QueryPayload:
    query: Text
    top_k: i32

fn embed(text: Text) -> Tensor[f32, 768]:
    text |> std.text.clean_whitespace() |> std.text.truncate(512) |> embedder.embed()

@endpoint(POST, "/v1/embed")
fn handle(payload: QueryPayload) -> Result[Tensor[f32, 768], Status]:
    if payload.query.is_empty():
        return Err(Status.BadRequest("empty query"))
    Ok(embed(payload.query))
```

The full RAG service from the specification is in [`examples/rag-service.tvx`](examples/rag-service.tvx).

> **Status: 0.0.4, an early alpha.** Good for trying the language and giving feedback; expect
> bugs and breaking changes in the library and tools; the syntax of edition 2026 is frozen
> ([`docs/guide/stability.md`](docs/guide/stability.md)). What works, and what has not been run on real hardware, is listed in
> [`docs/design/status.md`](docs/design/status.md). Bug reports and ideas are welcome in the issues.
>
> The documentation is also a website: <https://telvax.github.io/telvax/>. Every function and method of the standard
> library is in [`docs/reference/std.md`](docs/reference/std.md), and every command in
> [`docs/reference/cli.md`](docs/reference/cli.md).

## What you can build

| Use case | Example |
|---|---|
| Summarise a CSV / Parquet file with typed columns and SQL | [`examples/dataframe/sales.tvx`](examples/dataframe/sales.tvx) |
| Models whose tensor shapes are checked before they run | [`examples/ml/forward.tvx`](examples/ml/forward.tvx) |
| Turn a function into a validated JSON HTTP route | [`examples/service/sentiment.tvx`](examples/service/sentiment.tvx) |
| A RAG service: embeddings, vector search, an LLM | [`examples/rag-service.tvx`](examples/rag-service.tvx) |
| Text processing with `Map`, `List` and iterator methods | [`examples/text/word-count.tvx`](examples/text/word-count.tvx) |
| Errors as values: `Result`, `Option`, `?`, `if x is Some(v)` | [`examples/errors/parse-config.tvx`](examples/errors/parse-config.tvx) |

Each one comes with the commands to run it in [`docs/guide/use-cases.md`](docs/guide/use-cases.md).

## Install

```sh
curl -fsSL https://raw.githubusercontent.com/telvax/telvax/main/scripts/install.sh | sh
telvax --version
```

The script downloads a release binary or, if there is none for your system, builds from source
(needs [Rust](https://rustup.rs)). To build by hand: `cargo install --locked --path cli`.

Two builds are published for each system: the full `telvax` (about 50 MB, with SQL and Parquet)
and `telvax-lite` (about 10 MB). The lite build has everything else, in-memory tables and CSV
included; `std.table.sql` and Parquet report a clear error there. `TELVAX_LITE=1` makes the install script pick the lite one.

On Windows (PowerShell):

```powershell
irm https://raw.githubusercontent.com/telvax/telvax/main/scripts/install.ps1 | iex
```

Windows builds are new and have only been compiled and tested by CI, not by hand. The native
backend (`--features llvm`) is not available on Windows yet.

## Quick start

```sh
cargo build --release -p telvax_cli          # target/release/telvax
telvax doctor                                 # what this build can do (native backend, GPU, live)
telvax check examples/rag-service.tvx         # parse + type-check, diagnostics point at the exact span
telvax check --json examples/rag-service.tvx  # the same diagnostics as JSON, for CI and editors
telvax run examples/basics/primes.tvx main    # interpret
telvax repl                                   # type definitions, variables and expressions and see the result
pip install "telvax[kernel]"                   # a Jupyter kernel for Telvax and the %%telvax magic (python/README.md)
telvax dap                                    # debug adapter over stdio: breakpoints, stepping, variables
telvax profile app.tvx main --top 10          # time per function, line and std operation; memory
telvax test tests/ --coverage                 # run `test_*` functions with fixtures, cases and mocks; lines not run
telvax fmt --check                            # every .tvx file under this directory (--diff shows the changes)
telvax doc src/ -o REFERENCE.md               # Markdown reference of functions, types and enums, with their `#` comments
telvax serve examples/rag-service.tvx         # HTTP server on :8080 (offline model stand-ins)
# curl -XPOST localhost:8080/v1/rag/query -H 'content-type: application/json' -d '{"query":"hello","top_k":3}'
telvax serve examples/rag-service.tvx --live  # real Ollama / OpenAI / PostgreSQL+pgvector
telvax run model.tvx infer --device gpu       # tensor ops on the GPU (build with --features gpu)
telvax build model.tvx --emit stablehlo       # StableHLO for XLA / TPUs (--emit ptx for NVIDIA)
telvax build --serve examples/rag-service.tvx -o service --dockerfile   # one binary + Dockerfile
```

Nightly Rust is required (`rust-toolchain.toml` selects it). More in the [tour](docs/guide/tour.md) and the [use cases](docs/guide/use-cases.md).

## What is here

| Area | Where | Notes |
|---|---|---|
| Compiler front end | `compiler/{lexer,parser,ast,semantic}` | `logos` + `chumsky` with error recovery; typed HIR; literal type inference; tensor shapes checked statically |
| Interpreter and runtime | `runtime/` | Arrow-backed tensors and tables, CSV / Parquet / SQL (DataFusion), real Ollama / OpenAI / pgvector clients |
| Native backend | `compiler/{ir,codegen}` | MLIR (`melior`) and LLVM (`inkwell`) 22; JIT and executables; int8 quantization of `matmul`. Optional: `--features llvm` |
| HTTP serving | `server/`, `cli/` | `@endpoint` becomes an Axum route; `telvax build --serve` embeds the program in one binary |
| Modules and packages | `compiler/modules`, `tools/package-manager` | `import` between files, `tvpm` with registries, a resolver and a lockfile |
| Editor support | `tools/lsp`, `editors/vscode` | diagnostics, hover, contextual completion, go to definition, inferred-type hints |
| Web playground | `playground/` | the checker and interpreter compiled to WebAssembly, with a console (a REPL) |
| Accelerators | `compiler/accel/` | StableHLO and PTX generators (not run on TPU / NVIDIA hardware); GPU execution lives in `runtime` (`gpu` feature) |
| Python | `python/` | PyO3 module; zero-copy Arrow, NumPy and CUDA-array interchange; Ray executor |
| Distribution | `tools/cluster` | `telvax worker` + `--workers` for `std.dist.map` |
| Benchmarks | `benchmarks/` | alpha targets: RAG overhead, memory footprint, ingestion throughput |

[`docs/design/status.md`](docs/design/status.md) maps every item of the original engineering
specification to what exists, what was verified and how, and what is still open.

## Language notes

* **Values.** Types you define and `Text` are values; a `List` is a reference (copies share elements).
  Tensors are immutable values. Untyped number literals take their type from how they are used.
* **Tensors.** `Tensor[f32, 2, 3]` carries its shape: `std.tensor.matmul` of mismatched
  shapes is a compile error. Symbolic dimensions (`[N]`) accept any size.
* **Tables.** `Table[Row]` is an Arrow table whose columns are the fields of type `Row`;
  `t.score` is the column as a zero-copy tensor. `head`, `tail`, `sort_by`, `sort_by_desc`, `select`, `filter`, `concat` and `split`
  are methods; `sql` runs a query. `read_ipc` / `write_ipc` use Arrow IPC (Feather) files, memory-mapped on
  read; `read_flight` / `write_flight` talk to an Arrow Flight server (build with `--features flight`). A column typed `T?` may hold nulls, and `Date` / `Timestamp` columns are Arrow temporal types whose
  values are ISO-8601 `Text`.
* **Modules.** `import util.helper`, `from crate.geometry import Point, norm`,
  `from mathx.vec import dot` (a dependency). A name that starts with `_` is private and cannot
  be imported.
* **Native subset.** Integers, floats, bools, strings, types you define, number lists, static-shape
  `f32`/`f64` tensors, loops and f-strings (ints, bools, strings). Arrow functions (`x => ...`), `Table`, SQL and most
  of `std` run in the interpreter only.

## Testing

Telvax programs have their own test runner: a function called `test_...` is a test, `fixture_...`
functions give it parameters, `cases_...` runs it on several values, `std.test.mock` replaces a
function, and `--coverage` and `--fail-under N` report and hold the lines the tests ran. The
[tour](docs/guide/tour.md#tests) has the details. To test this repository:

```sh
cargo test --workspace                       # no LLVM needed
. scripts/llvm-env.sh && cargo test --workspace --features llvm
cd python && maturin develop && python -m pytest tests
scripts/build-playground.sh --node && node --test playground/test
cd editors/vscode && npm ci && npm test
```

## License

Dual-licensed under MIT (`LICENSE-MIT`) or Apache 2.0 (`LICENSE-APACHE`): you may use Telvax under
either one, at your option.

## Community

Questions and ideas go to [Discussions](https://github.com/telvax/telvax/discussions); bugs and
feature requests to [issues](https://github.com/telvax/telvax/issues). Please read
[CONTRIBUTING.md](CONTRIBUTING.md) and the [code of conduct](CODE_OF_CONDUCT.md) first, and report
security problems privately as described in [SECURITY.md](SECURITY.md).
