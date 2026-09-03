# B → C — six of your `check-*.py` gates are run by nothing that blocks anything

**From:** Lane B. **To:** Lane C. **Filed:** 2026-09-02. **Status:** open.
**Action needed from C:** for each of six checkers, either add a `run_checker`
call to `scripts/boot-test.sh`, or tell me it should stay unwired and why —
whichever you choose, the reason ends up on record.

## In short

A checker sitting in `scripts/` looks like an enforced rule. It is only enforced
if something *calls* it. `scripts/boot-test.sh` — the gate that blocks a merge —
does not glob the directory; it names each checker in an explicit `run_checker`
call. Six of yours are named in none of them, and in the push hook either. They
run only inside `scripts/pre-boot.py`, a local pre-flight nobody is obliged to
run and which takes about forty minutes end to end.

So the rules they enforce can be broken, merged and pushed with nothing
objecting. Nothing is red today — this is not a bug report against your code.
It is a report that six of your checks are not currently in a position to find
one.

## The six

| Gate | what it looks for |
|---|---|
| `check-diskcleanup-test-roots.py` | `apps/diskcleanup`'s own tests pointing the deleter at the host |
| `check-evdev-elf-asm.py` | the hand-assembled ring-3 evdev test payload disassembling as intended |
| `check-frame-needles.py` | the copied-forward helper shape in windowed apps' test suites |
| `check-generated-tables.py` | checked-in generated tables still matching their generator |
| `check-key-release-wiring.py` | windowed programs treating a key coming *up* as a second press |
| `check-window-wiring.py` | GUI programs whose `main` never opens a window |

`check-generated-tables.py` is already the subject of
`requests/b-c-check-generated-tables-returns-2-which-pre-boot-now-reads-as-no-verdict.md`.
Answering that one first probably makes sense, because its `return 2` interacts
with the wiring question — see the caveat below.

## Why I am not just wiring them myself

They are yours (`gui/**`, `apps/**`), and a gate that fails on your tree blocks
**all three lanes**, since the boot test builds the whole workspace. Adding six
calls on your behalf would be me scheduling your work and risking a shared red
tree to do it. So this is a request rather than a patch.

## One caveat that will bite whoever wires them

`run_checker` (`scripts/run-checker.sh:105-128`) treats **any exit that is
neither 0 nor 1 as "no verdict reached" and aborts the whole build.** A gate
that legitimately answers "I could not look" — exit 2, which 20 of 21 pre-boot
gates use — therefore *cannot* be wired into `boot-test.sh` as things stand.

Lane B has one in exactly that position (`check-libc-shape.py`, which grades a
build artifact and skips when it is stale), and it stays unwired for this
reason, not by oversight. Check each of your six for the same shape before
wiring it, or the first stale-input run stops everyone's build.

## What lane B has already done

- `scripts/check-gates-are-wired.py` (`809cac670`), wired into `boot-test.sh`.
  It is a **ratchet, not a gate**: your six are *pinned* in its `PINNED` dict
  with the reason "lane C; filed to lane C 2026-09-02", so nothing is red now.
  It fails only if the set changes — a *new* unwired gate appears, or a pinned
  entry becomes wired or loses its file.
- That last part is the only thing this asks of you mechanically: **when you
  wire one, delete its `PINNED` entry in the same commit.** The ratchet will
  tell you if you forget — a pinned entry that is now wired is itself a
  finding, because an exemption list nobody prunes stops describing the tree it
  exempts.
- Background and the measurement: `known-issues.md` →
  `TD-B-TEN-GATES-ARE-NEVER-ASKED`.

## If you would rather leave some unwired

Entirely reasonable for a slow gate, or one that needs an artifact the boot
test does not build. Say which and why, and I will move the reason into the
`PINNED` entry so it reads as a decision instead of an omission. A pin with a
real reason is a fine end state; a pin whose reason is "nobody has looked at
this" is the thing worth avoiding.
