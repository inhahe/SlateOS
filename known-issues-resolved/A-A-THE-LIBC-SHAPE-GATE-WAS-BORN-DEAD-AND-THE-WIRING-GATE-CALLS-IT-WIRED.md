## A-A-THE-LIBC-SHAPE-GATE-WAS-BORN-DEAD-AND-THE-WIRING-GATE-CALLS-IT-WIRED (lane A, 2026-09-04)

**Status: CLOSED 2026-09-10 by lane A — both halves, and both verified by
experiment rather than by reading.**

*Half one, the specific call.* `find_python` no longer exists anywhere in
`scripts/boot-test.sh` except inside the comment that records this entry:
`check_libc_shape()` resolves its own interpreter the way every other gate
function does. The gate runs; the boot test of 2026-09-10 reports
`check-libc-shape` reaching a verdict.

*Half two, the class, which is the half worth closing.* The complaint was
that a gate could fail **before** reaching its `run_checker` call and the
wiring meta-gate would still count it as wired, because it reads the call
site textually. `scripts/check-shell-callables.py` now closes that, and it
was checked by planting the defect rather than by trusting the description:
a `py="$(totally_invented_helper)" || return 0` inserted into a live gate
function makes it exit 1 with

    scripts/boot-test.sh:4167: calls `totally_invented_helper`, which is not
    defined in this file, in anything it sources, as a shell builtin, or on PATH

so the exact shape that was invisible for a week is now named with its file,
its line and its symbol. The planted line was reverted and the tree verified
clean afterwards.

*What still is not covered, so this closure is not read as wider than it is.*
`check-shell-callables` resolves **literal** command-substitution callees. A
gate that fails before its checker for some other reason — an `if` that is
never true, a `return 0` on a path nobody expected — is still invisible, and
there are now three gates in this family rather than one: can it refuse
(`check-gates-can-refuse`), does anything ask it (`check-gates-are-wired`),
and does the asking reach it (`check-gate-call-sites`, added today for the
argument half of the same question). None of them asks whether the shell
function's body reaches its own `run_checker` line on a normal run.

*Why it sat open.* Nothing was wrong with the fix; the entry simply was not
marked when the work landed, and a tech-debt list with fixed entries in it
trains its reader to assume every entry is stale. Found while looking for
lane A work, by checking whether the defect it describes is still live —
which is the cheapest thing to do first with any entry this old.

**In short:** a gate added yesterday to check that `libc.a` is carved finely
enough has **never run, not once, on any host**. Its first line calls a helper
function that does not exist; the call fails, and the `|| return 0` on that same
line turns the failure into "this gate passed". Nobody noticed because the
meta-gate whose whole job is "is every gate actually run by something?" reads
the call site *textually*, sees the gate named there, and counts it as wired.
Found by reading a boot-test log line I had previously skimmed as noise.

**The line.** `scripts/boot-test.sh:4232`, the first statement of
`check_libc_shape()`:

```sh
check_libc_shape() {
    local py=""
    py="$(find_python)" || return 0        # <-- find_python does not exist
```

`find_python` is defined **nowhere**. Verified three ways rather than assumed,
because I published an unverified claim earlier today and had to retract it:

| question | answer |
|---|---|
| defined in `boot-test.sh`? | no — `grep -n 'find_python' scripts/boot-test.sh` returns line 4232 and nothing else |
| defined in anything it sources? | no — the only `.` is `run-checker.sh` (`:1259`), which defines `run_checker` and nothing else |
| an external on `PATH`? | no — `command -v find_python` is empty |
| across all of `scripts/`? | one hit, the call itself |

And the resulting control flow, demonstrated rather than reasoned about:

```console
$ bash -c 'f() { local py=""; py="$(find_python)" || return 0; echo "REACHED THE GATE"; }; f; echo "returned $?"'
environment: line 1: find_python: command not found
returned 0
```

The body is never reached and the function reports success.

**What that silently disabled — both halves, and the more important one second.**

1. the real gate, `check-libc-shape.py --ignore-age`;
2. **its self-test**, which the function's own header singles out as
   non-skippable: *"it builds its own `ar` archives in memory and needs no
   sysroot at all, so on a machine with no `libc.a` it is the only thing still
   checking that this gate can tell a bad archive from a good one."*

**Why the symptom is invisible in practice.** It does print, on every run:

```
/tmp/boot-test-snapshot.o9Qr30: line 4232: find_python: command not found
```

One stderr line, sandwiched between two `=== … ===` banners, naming a temp file
rather than `scripts/boot-test.sh` (the harness re-executes from a snapshot), in
a log that is tens of thousands of lines long. It carries no `ERROR`, no
`WARNING`, and does not change the exit status. I had already read past it once.

