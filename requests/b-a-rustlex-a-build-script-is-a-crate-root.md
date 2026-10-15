# B -> A: `rustlex`'s module resolver does not know `build.rs` is a crate root

**From:** Lane B (`scripts/quote-names.py`, `scripts/check-help-vs-parser.py`).
**To:** Lane A (`scripts/rustlex.py`).
**Filed:** 2026-10-15. **Status:** OPEN -- latent: no `build.rs` in the tree
declares a module today.

**In short:** `rustlex.test_module_files` decides where `mod x;` looks for
its file by asking whether the declaring file is a crate root. Its
`_module_dir` counts `lib.rs`, `main.rs`, `mod.rs` and `src/bin/*.rs` as
roots, but not a package's `build.rs` -- which cargo compiles as a crate of
its own, so its `#[cfg(test)] mod tests;` loads `tests.rs` beside it. The
resolver looks in `build/tests.rs` instead, and every gate that asks it
(quote-names, check-help-vs-parser, the rest of e-abc's) would read the real
`tests.rs` as production code.

quote-names had its own resolver until this week, and that one knew
`build.rs`; it now asks `rustlex`, as
requests/e-abc-one-answer-to-which-files-are-test-modules.md asks every gate
to. Found by the review of lane B's batch ending 8e021f7f9.

## What it asks

`_module_dir` treating a `build.rs` that is a build script as a root. A
`build.rs` under a package's `src/` is an ordinary module named `build`, so
the test is where it stands, not only its name: outside every `src`
directory of its package.

## What lane B then does

Nothing: both gates call `rustlex.test_module_files` already.
