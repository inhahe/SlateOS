### TD-OILS11. `osh` `trap`: async-signal handlers (`INT`/`TERM`/…) are stored but not delivered — OPEN (gated on kernel signal/exception support)

**Where:** `userspace/oils/src/interp.rs` (`builtin_trap`, `fire_trap`,
`run_exit_trap`, `traps` map on `Shell`) and `userspace/oils/src/main.rs`.

**What:** `trap` records handlers for any valid signal/pseudo-signal spec and
prints/lists/resets them correctly. The **synchronous** traps are now all
fired: `EXIT` (once on top-level shell exit), `ERR` (on a failing command
outside an exempt context — same rule as `errexit`, independent of `set -e`),
`DEBUG` (before each simple command), and `RETURN` (on function return, in the
function's scope). The `DEBUG`/`RETURN` traps honour **functrace**
(`set -T`/`set -o functrace`) and the per-function trace attribute
(`declare -ft NAME`); the `ERR` trap honours **errtrace** (`set -E`/`set -o
errtrace`). With the corresponding trace option off, the trap is NOT inherited
into a called function (matching bash) — a failing command inside an untraced
function does not fire the caller's `ERR` trap, only the function call itself
failing at the caller level does. Inheritance is implemented by *masking*
(`Shell::trap_suppress`, per-function frame) rather than removing the trap, so a
trap set inside a function **persists globally** (`trap -p` still shows it) and
fires for later top-level commands — matching bash, and fixing the earlier
discard-on-return model. Subshells (`( … )`) likewise inherit `DEBUG`/`RETURN`
under functrace and `ERR` under errtrace. `ERR` firing uses an "armed at command
start" snapshot so a function that installs its own `ERR` trap and then fails
does not double-fire at the caller. **Residual divergence (minor):** the exact
`ERR` firing *count* under `errtrace` with **deeply nested** spaced subshells
(`( ( false ) )`) diverges — bash caps at 2 firings regardless of depth via
undocumented internal bookkeeping, whereas `osh` fires once per boundary. And
command substitutions `$( … )` intentionally do NOT run the trace traps for
their internal commands (bash's `ERR`-in-`$()` behaviour is quirky — it captures
the trap's own stdout into the result and fires on the sub's overall status).
Both are exotic corners where bash itself is inconsistent. What remains
unimplemented is **async signal delivery** —
handlers for real signals (`INT`/Ctrl-C, `TERM`, `HUP`, …) are stored and
printed faithfully but never invoked, because there is no OS-level signal
delivery on the host and SlateOS uses IPC/exceptions rather than Unix signals.

**Proper fix:** map SlateOS exception/IPC "signal" events to the stored
handlers once the process model exposes them (e.g. a keyboard-interrupt or
kill-equivalent IPC message the shell polls between commands, or an async
callback). Deferred: the synchronous traps cover the overwhelmingly-common
uses (`EXIT` cleanup, `ERR` reporting, `DEBUG`/`RETURN` tracing), and the
storage/print/list/reset surface is complete.

~~**Also (minor):** a `RETURN` trap is not yet fired for a returning **sourced
script** (only function returns), and an `exit N` *inside* an `ERR`/`DEBUG`/
`RETURN`/`EXIT` handler does not propagate to actually exit the shell.~~
**Both FIXED 2026-07-27** — see `BUG-OILS-TRAP-EXIT-SWALLOWED` and
`BUG-OILS-SOURCE-FRAME-SEMANTICS` below.

~~**Also (TD-OILS-TRAP-CAPTURE, minor):** synchronous trap handler *output* is
written through `fire_trap` → `run_source` (a fresh `Out::Inherit`, i.e. the
real stdout), so it is NOT captured by an enclosing command substitution.~~
**FIXED 2026-07-27** — see `BUG-OILS-NESTED-CODE-LOSES-CALLERS-FDS` below. The
"bash is subtle and inconsistent here" reading recorded in this entry was wrong:
bash's rule is simply *"handler output goes to the shell's current stdout"*, and
the `DEBUG`-to-the-terminal lines that seemed to contradict it were the DEBUG
traps firing in the **parent** for the commands outside the substitution.