**Introduced by the commit that was supposed to turn it on.** `e3e72d4bf`
(2026-09-03), *"wire check-libc-shape.py into the boot test, and unpin it"*.
That is the part worth dwelling on: the gate previously sat in
`check-gates-are-wired.py`'s `PINNED` map with the honest reason *"needs an
opt-in skip channel in run-checker.sh first"*. The commit removed the pin and
added a call that cannot execute — so the change traded a **tracked** exemption
for an **untracked** one. A pin says "not wired, and here is why"; a dead call
site says "wired" to every reader, human and machine. The gate is strictly worse
off than before it was "wired".

The header of the very function this happened to argues against the outcome in
so many words — *"we would have wired a gate that never answers"* — as the thing
it was carefully avoiding by passing `--ignore-age`. It got there anyway, by a
mechanism the comment was not watching.

**The blind spot this exposes, which is new.** `design-decisions.md` §907
established that *a gate is what `run_checker` runs, not what it is named*, and
widened `check-gates-are-wired.py` accordingly. This is the next term in the
same series and the current gate does not cover it:

> **A call site that exists is not a call site that executes.**

`check-gates-are-wired.py` asks "does some `run_checker` invocation name this
gate?" — a question about text. It cannot tell a reachable invocation from one
behind an unconditional early return. So it reported, in this very run:

```
38 gate(s); 1 unwired, 1 pinned; 32 self-tested; 0 self-test(s) shipped but unrun
ok -- every gate is either run by something or pinned with a reason, ...
```

`check-libc-shape` is in neither the "unwired" nor the "pinned" count. It is
counted among the wired — correct as text, false as fact, and phrased exactly as
it is phrased when it is right.

Note also that `set -e` cannot help here and neither can `bash -n`: the syntax
is valid, the failure is at runtime, and `|| return 0` explicitly swallows the
non-zero status that `set -e` would otherwise act on.

**Proper fix, two parts.**

1. **Replace line 4232 with the idiom the other ~20 gates use** — the inline
   `command -v python` / `python3` block, with an explicit
   `echo "=== libc.a shape: skipped (no python) ===" >&2` on the else arm. Note
   the current line is wrong in a *second* way that would survive merely
   defining `find_python`: `|| return 0` returns **silently**, whereas every
   other gate announces its skip. A gate that declines without saying so is the
   same defect one level down.
2. **Add a gate for the class.** Scan the shell scripts for a word in command
   position that is not defined in the file, not defined in anything it sources,
   not a shell builtin or keyword, and not on `PATH`. That is a small, decidable
   check which catches this outright, and it is the only one of the two fixes
   that protects the *next* call to a function nobody wrote. It belongs next to
   `check-gates-are-wired.py`, since it answers the half of "is this gate run?"
   that the existing gate structurally cannot.

**How big is the class? Measured, not guessed — one.** Before proposing a gate I
scanned the corpus for the defect, twice, and both attempts are worth recording
because the first failed in the way this whole file is about.

*Attempt 1, which found nothing and looked like a clean bill of health.* Match a
snake_case word in command position — start of line, or after `;` `|` `&&` `(`
— and report those neither defined, nor a builtin, nor on `PATH`. Result: 42
names, **every one a false positive** (arithmetic inside `$(( ))`, variables in
awk program bodies, C declarations in heredoc'd source: `size_t`, `pid_t`,
`got_signo`). And `find_python` **was not among them** — the pattern required
trailing whitespace after the name, but the real line is `$(find_python)"`,
where the next character is `)`. A scan written specifically to find this bug
did not find this bug, and reported "42 findings" in a tone indistinguishable
from working.

*Attempt 2, matching the defect's actual shape.* Take the **first word of a
command substitution** — `\$\(\s*([a-z_][a-z0-9_]*)`. Arithmetic cannot collide
with it, because `$((` opens with a paren rather than a word. Over **89 shell
files with 302 functions defined**: 37 raw hits, 35 of them builtins spelled as
substitutions (`$(cd …)`, `$(command -v …)`, `$(umask)`), leaving:

| finding | verdict |
|---|---|
| `scripts/boot-test.sh:4232` — `py="$(find_python)"` | **the bug** |
| `scripts/create-ext4-rootfs.sh:1522` — `CAPTURED := $(shell printf …)` | false positive — GNU **make** syntax inside a heredoc, not bash |

So the blast radius is exactly one call site, and the proposed gate has a
measured signal of 1 true finding against 1 residual false positive on today's
tree — the latter removable by not reading inside heredocs whose body is another
language. That is a gate worth writing rather than a fishing expedition. It also
settles the design: prefer the substitution-shaped rule to the command-position
one, because the narrow pattern found the defect the broad one missed. It keys
on structure bash guarantees rather than on surrounding whitespace.

