# E → A: `rustlex.live_code` leaves a test item without a body live

**From:** lane E · **To:** lane A (`scripts/rustlex.py`) · **Filed:**
2026-10-10
**Status:** open. Nothing in lane E is blocked, but two of lane E's gates
read through `live_code` and inherit both defects:
`check-unused-exports.py` (its `production` docstring names this request)
and `key-survey.py`.

## In short

`live_code` blanks a `#[cfg(test)]` item by matching its braces. Two kinds of
test item come through it live:

1. **An item with no braces of its own** loses only its attribute, and the
   item itself stays: `#[cfg(test)] pub mod x;`, `use ...;`,
   `const X: T = ...;`, `static`, `type`, `extern crate`, `pub struct X;`,
   and `pub struct X(..);`. The code is the non-block branch at
   `rustlex.py:388-401`, where `end = i + len("#[cfg(test)]")`.
2. **A test function whose signature has a `;` inside brackets** --
   `fn f(a: [u8; 2]) {`, or `-> [T; 11] {` -- is taken for an item of the
   first kind. That is because the branch compares the first `;` with the
   first `{` at any depth. The function is left live whole, body and all.

## Measured on lane-e-wip, 2026-10-10 (every tracked `.rs`)

| Shape | Count | For example |
|---|---|---|
| `mod x;` | about 140 | `apps/fieldmenu/src/lib.rs:307` |
| `const`, `static`, `type` | 45 | `posix/src/environ.rs:788` `pub static ENV_TEST_LOCK` |
| `use` | 18 | `apps/checkers/src/main.rs:19` |
| test function, `;` in its signature | 15 | `gui/appearance/src/lib.rs:2489` `fn roles(&self) -> [(&'static str, Color); 11] {` |
| `extern crate` | 6 | `gui/vulkan/src/lib.rs:142` |
| `pub mod x;` | 4 | `gui/desktop/src/lib.rs:97` `pub mod draw_check;` |
| unit and tuple structs | 3 | `apps/hwquery/src/query.rs:1270` `pub struct StubProvider;` |

What each consumer sees as a result:

- `check-unused-exports.py` follows `gui/desktop`'s test-only
  `pub mod draw_check;` and reports its exports as unused.
- A survey of test function bodies (`find-*`, `frozen-flag-survey`,
  `check-read-defaults`) reads the 15 test functions as production.

Nineteen scripts in lanes B, D and E read through `live_code`, so one fix
reaches all of them.

## A fix, for lane A to judge

- **Decide by the first `{` or `;` at bracket depth 0**, not by the first of
  each at any depth. A `;` inside `[T; N]` or `(..)` then no longer ends
  anything. One trap: the `>` of `->` and `=>` must not lower the depth.
  `_field_end` lowers it on every `>`, so `fn f() -> [u8; 2] {` would reach
  depth -1 at the arrow and see its `;` at depth 0.
- **In the non-block branch, blank through the item's own `;`** -- the first
  one at depth 0, which is what `_field_end` already finds for a field --
  rather than through the attribute alone.

Cases for the self-test, each one a shape above:

- `#[cfg(test)] pub mod x;` followed by `pub fn after() {}`: `x` gone,
  `after` live.
- `#[cfg(test)] pub struct Stub;`
- `#[cfg(test)] use std::cell::RefCell;`
- `#[cfg(test)] fn f(a: [u8; 2]) { body() }`: `body` gone.
- `#[cfg(test)] fn g() -> [u8; 2] { body() }`: the arrow case.

## And one thing in `boot-test.sh`

When `check-unused-exports.py` fails, the boot test's message says an
export's documentation "names a caller that does not exist". The script
makes no such check. It exits non-zero only when its self-test fails or it
finds fewer library crates than its floor; without `--strict` its report
never refuses. The message should name those two cases instead.
