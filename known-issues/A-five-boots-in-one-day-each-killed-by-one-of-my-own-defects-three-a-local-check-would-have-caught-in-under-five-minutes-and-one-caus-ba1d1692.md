### [A] Five boots in one day, each killed by one of my own defects -- three a local check would have caught in under five minutes, and one caused BY running those checks during the boot -- 2026-09-21
**Status:** OPEN as a workflow note. No code fix; the remedy is an order of operations.

**In short:** a full boot test is ~2 hours and cannot be shortened -- there is no
flag to skip the gates or the 35-script test suite, by design. On 2026-09-21 I
started five of them. Four died on a defect of mine, and each defect was
cheaper to find than the boot was to run.

| boot | killed by | what would have caught it | cost of that |
|---|---|---|---|
| 1 | `check-selftest-skips`: 6 findings, all mine -- rungs skipping on `.is_err()` of the code under test | `python scripts/check-selftest-skips.py` | ~40 s |
| 2 | `script-index`: I added `selftest-boot-gate-identity.py` and never indexed it | `python scripts/gen-script-index.py` | instant, and it prints the fix |
| 3 | clippy gate: 4 `clippy::all` errors in code written after my last clippy run | `cargo clippy -p kernel` | ~4 min |
| 4 | a dispatch rung asserting `PermissionDenied` where kernel context can only answer `NoSuchProcess` | reading the two rungs either side of mine, which say so about themselves | ~1 min |

**The order of operations that would have saved ~6 hours:**

1. `cargo check`, then **`cargo clippy -p kernel`** -- not after the boot, and
   not once per session. Boot 3 died because my earlier clippy runs were clean
   and I carried that verdict onto code that did not exist when they ran.
2. Run the check scripts **for the areas touched**. They are individually
   runnable and most cost under a minute. A new script means
   `gen-script-index.py`; a new self-test means `check-selftest-skips.py` and
   `check-tested-but-uncalled.py`.
3. For a new **self-test assertion**, read the neighbouring rungs first. Boot 4's
   defect was an expectation that cannot hold in kernel context, and the rungs
   immediately above it announce the correct pattern in their own output.

**What is NOT the fix.** A wrapper script running "the cheap gates" before a
boot would duplicate `boot-test.sh`'s gate list, and `check-gates-are-wired`
exists precisely to stop a second list drifting from the first. Nor should the
clippy gate move earlier in `boot-test.sh`: all three lanes boot through that
file, and reordering shared machinery to compensate for a step one lane skipped
is the band-aid `CLAUDE.md` warns about.

**RETRACTION, written an hour later: boot 5 was not my concurrent load, and
the paragraph below is wrong.** Boot 6 failed at the same gate with **nothing
running alongside it**. The real cause, found by reading
`scripts/bashprobe.py`:

> `WSL = ["wsl", "-d", "Ubuntu", "--", "bash", "-s"]`

The probe does not use the MSYS `bash` on PATH -- it drives **WSL** bash and
feeds the script on stdin. "bash exited 0, stdout empty" is WSL answering with
nothing, which happens when the distro is idling down or starting up, and
`boot-test.sh` itself uses WSL elsewhere (the rootfs build). That fits the
evidence my load theory did not: boots 1-4 cleared this gate, 5 and 6 died at
it, and the *failing case differed* between them (`$'a
b'` then `$'\$'`) --
input-independent, which a real disagreement would not be.

**Every test I ran to confirm the load theory could not have refuted it.** I ran
the checker by hand and it passed, four times, and concluded "quiet machine, so
it was load". But my hand-runs used MSYS bash for the *main* mode, and the gate
that fails is the `--self-test` (`boot-test.sh:5686`, the one invocation WITHOUT
`--may-skip`). Three separate mismatches between what I tested and what fails:
wrong flag, wrong bash, and a condition I never varied.

**RETRACTED, and the retraction is the instructive part.** The paragraph below
says the harness's `_bash_oracle_selftest_died` handler is wrong to claim *"a
self-test needs no bash"*. **The handler is right and I was wrong.** Measured
directly while WSL was returning `Catastrophic failure`:

| invocation | exit | meaning |
|---|---|---|
| `check-shellquote-vs-bash.py --self-test` | **0**, 82/82 pass | genuinely needs no bash |
| `check-shellquote-vs-bash.py` (main) | **1**, UTF-16 `Catastrophic failure` | needs WSL, and WSL was down |

So boots 5 and 6 died on the **main** run via `_bash_oracle_disagreed`, not on
the self-test. I had inferred the self-test was at fault because `bashprobe`
appeared in the traceback, without checking which of the two invocations
produced it -- and then "corrected" a true sentence into a false one, in this
file and in a request to lane B.

