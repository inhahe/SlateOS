## TD-EXPLORER-CLIPPY-ALL-TARGETS-WAS-RED (lane C, 2026-08-16) — FIXED

`cargo clippy -p explorer --all-targets` failed outright on three
`useless_vec` errors in `apps/explorer/src/columns.rs` test code (clippy is
deny-level for the crate). Because the failure was in the *test* target, a plain
`cargo clippy -p explorer` was green and nobody saw it. Fixed in `3b0056f7a`.

Worth generalising: **run the clippy gate with `--all-targets`.** A crate whose
test target does not lint is a crate whose lint findings are half-observed, and
the failure mode is silent — the command exits 0.

Still open in this crate: 46 `arithmetic_side_effects` warnings in non-test code
(unchanged by this work, counted before and after). Not yet triaged.
