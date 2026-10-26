# Contributing to Telvax

Thanks for trying Telvax. At 0.0.x the most useful contribution is a bug report or a program that
did not behave as you expected.

## Reporting problems

Open an issue and include:

* the output of `telvax --version` and your operating system;
* the smallest `.tvx` program that shows the problem;
* what you ran (`telvax run ...`, `telvax serve ...`) and the full output.

Language ideas are welcome too, ideally with a short example of the code you wish you could write.

A change to the syntax, the type system or the standard library starts as an RFC in
[telvax/rfcs](https://github.com/telvax/rfcs): see its README for the process. The pull request here
links the accepted RFC. Translations of the documentation are in `telvax/docs-pt-br`, `docs-es` and
`docs-zh-cn`; the English files in `docs/` stay the source.

## Building

Nightly Rust is required; `rust-toolchain.toml` selects it.

```sh
cargo build --release -p telvax_cli      # target/release/telvax
cargo test --workspace                  # default build, no LLVM needed
```

Optional pieces:

| Feature | What it adds | Needs |
|---|---|---|
| `--features telvax_cli/llvm` | native code, `--emit ptx` | LLVM 22 with MLIR (`. scripts/llvm-env.sh`) |
| `--features telvax_cli/gpu` | `--device gpu` | a GPU with Metal, Vulkan or DX12 |

The Python bindings (`python/`), the VS Code extension (`editors/vscode`), the Wasm playground
(`playground/`) and the documentation site (`site/`, published by version, see its README) have their own README and tests.

## How changes get into `main`

`main` is protected: nothing is pushed to it directly, not even by the maintainer. Work happens on a
branch and reaches `main` through a pull request, and the pull request has to pass every check
(formatting, clippy, tests on Linux and Windows, the LLVM and GPU builds, the lite build, the
Python bindings, the playground in a real browser, the extension in a real VS Code, the Docker
image, `cargo deny` and the secret scan) before it can be merged. Merges are squashed, so the pull
request title becomes the commit message: keep it short and in the imperative (`feat: add ...`).

```sh
git switch -c fix/short-name
# ...edit, commit...
git push -u origin fix/short-name
gh pr create --fill
gh pr merge --auto --squash      # merges by itself once every check is green
```

You can still run the checks locally while you work (see below), but a green pull request is what
counts.

## Before sending a pull request

```sh
cargo fmt --all
cargo clippy --workspace --all-targets
cargo test --workspace
cargo deny check
```

* Code in the docs is tested (`cli/tests/docs.rs`): a ```` ```telvax ```` block must pass `telvax check` and
  `telvax fmt --check`, and run when it has `fn main`. Mark a piece of a program ```` ```telvax ignore ````.
  Files under `examples/` get the same checks.
* Add a test with the change. Compiler tests live next to the crate (`compiler/*/tests`), runtime
  tests in `runtime/tests`.
* Keep commits short and in the imperative, with the existing prefixes: `feat:`, `fix:`, `docs:`,
  `test:`, `refactor:`, `style:`, `ci:`, `build:`, `chore:`.
* If a change alters the language, update `docs/guide/tour.md` and `CHANGELOG.md`.
* The programs in `tests/editions/` are frozen (see `docs/guide/stability.md`). A change must not make
  one fail, and must not edit one. If it cannot avoid that, it is a breaking change: a new edition and
  a `feat!:` title. New syntax gets a new frozen program and a line in that directory's `CHECKSUMS`.
* `docs/design/status.md` says what has and has not been run on real hardware. Keep it honest: do
  not mark something as working because it compiles.

## Where things are

See the table in the README. A quick map for the language itself: `compiler/lexer` (tokens),
`compiler/parser` (syntax), `compiler/semantic` (types and checks), `runtime` (interpreter),
`compiler/ir` and `compiler/codegen` (native), `compiler/accel` (PTX and StableHLO).

A new standard library function goes in `compiler/semantic/src/stdlib.rs` (its type) and
`runtime/src/std_impl.rs` (what it does). Describe it in `compiler/semantic/src/stdlib_docs.rs` too: a test
fails without the description, and `docs/reference/std.md` is written from it with
`UPDATE_REFERENCE=1 cargo test -p telvax_hir --test reference`.

## Releases

A pull request merged into `main` is released automatically, and its **title** (a
[Conventional Commit](https://www.conventionalcommits.org)) picks the version, so no file has to be
edited. A release takes an hour and only the newest of the runs that wait is kept, so one release can
carry several merges: the title of **each** commit since the latest tag counts, and the biggest bump
wins (a `feat:` and then a `fix:` is a minor release):

| Title | Version | Example |
|---|---|---|
| `feat!: ...` or a `BREAKING CHANGE:` line in the body | major (`1.2.3` to `2.0.0`; while the major is 0, minor) | `feat!: rename the pipe operator` |
| `feat: ...` | minor (`0.1.2` to `0.2.0`) | `feat: add traits` |
| `fix:`, `perf:`, `refactor:`, `build:`, `revert:` or no prefix | patch (`0.1.2` to `0.1.3`) | `fix: empty table in sql` |
| `docs:`, `ci:`, `test:`, `style:`, `chore:` | no release | `docs: add a use case` |

Add `[skip release]` to a title and that merge counts for nothing; it does not cancel the others. To pick a version yourself, raise
`version` in the root `Cargo.toml` above the latest tag. The release notes are the `CHANGELOG.md`
section for that version, if you wrote one, followed by the list of merged pull requests.

## Security

See [SECURITY.md](SECURITY.md).
