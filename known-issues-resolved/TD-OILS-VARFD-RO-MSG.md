### TD-OILS-VARFD-RO-MSG. `osh` readonly-varfd redirect emits one error line; bash emits two — 2026-07-19 — ✅ FIXED 2026-07-20

**Status:** FIXED. `redir_effective_fd`'s readonly-varfd branch now returns both
diagnostics — `{target}: readonly variable` followed by a `line N:`-prefixed
`{target}: cannot assign fd to variable` — so `readonly v=abc; echo x {v}>f`
prints two lines exactly like bash (both status 1, command not run, `$v`
unchanged). Verified byte-for-byte against bash 5.2 and regression-guarded by
`varfd_readonly_target_emits_two_diagnostics`.

**Where:** `userspace/oils/src/interp.rs` `redir_effective_fd` (the readonly
varfd branch), emitted by the two `resolve_redirects`/`redir_effective_fd`
callers via `err_prefix()`.
