## `TD-D-THE-VARIANT-GATE-LET-A-SIBLING-ENUM-OUTRANK-A-FILES-OWN-STRUCT` -- `check-variant-lists.py` resolved a list's element type to an enum elsewhere in the crate when the file itself defined a struct of that name (lane D, 2026-10-07) -- FIXED 2026-10-07
**Status:** FIXED 2026-10-07 -- `resolve_enum` asks the file's own structs, unions and type aliases before the crate's enums, with two self-test cases

**In short:** the boot test refuses to build when a list of an enum's
values no longer names every value. To know which enum a list is of,
the checker looks the list's element type up by name. It looked in the
same file for an enum, then anywhere in the same crate. It never asked
whether the same file defined a *struct* of that name first. So
`posix/src/gensalt.rs`, which defines a private `struct Method` and a
fifteen-row table of them, `METHODS`, was judged against
`posix/src/crypt.rs`'s unrelated twelve-variant `enum Method`. The table
was reported as missing every variant, and lane D's round 196 boot was
refused fifteen minutes in.

**The fix.** Resolution now follows Rust's own order. An item a file
defines needs no `use`. An enum in a sibling module is in scope only
through one. So a struct, union or type alias that the file defines
settles the question before the crate's enums are tried. The gate
already treated a struct of that name elsewhere in the crate as settling
it when no enum of that name was in the crate. That earlier rule is the
`gui/keylayout` `struct Level` against `kernel/src/klog.rs` `enum Level`
case. This fix only moves the same-file half of that rule ahead of the
crate's enums.

**Why it is additive.** The change can only turn a report into a skip,
and only for a list whose element type the file itself defines as a
non-enum. On the whole tree the run before and after holds the same 149
lists that name every variant. The single difference is gensalt's table.
A second self-test case pins the boundary: a struct in a *sibling* file
still leaves the crate's enum in charge, as before.

**What is still approximate** (unchanged by this fix, and failing toward
checking more rather than less): a bare name used in one file and defined
as an enum in a sibling file is taken to mean that enum whether or not
the file imports it. The gate tolerates glob imports that way rather than
modelling Rust's module paths.

**Found by** round 196's boot test (lane-d `bc07a1083`). The pre-push
hook does not run this gate; `scripts/pre-boot.py` does, and lane D now
runs it beside its pipelines.
