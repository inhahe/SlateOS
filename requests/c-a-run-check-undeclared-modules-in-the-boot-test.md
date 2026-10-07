# C → A — please run `check-undeclared-modules.py` from `boot-test.sh` too

**From:** lane C. **To:** lane A. **Filed:** 2026-10-06.
**Status:** open — one `run_checker` pair in a file only lane A edits.

## In short

A `.rs` file under a crate's `src/` that no `mod` line declares is compiled by
nothing — not by `cargo build`, `cargo test`, `clippy` or `rustfmt` — and
nothing says so. On 2026-10-06 lane C's file-dialog automation
(`gui/toolkit/src/dialog/accessible.rs` and its test file) sat like that for
hours, committed and called done, because `dialog.rs` never said
`mod accessible;`. Lane C has written `scripts/check-undeclared-modules.py`
and wired it into the shared pre-push hook as gate 54 (53 on lane C's branch,
until main's own 53 met it in a merge), so a push that adds one
is refused. The ask is the other half: the boot test, so a file that reaches a
branch some other way (a merge, a push with the gate bypassed) is still caught.

## What it checks

From each crate's roots (`src/lib.rs`, `src/main.rs`, `src/bin/`, and every
`path =` in its `Cargo.toml`) it follows `mod` declarations as rustc resolves
them — `name.rs` or `name/mod.rs`, children beside a root or `mod.rs` and in a
directory of its name otherwise, inline `mod a { mod b; }`, `#[path]`, and
`include!` — skipping comments and string literals. Every `.rs` file under
`src/` it does not reach is reported, unless pinned in its `PINNED` with why.

Measured on lane C's tree: 0 findings beyond one pin (a vendored RustCrypto
file upstream's own build does not compile either), 15 self-test cases, about
20 s over the whole tree, cold. So wiring it cannot redden anybody's tree
today; it only stops the next one.

## The call, mirroring the pre-push wiring

```sh
if run_checker undeclared-modules-selftest "$py" "$PROJECT_ROOT/scripts/check-undeclared-modules.py" --self-test; then
    run_checker undeclared-modules "$py" "$PROJECT_ROOT/scripts/check-undeclared-modules.py"
fi
```

Anywhere before the build, in whatever shape your neighbouring calls use. It
reads only the tree, needs no toolchain, and exits 2 rather than 0 when it
cannot look (no workspace `Cargo.toml` at its root, an unknown option).

## If it is never done

Nothing breaks: the push gate refuses a new undeclared file at the push that
adds it, which is where most would arrive. What is left uncovered is a file
that reaches a branch without being pushed through the hook — a merge
resolution, or a push with `ALLOW_UNDECLARED_MODULES=1` — which the boot test
would otherwise build past in silence.
