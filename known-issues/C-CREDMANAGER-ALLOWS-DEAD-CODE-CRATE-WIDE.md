### C-CREDMANAGER-ALLOWS-DEAD-CODE-CRATE-WIDE — 2026-08-26 — LANE C, OPEN, tech debt

**What it is.** `apps/credmanager/src/main.rs` carries `#![allow(dead_code)]`
at crate level. Nineteen items are genuinely unreached from the binary: the CSV
export (`export_csv`, `escape_csv`), the backup serialiser
(`serialize_backup`), the clipboard `copy`, `IdGen`, and the constructors for
the entry kinds the Add button does not open a form for. They are finished and
tested code waiting on a button, not corpses, so deleting them is wrong.

**Why it matters.** A blanket allow does not distinguish "not wired yet" from
"orphaned". It swallows both. The conversion above left `rows_top` and
`build_render_tree` reachable from nothing but tests, and the lane gate's
`-D dead_code` said nothing, because the allow was already covering nineteen
other things. They were found by removing the allow by hand and reading the
list — which is not a process, it is a coincidence.

**What was done now.** Both orphans are `#[cfg(test)]`, which is the honest
marker for "test-only" and keeps them under the lint's eye in the binary. The
allow is back, with a comment naming exactly what it is expected to cover.

**The proper fix.** Replace the crate-level allow with a per-item
`#[allow(dead_code)]` and a one-line reason on each of the nineteen, so that
anything *else* going dead is a gate failure on the day it happens rather than
whenever someone next removes the allow out of curiosity. The reason to do it
per-item rather than deleting the code is that each of those items is a feature
with a roadmap entry: they want wiring, not a funeral.

**Trigger:** do this when credmanager next gets a control wired (the Add form,
or an Export menu item), since that pass will delete some of the nineteen
annotations anyway.