I also staged an applier to rewrite that handler's wording. It has been deleted
unapplied. Had it landed, a correct warning would now read as two causes when
it has one.

**What IS true, and is the part worth keeping:** `--may-skip` on the main run
does not help, because a skip requires the checker to exit **2**
(`bashprobe`'s own convention for `NoBash`). A *broken* WSL -- present but
answering garbage -- raises `ProbeError` and exits **1**, which is a finding,
not a skip. Absent WSL skips; sick WSL fails the build. That distinction is
exactly what the lane B request asks for, and that ask stands.

**Superseded paragraph:**

**One more thing the harness believes that is no longer true.** Its dedicated
handler, `_bash_oracle_selftest_died`, says: *"This is not a WSL problem and
skipping it would be wrong: a self-test needs no bash."* The self-test now
reaches `bashprobe` and does need WSL, so the reasoning that makes this gate
mandatory no longer holds -- a WSL hiccup is currently a hard stop on every
boot this lane attempts.

**WSL was healthy when checked** (3/3 direct calls, plus `bash -s` on stdin), so
this is intermittent rather than broken, and a retry is a legitimate response
while the real fix is decided.

**Superseded paragraph, kept because the retraction is the point:**

**A fifth boot, and this one I killed directly.** Boot 5 died at gate 50 on
`check-shellquote-vs-bash`: *"THE WORD PROBE IS BROKEN -- every result below
would be a lie"*, on the line `$'a
b'`. Re-run on a quiet machine: **0
failures, exit 0.**

I was running six gate scripts concurrently with the boot -- to verify a change
I wanted to fold into it, in order to *save* a boot -- and the probe could not
spawn `bash` under that load. So the rule from the table above needs its other
half:

> Run the cheap checks **before** a boot. Run **nothing** during one.

**It also includes `git commit`, which I learned by killing boot 7 with the very
commit that recorded the rule below.** Boot 7 started at 01:27. I committed at
01:35 and again at 02:07. It died reporting *"2 tooling test suite(s) failed:
test-boot-test.py, test-checkers-honour-head.py"* -- and those two are precisely
the suites that reason about **git HEAD**:

| suite | a rung it carries |
|---|---|
| `test-boot-test.py` | *"a **staged** source edit is dirty too (the diff is against HEAD, not the index)"* |
| `test-checkers-honour-head.py` | HEAD-relative by name; 898 s, 36% of the suite |

They build fixtures and compare the working tree against HEAD. Move HEAD
underneath them and their expectations are measuring a tree that no longer
exists. Both pass standalone -- I re-ran `test-boot-test.py` immediately after
and got exit 0 -- which is the same misleading signal as gate 50: **the thing
only fails when something else is happening, so reproducing it alone proves
nothing.**

So the rule is not "do not run heavy things during a boot". It is:

> **During a boot, change nothing and run nothing.** Not source, not tracked
> documents, not HEAD. A boot reads the tree it is testing for two hours, and
> three separate gates check that tree against HEAD.

The markdown edits felt safe because they are not compiled. Compilation was
never the mechanism -- `git` was. I had even reasoned explicitly that "markdown
edits are safe (not compiled)" and committed on that basis, twice.

**And that includes `git push`, which is not obvious.** A push feels like a git
operation, not a consumer of anything a boot needs. But `scripts/hooks/pre-push`
mentions `wsl` **15 times**, so pushing while a boot is running puts two
processes on the same WSL distro -- and an empty WSL answer is what surfaces as
`ProbeError` at gate 50 and refuses the build. With 20 commits unpushed and a
boot 85 gates in, the arithmetic still favours waiting: the commits are on disk
and only a machine failure loses them, while a push that kills the boot costs two
hours for certain.

The probe's message is worth knowing too, because it cost me the diagnosis
before it cost me the boot. It reports a `None` result as *"bash itself
rejected the line"* and says a framing failure would have raised `ProbeError`
instead -- but a `bash` that cannot fork looks exactly like a `bash` that
refused the syntax. I went to lane B's `$'\c'` request first, which was
already DONE and had nothing to do with it. **A probe that cannot distinguish
"the subject said no" from "the subject never ran" reports the wrong cause
with full confidence** -- the same defect class as reading exit 0 as a warning
count, one layer down in someone else's tool.

**The one thing worth measuring next time.** Every one of the four was found by
a gate that already existed. None was a gap in coverage -- they were gaps in
*when I chose to run* the coverage. That is a scheduling defect, not a testing
one, and it has the cheapest possible fix.
