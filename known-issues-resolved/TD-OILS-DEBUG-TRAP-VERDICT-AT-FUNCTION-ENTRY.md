### TD-OILS-DEBUG-TRAP-VERDICT-AT-FUNCTION-ENTRY. The extdebug verdict is ignored at the function-entry DEBUG firing — ✅ RESOLVED 2026-08-01

**Where:** `userspace/oils/src/interp.rs` — `call_function`'s extra entry-time
DEBUG firing, and the extra one before a RETURN trap's action, in both
`call_function` and the `source` builtin.

**What:** under functrace bash announces a function call twice — once for the
call word from the call site, once on entry to the body, both with
`$BASH_COMMAND` still the call word. Refusing the *second* one behaves unlike
refusing the first:

- the body does not run, **and neither does the RETURN trap**;
- the call's status is the **handler's own** status, not 0 (bash leaves
  `execute_function`'s `result` at the trap's value), so a handler returning 1
  makes the call return 1 — where a refused *body* command leaves 0;
- a handler returning 2 at the entry firing returns from the function being
  entered, with status 2 (its frame is already pushed).

A *third* firing appears only when a RETURN trap is actually set: bash
announces once more immediately before running the RETURN action, with
`$BASH_COMMAND` still the body's last command. Refusing that one takes the
action away and nothing else — the function has already run and its status
stands.

A sourced script has no separate entry firing (`. script` is announced once,
from the call site), but it does get the pre-RETURN one — and there
`$BASH_COMMAND` reads back as the `. script` word, because bash saves and
restores the variable around the re-read where a function body's last
announcement is left standing.

Measured with a counting handler (`n=$((n+1)); [ "$n" != "$K" ]`) so the two
firings for the same `$BASH_COMMAND` can be told apart.

**Fixed by** routing both extra firings through `Shell::fire_debug_trap` and
reading the `DebugVerdict` — whose `Skip` already carries the handler's status,
which the two sites use differently (`exec_simple` forces 0, `call_function`
keeps it) — suppressing the RETURN trap on a refused entry, and snapshotting
`$BASH_COMMAND` before the sourced body so the pre-RETURN firing can restore it.

**Deliberately not replicated:** a `return` (status 2) verdict at the
*pre-RETURN* firing makes bash re-announce, unboundedly — a handler that
answers 2 every time livelocks bash 5.2.37 outright. osh treats that verdict as
"run the action anyway", so it diverges from bash only by the retry
announcements, and only for a handler that answers 2 at exactly that firing.
Reproducing a hang is not a fidelity win.

**Impact (before the fix).** Only reachable with a DEBUG handler whose status
varies between two firings of the same command, i.e. a real step-debugger.
Ordinary "refuse this command" handlers are refused at the call site first,
where the verdict was already honoured.

Covered by `tests/corpus/debug-trap-verdict-at-function-entry.sh`.