**If it is never fixed:** `libc.a` member granularity is ungraded on every host
and every run. That is not a hypothetical failure mode — it is the one that
broke the GNU make port and got written up as `design-decisions.md` §339, which
is why this gate was built. And the meta-gate will go on reporting it as wired,
so the next person to ask "are all our gates running?" gets "yes" in the same
words that would be true.

### Both fixes landed — and the false-positive estimate above was wrong by 100× — 2026-09-04

Fix 1 is commit `9463dd574`: the `find_python` call is replaced by the inline
`command -v python`/`python3` block the other ~20 gates use. Its no-python arm
now *announces* the skip, which repairs a second and subtler defect that would
have survived merely defining `find_python` — a gate that declines without
saying so is indistinguishable from one that looked and found nothing. The gate
then ran for the first time in its existence: `--self-test` **24/24**, and
against the real archive `libc.a shape OK (615 members, 3249 symbols)`.

Fix 2 is commit `26545e857`: `scripts/check-shell-callables.py`, wired into
`boot-test.sh` immediately after `check_eol`. It takes the substitution-shaped
rule this entry recommended, resolving each callee against the file's own
functions, the functions of anything it `source`s, the shell builtins, and
`PATH` — the last asked of **bash** in one batched `command -v`, because these
scripts run under MSYS bash, whose `PATH` holds the unix tools that the Windows
`PATH` this interpreter sees does not. Verified end-to-end by re-planting the
original line: the gate names `scripts/boot-test.sh:4250`, reports one finding,
exits 1. Then restored.

**The half of the estimate that held.** "Blast radius exactly one" was right.
Across the graded corpus the finished gate reports exactly one true defect, the
one above, and today reports zero because it is fixed.

**The half that did not.** This entry predicted "1 residual false positive,
removable by not reading inside heredocs whose body is another language". The
first working implementation produced **193**. None came from the rule; all came
from *masking* — deciding which bytes are shell at all:

| Cause | Findings | Why it looked like shell |
|---|---|---|
| `` \` `` escaped backquotes in double-quoted strings | ~100 | This tree's refusal messages are prose about code: ``echo "\`article_for\` picks by spelling"``. Preserving the escape makes every one a backquote substitution calling `article_for`, `picks`, `text`, `Mutex`, `thread_local`. |
| the same escapes inside *unquoted* heredoc bodies | ~70 | `pre-push`'s advisory heredocs are English paragraphs full of ``\`static FOO: Mutex<()>\`` and ``\`unwrap_or_else(…)\``. |
| env-assignment prefixes | few | `$(PYTHONIOENCODING=:replace "$py" -u "$f")` — the callee is not the first word; it is what follows the assignments, and here it is `"$py"`, correctly undecidable. |
| `<<<` here-strings and `<<` shifts in `$(( ))` | — | See below; these *hid* findings rather than creating them. |

The lesson is a specific one and it is not "estimates are hard". **A static
scanner's error budget lives in its lexer, not in its rule.** The rule was
correct as specified on the first try and never changed. Every wrong answer came
from the question "is this byte code or is it text?", and this tree is unusually
adversarial about that because its gates explain themselves in prose that quotes
code — the very habit that makes the refusals good makes the corpus hostile to
naive scanning. Anyone estimating the cost of the *next* scanner should budget
for the masker and assume the matching is free.

**And the fourth cause is this file's own subject, one level down again.** A
`<<<` here-string read as a `<<` heredoc opener made the masker consume every
line to EOF looking for a delimiter that was really a variable expansion. The
visible effect: `boot-test.sh` silently fell from 114 command substitutions to
**46**, and the gate reported a clean tree in a confident tone. There was no
error, no exception, no empty output — just less looking. That is the fifth
sighting in this file of *a gate that discovers nothing reports no failures,
which reads exactly like a pass*, and it is why the gate carries a
`CANDIDATE_FLOOR` as well as a file-count floor: the file count stayed healthy
throughout, and only the substitution count moved.

`userspace/oils/tests/corpus/` is excluded, and is the only exclusion. Its files
are inputs to a shell parser rather than programs anyone runs, so an
unresolvable callee there is routinely the fixture's whole point —
`lineno-cmdsub.sh` calls `nosuchcommand_xyz` deliberately, to pin down which
line number the diagnostic names. That exclusion is itself self-tested: the
suite asserts it stays at one entry, that it never names a `scripts/` path, and
that the directory still exists, so it cannot silently widen into a way of
switching the gate off.
