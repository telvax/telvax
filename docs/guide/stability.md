# Syntax stability

A program that works today keeps working. The syntax is grouped in **editions**. The only edition so
far is `2026`, and its syntax is frozen: from the release that contains this page, a program written in
edition 2026 is accepted by every later Telvax that reads edition 2026.

## What is promised

* **Programs are not broken.** Every program in `tests/editions/2026/` must keep passing
  `telvax check`, `telvax fmt --check` and, when it has `test_...` functions, `telvax test`. The test
  that runs them is `cli/tests/editions.rs`.
* **The files are frozen.** `tests/editions/2026/CHECKSUMS` holds a hash of each program. Changing a
  program, or adding one without its line, fails the test. A pull request that has to edit a frozen
  program is a breaking change and needs a new edition (below).
* **New syntax may be added.** A new word, form or standard library function that makes no old program
  stop working is an ordinary `feat:` change. New programs go into the frozen directory too, so the
  promise grows.
* **Error messages are not part of it.** The text of a diagnostic, the lint warnings and the output of
  `telvax fmt` for code the formatter used to reject may change. The formatter does not change the
  layout of a program that is already formatted.

## What is not promised

* The `std` library can gain functions, and a function can get a new overload. Removing or changing one
  follows the same rule as syntax: a new edition.
* The native backend, GPU execution and the accelerator outputs are listed in
  `docs/design/status.md`; what runs there can grow, and a program that did not compile natively may.
* A bug fix may reject a program that only worked because of the bug (a type error the checker missed).
  The change says so in `CHANGELOG.md`.

## Choosing an edition

`telvax.toml` names the edition of a package:

```toml
[package]
name = "app"
version = "0.1.0"
edition = "2026"
```

* Without the line, the package is read as the newest stable edition (`2026`), never as a preview.
  `tvpm new` writes it, so a new package is pinned from the start.
* An edition the compiler does not know is an error that lists the ones it does and says to update
  Telvax if the package is newer.
* A single file with no `telvax.toml` is read as the newest stable edition.

## A new edition

A change that makes an old program stop compiling, or changes what it means, goes in a new edition
and not in the existing one:

1. The old edition keeps compiling as it did. The compiler reads both, chosen by `edition`.
2. The new directory `tests/editions/<year>/` gets its own frozen programs and `CHECKSUMS`, and the
   year is added to `EDITIONS` in `tools/package-manager/src/manifest.rs`.
3. `CHANGELOG.md` lists what changed, and `telvax-migrate` (`tools/migrate`) gets a rule that rewrites
   old source to the new edition where that can be done mechanically.
4. How long an old edition stays supported is decided when the second edition is proposed; dropping
   one is a `feat!:` change of its own.

## Deprecated forms

A form that a later edition removes is **deprecated** first: `telvax check` and the language server report it as a
warning that says what to write instead and which edition removes it, and `telvax check --deny-warnings` makes it an
error. A form stays deprecated, with its warning, for at least one minor release before an edition removes it, and in the
edition it was written for it keeps working for as long as that edition is supported.

## The preview edition

`2027-preview` is a rehearsal of the next edition, read only by a package that asks for it (`edition = "2027-preview"`):
a file with no `telvax.toml` and a package without the line are still read as 2026. It is not stable: it may change, or
go if the next edition is not wanted, and then its directory is deleted while the code and tests that proved the
mechanism stay. Its one difference from 2026: `std.string` is gone, and its functions (`join`, `slice`, `repeat`) are in
`std.text`, where 2026 has them too. In 2026 `std.string` still works, with a deprecation warning.

`telvax migrate` rewrites a 2026 program to it (see the command line reference), and `tests/editions/2027-preview/` has
its frozen programs, with a `telvax.toml` that names the edition.

## What exists today

Done: the `edition` line and its check, two editions read by the compiler, the language server and the formatter
(`2026` and `2027-preview`, chosen by the package), the frozen programs of each and the test that guards them,
deprecation warnings, `telvax migrate` in the binary, and this page. The `telvax-migrate` tool converts the old
Rust-style syntax and is still a separate program that will be removed.
