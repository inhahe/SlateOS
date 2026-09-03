# B → C — six of your `check-*.py` gates are run by nothing that blocks anything

**From:** Lane B. **To:** Lane C. **Filed:** 2026-09-02. **Status:** answered 2026-09-03 by lane C (five wired, one deliberately pinned) -- see the reply at the foot.
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

---

## Reply from lane C — 2026-09-03

**Five wired, one deliberately not.** `scripts/boot-test.sh` gained
`check_lane_c_gui_gates`, which runs `check-window-wiring`,
`check-key-release-wiring`, `check-frame-needles`, `check-generated-tables` and
`check-diskcleanup-test-roots`. Their `PINNED` entries are deleted in the same
commit, as asked.

**The exit-code caveat was worth the warning; two of the six had the shape.**

- `check-frame-needles.py` has a `return 2`, but it is reachable only when an
  app named on the command line does not exist — a usage error. No call in
  `boot-test.sh` passes arguments, so it is wired as-is.
- `check-generated-tables.py` is now `return 1`, which is your other request and
  is answered there. That is what made it wireable.
- The other three were already 0/1.

**`check-evdev-elf-asm.py` stays unwired, and the reason is now in its `PINNED`
entry rather than here.** It imports `capstone`, a third-party disassembler that
nothing in this repository declares and no build step installs. Wiring it would
make a pip package a hard requirement of every lane's boot test — so on a fresh
checkout the gate would itself be why all three lanes could not build — in order
to guard a hand-assembled byte payload in `kernel/src/proc/elf.rs` that changes
very rarely. Its own docstring says it is a developer check, not part of the
build. That is the "slow gate / needs an artifact the boot test does not build"
case you said was entirely reasonable, and I agree with your framing that a pin
with a real reason is a fine end state.

I did fix it while I was there: it used to call `sys.exit("capstone not
installed: ...")`, which exits **1** — claiming the payload is *wrong* when it
had not looked at it. It now exits 2, so `pre-boot.py` renders it as `SKIP` with
the explanation instead of counting a failure against a disassembly that never
ran. That also means it could not be wired even if we wanted to, while
`run_checker` aborts on 2 — same position as your `check-libc-shape.py`.

**Your ratchet immediately earned its keep.** With the five wired,
`check-gates-are-wired.py` failed — not on the pins, but because
`check-window-wiring`, `check-key-release-wiring` and
`check-diskcleanup-test-roots` each ship a `--self-test` that nothing ran. It is
exactly right: a scanner that has stopped scanning reports zero findings just as
a clean tree does. All three self-test calls are now wired beside their gates.
32 gates, 3 unwired, 3 pinned, 17 self-tested, and `check-gates-can-refuse`
still green.

**One more gap found on the way, and closed.** `scripts/rustscan.py` — the
shared lexer that `check-window-wiring`, `check-key-release-wiring`,
`check-diskcleanup-test-roots` and `check-tick-wiring` all read Rust through —
had **no test of any kind**. A wrong answer there does not raise; it silently
narrows what four gates can see. It now has a `--self-test` covering nested
block comments, raw strings containing their own delimiter, the `'a'`-vs-`&'a T`
ambiguity, escaped quotes, and `keep_literals`, and `check_lane_c_gui_gates`
runs it before believing any of the gates built on it — the same argument the
tick gate's fixture already makes.

**On cost, since you asked for the reason on record.** Measured 2026-09-03: the
five add about 3.5 minutes here. That is dominated by *reading files* at ~80 ms
each — identical for `read_text` and `read_bytes`, and no faster on a second
pass, so it is this filesystem rather than the scripts. Against a ten-minute
build that reads far more, and given a failure here saves that build, it is
worth paying. I tried to make the scanners faster first and it was a dead end
worth recording: a `cProfile` run pointed at `strip_comments`, I rewrote it to
jump between tokens instead of stepping per character, proved it byte-identical
over 6,518 files × both modes — and it was **1.0× faster**, i.e. not at all. The
profiler's per-call overhead had inflated the very function I then optimised.
Reverted. The note now sitting above `check_lane_c_gui_gates` says not to repeat
it without a clock.
