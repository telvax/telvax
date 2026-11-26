# What 1.0 promises

[Syntax stability](stability.md) promises the language: a program of edition 2026 is accepted by every later Telvax. This
page is the rest of what "1.0" means: what a 1.x release keeps working, what is experimental, and what is not promised at
all. A promise here has a test behind it; something without one is listed as not promised.

## Kept stable in 1.x

| Area | What is kept | What may change |
|---|---|---|
| The language | The syntax of the editions the compiler reads, as [stability](stability.md) says. | Error messages and lint warnings. |
| The standard library | Every function and method keeps its name, its parameters and what it does. New functions, methods and overloads may come. | Removing or changing one needs a new edition. The text of the reference. |
| The command line | The commands of `telvax` and `tvpm`, their flags and their [exit status](../reference/cli.md#exit-status): 0 done, 1 failed, 2 called wrongly. | New commands and flags. The text written for people (messages, `--help`, tables, progress). |
| `telvax check --json` | An array of records with `file`, `line`, `column`, `end_line`, `end_column`, `severity` and `message`, and `notes`. | New fields may be added; a reader ignores the ones it does not know. |
| The log lines of `serve` (`--log-format json`) | One JSON object per line with `ts`, `level`, `request_id`, `method`, `path`, `status`, `duration_ms` and `trace_id`. | New fields; the order of the fields. |
| What a service answers | `/healthz` (`200`, and `503` while it stops), `/metrics` in the Prometheus format, the request and trace headers (`x-request-id`, `traceparent`), and the meaning of the options of `serve` and of their variables. | The names of the metrics may gain labels. `/docs` is for people. |
| Project files | `telvax.toml` and `telvax.lock`, and the layout of a registry (`index/<name>.json`, `.nvpkg`). A reader ignores fields it does not know, so a newer file still loads. | New fields. |
| Model files | `std.model` files of format version 1 load in every 1.x. A newer version is refused with a message, never misread. | New format versions, which older releases refuse. |
| Editor protocols | `telvax lsp` follows the Language Server Protocol and `telvax dap` the Debug Adapter Protocol. | Which optional features of the protocols are offered. |

A form that will go away is deprecated first: `telvax check` warns about it with what to write instead, for at least one
minor release, and it is only removed by a new edition (see [stability](stability.md#deprecated-forms)).

## Platforms

| Tier | Platforms | What it means |
|---|---|---|
| 1 | Linux x86_64 and arm64, macOS arm64 | Built and tested on every change, binaries in every release, and the release checks them after it is published. A failure there blocks a release. |
| 2 | macOS x86_64 | Binaries in every release, built on an arm64 Mac and checked under Rosetta. Not tested on an Intel Mac. |
| Experimental | Windows x86_64 | Built and tested in CI; the release adds its binaries when they build, and does not wait for them. It has not been tried on a physical Windows machine. |

The native backend is not available on Windows.

## Experimental

These work and are tested, but a 1.x release may change or remove them, and they are named experimental where they are
documented:

* **The native backend** (`telvax build` with `--emit exe`, `obj`, `llvm-ir`, `mlir`): it compiles a subset of the
  language (numbers, `f32` and `f64` tensors, loops, f-strings), needs a build with LLVM that the published binaries are
  not, and does not free the strings and vectors it allocates. The interpreter runs every program.
* **Accelerators**: `--device gpu` runs on real hardware over Metal; `--emit ptx` and `--emit stablehlo` produce output that
  has never run on an NVIDIA GPU or a TPU. `--quantize` is part of this.
* **The preview edition** `2027-preview` (see [stability](stability.md#the-preview-edition)).
* **The Python module** and the Arrow Flight build.

## Not promised

* **Speed.** The interpreter walks the tree: it is fast enough to serve and to glue a pipeline, not for heavy numeric loops,
  which belong in the native operations of `std` (tensors, sparse matrices, tables). A release may be slower in some
  program; a large slowdown is a bug.
* **Output for people**: messages, help, `telvax doc`, `telvax profile` and its files, `telvax schema`, `/openapi.json`
  beyond what the program declares, and the formatting of code that the formatter used to reject.
* **The internals**: the crates of the workspace as Rust libraries, the HIR, and the files under `target/`.

## Building from source

Telvax builds with a nightly Rust named in `rust-toolchain.toml`. A 1.x release names the nightly it was built with, and
building that release needs that one; a newer nightly may work. A stable toolchain is not a promise of 1.0.

## Releases

1.0.0 and the releases after it are published as releases, not pre-releases. A pre-release is a version with a suffix
(`1.1.0-rc.1`), for testing before the release it names. The changes of each release are in [what's new](../../CHANGELOG.md),
with a **change** note for anything that a program or a script may notice.
