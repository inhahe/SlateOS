### TD-OILS-CMODE-EXIT. `osh -c` fatal-expansion exit status is 1, not bash's `-c`-only 127 — 2026-07-19 — ✅ RESOLVED 2026-07-19

**What:** bash exits with **127** when a fatal parameter-expansion error
(`${var:?msg}`, `set -u` on an unbound variable) aborts a `bash -c STRING`
invocation, but exits with **1** for the same error in a *script file*
(`bash script.sh`). osh originally returned 1 in both modes.

**Resolution:** osh now carries a `command_mode` flag (set by `main.rs` for
`-c`), and `fatal_abort_status(code)` returns the raw `code` (127) only at
the main shell in command mode, else 1. Verified: `osh -c 'echo
"${var:?msg}"'` → 127; `osh script.sh` (same body) → 1 — both match bash.

**Follow-up (also resolved 2026-07-19):** with **errexit** (`-e`) enabled,
bash downgrades that 127 to **1** (it treats the fatal expansion as a failed
command). This keys purely on the `-e` option being set, regardless of
whether errexit would fire in the current context (`set -eu; echo $UNDEF ||
true` still exits 1). `fatal_abort_status` now maps 127→1 when `self.errexit`
is set. Covered by `nounset_abort_under_errexit_is_one`.
