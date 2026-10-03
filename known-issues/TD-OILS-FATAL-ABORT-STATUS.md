### TD-OILS-FATAL-ABORT-STATUS. `osh` exit status after a *fatal expansion abort* diverges from bash in `-c` mode and for arithmetic errors — OPEN (narrow, mode-dependent quirk; bash itself is inconsistent) 2026-07-19

**Where:** `userspace/oils/src/interp.rs` — the `Flow::Exit(1)` / `last_status`
handling in `exec_simple_inner` (nounset `unbound_error`, `${var?}` abort,
`arith_error`, `glob_error` branches) and `run_source`'s propagation for `-c`.

**What:** when a non-interactive shell aborts on a fatal expansion error, bash's
final exit status depends on the invocation mode and error kind, and osh does not
reproduce every case. Measured (bash 5.2.37):

| case | bash `-c` | osh `-c` | bash *script* | osh *script* |
|---|---|---|---|---|
| `${y?}` (unset param) | **127** | 1 | 1 | 1 |
| `set -u; $y` (nounset) | **127** | 1/2 | 1 | 1 |
| `$((1/0))` (div-by-zero) | 1 | 1 | **0** (continues!) | 1 |
| `${!x*extra}` (bad indirect) | 1 | 2 | — | — |
| `${x[1@]}` (bad subscript) | 1 | 0 | — | — |

The *observable* behaviour that matters — the diagnostic text, that the shell
aborts, and that following commands don't run — already matches bash. Only the
numeric exit code of the aborting shell differs, and only in these edge paths.

**Why deferred (not a quick fix):** bash is itself inconsistent here — the same
`${y?}` error yields 127 under `-c` but 1 as a script, and a div-by-zero *aborts*
under `-c` (status 1) yet *continues* as a script (status 0, from the next
command). A naive "always exit 127 on expansion abort" would break the
currently-matching script-mode cases (which are correct at 1). The correct fix
needs bash's exact rule reverse-engineered per error-kind × invocation-mode, plus
possibly making arithmetic `$((1/0))` non-fatal in script mode. Low value
(pathological error paths, exit code only), so parked here rather than guessed.
