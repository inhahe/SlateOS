### BUG-OILS-DEVNULL-READ-UNMAPPED. Three `redir.stdin` read paths bypassed `map_device_path`, so `read x < /dev/null` on the Windows host read a stray `D:\dev\null` file — 2026-07-27 — ✅ RESOLVED 2026-07-27

**Symptom.** `cargo test -p oils` failed three tests
(`dev_null_read_yields_eof`, `dev_null_write_discards`,
`read_a_creates_empty_array_on_eof`) with the *contents of a real file* where
EOF was expected — 70 asterisks and a "Visual Studio 2022 Developer Command
Prompt" banner. Pre-existing (reproduced on a clean `git stash`), so not a
regression from the trap work that surfaced it.

**Root cause.** Two independent halves.

1. **Host pollution.** `D:\dev\null` exists as an ordinary 295-byte file (also
   `D:\dev\full`, `\stderr`, `\stdout`, `\test_dev`), created by *non-osh*
   tooling: a `cmd.exe`-side `>/dev/null` resolves `/dev/null` against the
   current drive root. `map_device_path` (interp.rs ~17499) was added precisely
   to stop `osh` doing this, mapping `/dev/null` → `NUL` on Windows.
2. **Three read paths never called it.** `map_device_path` was applied at 13 of
   16 sites, but the `read`-builtin input helpers and the stdin slurp took the
   raw path: `std::fs::read(path)` (~13893) and two
   `std::fs::File::open(path)` (~15504 line reader, ~15559 record reader).
   `resolve_redirects` deliberately stores the **unmapped** path in
   `plan.stdin` ("downstream code re-opens the path when it actually reads"),
   so every downstream consumer has to map — and these three forgot. The
   validating open in `resolve_redirects` *did* map, so `< /dev/null` was
   accepted and then read from the wrong file.

**Fix.** Added `map_device_path` to all three. Audited every remaining
`File::open`/`File::create`/`OpenOptions`/`fs::read` in non-test code: the only
other unmapped ones are `fs::metadata` calls backing the `test`/`[` file
predicates and process-substitution temp files, neither of which should map.

**Residual (host-only, deliberately not fixed).** The stray `D:\dev\*` files
are outside the repo and were not created by this project's code, so they were
left in place; with the mapping fix the tests pass regardless of whether they
exist. `[ -e /dev/null ]` still stats the stray path on Windows — a host-only
artifact with no SlateOS-target equivalent (there `/dev/null` is a real device
node).
