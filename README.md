<div align="center">

# Telvax

**A statically typed, compiled language for data, ML pipelines and AI serving.**

[![CI](https://github.com/telvax/telvax/actions/workflows/ci.yml/badge.svg)](https://github.com/telvax/telvax/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/telvax/telvax?include_prereleases&label=release)](https://github.com/telvax/telvax/releases)
[![License](https://img.shields.io/badge/license-MIT%20%2F%20Apache--2.0-blue)](#license)

[Documentation](https://telvax.github.io/telvax/) ·
[Tour](docs/guide/tour.md) ·
[Standard library](docs/reference/std.md) ·
[Command line](docs/reference/cli.md) ·
[Discussions](https://github.com/telvax/telvax/discussions)

</div>

Telvax is a programming language. You write programs in text files that end in `.tvx` and run them with
the `telvax` command, which you install once. It is made for programs that work with data (tables, CSV,
Parquet), machine learning (tensors) and AI services (a web server that answers JSON requests).
If you have never used it, start with [Quick start](#quick-start): it needs no other software.

Telvax reads like Python (blocks by indentation, no braces or semicolons) and checks like a compiled
language. Tensor shapes are part of the type system, a pipeline operator (`|>`) composes stages, and a
function marked `@endpoint` becomes an HTTP route with validation and JSON handling generated for it.

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

This is a service: it needs a running [Ollama](https://ollama.com) to answer. The full RAG service is in
[`examples/rag-service.tvx`](examples/rag-service.tvx), and it runs offline with stand-ins for the models.
A program with nothing to set up is in [Quick start](#quick-start).

> **Early alpha.** Good for trying the language and giving feedback. Expect bugs and breaking changes
> in the library and the tools; the syntax of edition 2026 is frozen ([stability](docs/guide/stability.md)).
> What works, and what has not been run on real hardware, is listed in [`docs/design/status.md`](docs/design/status.md).

## Highlights

* **Tensor shapes in the types.** `Tensor[f32, 2, 3]` carries its shape, so a `matmul` of mismatched shapes is a compile error.
* **Typed tables.** `Table[Row]` is an Arrow table whose columns are the fields of `Row`; it reads CSV, Parquet and Arrow, and runs SQL.
* **Services without glue.** `@endpoint` turns a function into a validated JSON route; `telvax build --serve` makes one binary and a Dockerfile.
* **Errors are values.** `Result`, `Option`, `T?`, the `?` operator and and `if x is Some(v)`.
* **Tools included.** Formatter, type checker with exact spans, REPL, test runner with coverage, profiler, debugger (DAP), language server, VS Code extension, package manager (`tvpm`) and a Jupyter kernel.
* **Interpreter and native backend.** Run a program right away, or compile it with MLIR and LLVM; StableHLO and PTX generators are available for accelerators.

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
```

Nothing opens when it finishes: Telvax is a command-line tool, with no window and no app. The script
prints one line and puts two programs, `telvax` and `tvpm` (the package manager), in `~/.telvax/bin`.
On macOS and Linux it does not change your `PATH`, so add the folder yourself (put the line in `~/.zshrc` or `~/.bashrc`
to keep it) and check:

```sh
export PATH="$HOME/.telvax/bin:$PATH"
telvax --version        # prints: telvax, then the version you installed
```

If the shell answers `command not found: telvax`, the `PATH` line is missing. If there is no binary
for your system, the script builds from source (needs [Rust](https://rustup.rs)) and `cargo` puts the
programs in `~/.cargo/bin`. To build by hand: `cargo install --locked --path cli`.

Two builds are published for each system: the full `telvax` (about 50 MB, with SQL and Parquet)
and `telvax-lite` (about 10 MB). The lite build has everything else, in-memory tables and CSV
included; `std.table.sql` and Parquet report a clear error there. `TELVAX_LITE=1` makes the install script pick the lite one.

Each release also publishes a Docker image for `linux/amd64` and `linux/arm64`, with `telvax` and `tvpm`:

```sh
docker run --rm -v "$PWD:/work" -w /work ghcr.io/telvax/telvax run app.tvx main
```

The tags are the version of the release (see the [releases](https://github.com/telvax/telvax/releases)) and `latest`.

On Windows (PowerShell):

```powershell
irm https://raw.githubusercontent.com/telvax/telvax/main/scripts/install.ps1 | iex
```

On Windows the script adds the folder to your `PATH` (open a new terminal). Windows builds are new and have only been compiled and tested by CI, not by hand. The native
backend (`--features llvm`) is not available on Windows yet.

## Quick start

Install Telvax (see [Install](#install)), then save this as `hello.tvx`:

```telvax
fn main():
    let names = ["Ada", "Alan", "Grace"]
    for name in names:
        print(f"hello, {name}")
```

```sh
telvax run hello.tvx main
```

It prints:

```text
hello, Ada
hello, Alan
hello, Grace
```

`main` is the function to run. `telvax check hello.tvx` only reads and type-checks the file and prints
`` `hello.tvx` ok ``; a mistake is reported with the line and column. More commands:

```sh
telvax check hello.tvx          # parse and type-check; diagnostics point at the exact span
telvax repl                     # type definitions and expressions and see the result
telvax fmt --check              # every .tvx file under this directory
telvax test tests/ --coverage   # run the `test_*` functions
telvax serve examples/rag-service.tvx   # HTTP server on :8080 (offline model stand-ins)
```

Every command and flag is in [`docs/reference/cli.md`](docs/reference/cli.md). For notebooks,
`pip install "telvax[kernel]"` adds a Jupyter kernel and the `%%telvax` magic ([`python/README.md`](python/README.md)). To build from a clone,
nightly Rust is required (`rust-toolchain.toml` selects it): `cargo build --release -p telvax_cli`.

## Documentation

| Read | For |
|---|---|
| [Tour](docs/guide/tour.md) | the language, from the first program to modules and tests |
| [Use cases](docs/guide/use-cases.md) | dataframes, models, services and RAG, with the commands to run each one |
| [Standard library](docs/reference/std.md) | every function and method of `std` |
| [Command line](docs/reference/cli.md) | every `telvax` and `tvpm` command |
| [Stability](docs/guide/stability.md) | what is frozen and what may change |
| [Status](docs/design/status.md) | what exists, what was verified and how, and what is open |
| [Syntax](docs/design/sintaxe-pythonica.md) | the design of the syntax |

The documentation is also a website: <https://telvax.github.io/telvax/>.

## Language notes

* **Values.** Types you define and `Text` are values; a `List` is a reference (copies share elements).
  Tensors are immutable values. Untyped number literals take their type from how they are used.
* **Tensors.** `Tensor[f32, 2, 3]` carries its shape: `std.tensor.matmul` of mismatched
  shapes is a compile error. Symbolic dimensions (`[N]`) accept any size.
* **Tables.** `Table[Row]` is an Arrow table whose columns are the fields of type `Row`;
  `t.score` is the column as a zero-copy tensor. `head`, `tail`, `sort_by`, `sort_by_desc`, `select`, `group_by`, `filter`, `concat` and `split`
  are methods; `sql` runs a query. `read_ipc` / `write_ipc` use Arrow IPC (Feather) files, memory-mapped on
  read; `read_flight` / `write_flight` talk to an Arrow Flight server (build with `--features flight`). A column typed `T?` may hold nulls, and `Date` / `Timestamp` columns are Arrow temporal types whose
  values are ISO-8601 `Text`.
* **Modules.** `import util.helper`, `from crate.geometry import Point, norm`,
  `from mathx.vec import dot` (a dependency). A name that starts with `_` is private and cannot
  be imported.
* **Native subset.** Integers, floats, bools, strings, types you define, number lists, static-shape
  `f32`/`f64` tensors, loops and f-strings (ints, bools, strings). Arrow functions (`x => ...`), `Table`, SQL and most
  of `std` run in the interpreter only.

## Repository layout

| Area | Where | Notes |
|---|---|---|
| Compiler front end | `compiler/{lexer,parser,ast,semantic}` | `logos` + `chumsky` with error recovery; typed HIR; literal type inference; tensor shapes checked statically |
| Interpreter and runtime | `runtime/` | Arrow-backed tensors and tables, CSV / Parquet / SQL (DataFusion), real Ollama / OpenAI / pgvector clients, SQL on PostgreSQL |
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

## Contributing

Read [CONTRIBUTING.md](CONTRIBUTING.md) first. A change to the syntax, the type system or the standard
library starts as an RFC in [`telvax/rfcs`](https://github.com/telvax/rfcs).

## License

Dual-licensed under MIT (`LICENSE-MIT`) or Apache 2.0 (`LICENSE-APACHE`): you may use Telvax under
either one, at your option.

## Community

Questions and ideas go to [Discussions](https://github.com/telvax/telvax/discussions); bugs and
feature requests to [issues](https://github.com/telvax/telvax/issues). Please read the [code of conduct](CODE_OF_CONDUCT.md) first, and report
security problems privately as described in [SECURITY.md](SECURITY.md).

## Other repositories

| Repository | What it holds |
|---|---|
| [`telvax/rfcs`](https://github.com/telvax/rfcs) | proposals for changes to the language and the tools |
| [`telvax/docs-pt-br`](https://github.com/telvax/docs-pt-br), [`docs-es`](https://github.com/telvax/docs-es), [`docs-zh-cn`](https://github.com/telvax/docs-zh-cn) | translations of the guide (none translated yet) |
| [`telvax/website`](https://github.com/telvax/website) | the public site (not built yet; the documentation site is `site/` here) |
| [`telvax/workflows`](https://github.com/telvax/workflows) | shared GitHub Actions workflows (none yet) |
| [`telvax/telvax-grammar`](https://github.com/telvax/telvax-grammar) | a copy of the TextMate grammar, for tools that take it from a repository (GitHub Linguist) |
| [`telvax/.github`](https://github.com/telvax/.github) | organization profile and default community files |
