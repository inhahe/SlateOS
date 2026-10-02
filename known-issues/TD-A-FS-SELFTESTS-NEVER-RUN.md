## TD-A-FS-SELFTESTS-NEVER-RUN — ~220 `kernel/src/fs` self-tests are dead code

**Lane A. Found 2026-08-23, during the §261 byte-clean conversion.**

`kernel/src/fs/` has 424 `pub fn self_test()` entry points. Only about 200
of them are invoked from `main.rs`. The rest are called from nowhere at
all — a few are reachable from a `kshell` subcommand a human would have to
type, and the remainder are reachable from nothing whatsoever. They
compile, so nothing warns; they simply never run, in the boot test or
anywhere else.

This was found the direct way: the §261 conversion added new non-UTF-8
regression tests to six modules, and after a green boot test only four of
the six markers appeared on the serial log. `fcomment` and `immutable`
were missing because their `self_test()`s had never run — `immutable`'s
covers the table that decides whether a write, truncate, delete or link is
refused, so the flag-enforcement path had no boot coverage at all. Both
are now wired into `main.rs`.

**Reproduce / enumerate:**

```bash
for f in kernel/src/fs/*.rs; do m=$(basename "$f" .rs)
  grep -q "pub fn self_test" "$f" &&
  ! grep -q "fs::$m::self_test" kernel/src/main.rs && echo "$m"
done
```

**The proper fix** is to call every one of them from the boot self-test
block in `main.rs`, in the same `if let Err(e) = … { serial_println!(…) }`
shape as the existing entries. Two things make it more than a mechanical
edit, which is why it is filed rather than done inline:

- These tests have *never executed*. Expect a substantial number to fail
  or panic on first run, and a panic in the boot self-test block halts the
  boot test rather than reporting. They should be enabled in batches, each
  batch boot-tested, with the failures fixed as they surface — that is the
  point of enabling them, but it is its own task and cannot ride along
  inside an unrelated commit.
- Boot time. These are in-memory table tests and individually fast, but
  220 of them is a real addition to a boot test that already runs ~9
  minutes.

**Why it matters beyond the missing coverage:** every one of these modules
reads as tested. A future reader — or a future session doing exactly what
this one did — sees a `self_test()` with real assertions and reasonably
concludes the module is covered. It is the same "a test that never runs is
not a test" trap the earlier locale/timezone sweep hit, at about ten times
the scale.

**Confirmed: the wiring finds real bugs, not just stale tests.** The very
first batch to be enabled panicked the boot on `fs::pinnedapps` test 6, and
the assertion was right — `reorder` was broken. It set `pin.position =
new_position` and touched nothing else, so the moved pin and the incumbent
both claimed the slot; `list_pins` sorts by position with a *stable* sort,
so the incumbent stayed first and `pinnedapps move taskbar terminal 0`
reported success while changing no order at all. It now performs a real
move and renumbers the location contiguously. `pin` had a smaller sibling
defect found in the same read: `max().unwrap_or(0) + 1` put the first pin
in an empty location at position 1, leaving slot 0 permanently vacant.
Both are fixed in `82155959a`. Note what this says about the batching
advice above — the panic is the *feature*; enable in batches precisely so
each panic points at one module.

### 2026-08-23 — the 37 that were blocked on the *other* defect

**The batch of 37 wired in `d8f43153e` could not have been wired earlier,
and the reason is worth recording because it is the same trap twice.**

These 37 were manual-only, so this entry covered them — but they were also
destructive (`TD-A-SELFTESTS-NOT-IDEMPOTENT`), each opening with
`clear_all()`. The two defects had to be fixed in a fixed order, because at
boot the tables are empty, so the wipe is a no-op and **the boot test is
green either way**. Wiring first would have produced a green boot test that
said nothing whatever about the shell path — and worse, boot coverage would
then stand as *evidence* that the suite was fine, for a suite that still
emptied the user's credential store the moment they typed `credentials
test`. `ace6cff40` removed the destruction first; only then was the wiring
honest.

The general rule this yields, for the ~197 suites still manual-only: **check
whether a suite is destructive before wiring it, not after.** A green boot
test cannot tell you, and will actively mislead you.

**Wiring 37 suites blind would have been reckless, so they were audited
first.** Two of them are named `osreset` and `installer`. The audit asked
one question of each suite — does it name anything outside its own module?
— and across all 37 there is exactly one such reference: `perfmon` reading
`crate::hpet::elapsed_ns`, a read-only clock query. `installer`'s
alarming-looking `erase_disk: true` is a field of a config struct being
recorded in an in-memory table, not a disk erase. So these are safe at boot
by construction rather than by hope. Reproduce with:

```bash
sed -n '/^fn self_test_inner/,/^}\s*$/p' kernel/src/fs/<mod>.rs |
  grep -oE "crate::[a-z_]+::[a-z_]+|fs::[a-z_]+::[a-z_]+" | sort -u
```

**The conversion broke this entry's own guard, which is the more
interesting failure.** `scripts/check-self-tests-wired.py` modelled
"reachable" as a mention followed by `(` — true only while every call in the
tree was a direct call. `with_pristine(&STATE, State::new(),
self_test_inner)` passes the suite as a function *value*, so the name is
followed by `)`, and all 53 converted suites read as reachable from nothing.
Dead count went 0 → 54 in one commit.

That is the worst failure available to a guard: not a missed defect but a
mass false alarm, which is how a guard gets `--quiet`-ed permanently and
then misses the real one. Fixed in `ae822c8b5` by splitting the question in
two — `BARE_CALL` still asks "is this a call?" for the gated-call report,
where the distinction is the whole point, while `BARE_MENTION` asks "does
this name reach a value?", which is what reachability means in Rust. The
scan also now runs over the existing comment/string blanker, because
relaxing the pattern alone would let a doc comment vouch for its own dead
suite — and these suites' doc comments discuss `self_test` by name.

**Counts after this batch:** boot-reachable 901, manual-only 197, dead 0.

**It found a bug on the first boot, as predicted — `perfmon` test 10.**
36 suites ran green and then the boot panicked with `left: 10, right: 5`.
The suite called `set_max_samples(5)` and asserted the history held 5;
`set_max_samples` clamps to a floor of 10, so it stored 10 and — returning
`()` — said nothing about having done so. The assertion had been wrong for
as long as it existed.

The interesting part is what the cause had *already* done elsewhere. A
silent clamp forces every caller that wants to report the effective value to
restate the range, and `kshell` did exactly that: `v.clamp(100, 60000)`
written out a second time next to `perfmon::set_interval(v)`, free to drift
from the real policy with nothing to catch it. So the fix was not the
assertion. Both setters now return the value stored, the ranges are named
constants, the doc comments admit a clamp happens, and `kshell` prints
`(clamped from N)` — strictly more than it could say before. Fixed in
`b86e51354`.

This is the second time this batch that the *test* was the wrong half to
fix. Worth stating as a rule: when a never-run suite finally runs and fails,
the assertion is a report, not a diagnosis — read the API it is asserting
against before changing either.

### 2026-08-23 — the 16 the first sweep could not see

**Immediately after the 37 landed, a second-opinion pass found 16 more
destructive suites of the same class.** Eleven of them are the interesting
ones: they would have read as *safe to wire* under the survey that gated the
first batch, and wiring them would have shipped eleven data-destroying
suites into boot.

`build/survey_destructive.py` classified each manual-only suite by grepping
its body for a whole-table clear. That grep knew six spellings —
`clear_all`, `reset_all`, `remove_all`, `delete_all`, `clear_history`,
`wipe_all` — and anything it did not match fell into a bucket named
`quiet`, described as "probably safe, still worth a glance." **A vocabulary
list is the wrong shape of test for "does this destroy data", because the
vocabulary is open.** Eleven suites emptied their tables under names not on
it:

| module | how it empties the table |
|---|---|
| `autofix` | `clear_resolved()` |
| `cliphistory` | `clear()` |
| `crashreport` | `clear_reports()` |
| `datausage` | `reset_usage()` |
| `dmevent` | `clear_events()` |
| `dnssettings` | `flush_cache()` |
| `hwmonitor` | `clear_alerts()` |
| `nameservice` | `flush_cache()` |
| `printmgr` | `clear_completed()` |
| `startuprepair` | `reset_failed_boots()` |
| `tracemon` | `clear_buffer()` |

The remaining five (`dumpanalyzer`, `location`, `multiclip`,
`recentsearch`, `sysresource`) *were* in the `wipes` bucket all along. They
were missed for an unrelated reason: they are lazy-init
(`static STATE: Mutex<Option<T>>`), and the converter written for the first
53 only understood the eager shape. `build/sweep_lazy.py` handles them —
their pristine value is literally `None`, which is what a fresh boot holds,
so there is no constructor to extract.

**What found the eleven was a structural property, not a longer word list.**
`build/widen_check.py` flags any call that (a) has a destructive-sounding
name and (b) **takes no arguments**. Taking no arguments is the tell: a
function that destroys *one row* needs to be told which row, so a
zero-argument destructive call acts on the whole table almost by definition.
That test does not care what the author called it. It produced exactly one
false positive across 161 modules — `colorblind::list_presets`, because
"presets" contains "reset" — which is the right error to make.

The rule, then, is not "add these eleven names to the regex." It is: **when
a check enumerates the ways something can go wrong, assume the enumeration
is short, and find a second check that keys on structure instead.** The
first survey's `quiet: 155` was a number I nearly trusted.

**A subtlety worth recording: `OPS` is a mirror, not a counter.** These
modules do `state.ops += 1; OPS.store(state.ops, Ordering::Relaxed);` —
`state.ops` lives *inside* the table, and `OPS` is a lock-free cached copy
outside it for cheap reads. `with_pristine` restores the table, and so
restores `state.ops`, but it cannot know about the mirror. Left alone, the
two disagree permanently and `<module> stats` reports the suite's activity
as the user's. Each wrapper therefore saves and restores `OPS` around the
call. The first comment written for this said "`OPS` lives outside the
table, so `with_pristine` cannot see it", which is true of the variable and
false of the value — the correction is in `1c29294f3`.

Converted in `1c29294f3`, wired in the commit that follows it.
**Counts after this batch:** boot-reachable 933, manual-only 181, dead 0.

### 2026-08-23 — "which of these are destructive?" was the wrong question all along

**Of the 181 suites still manual-only, the survey called 8 destructive. 149
of them permanently modify user state.** The gap is not a bug in the
detector. It is the detector's premise.

Both generations of the check looked for **deletion** — first by name
(`clear_all`, `reset_all`, …), then, after that missed eleven, by the
structural tell that a destructive call taking no arguments must act on the
whole table. Both are real improvements and both are still looking for the
wrong thing, because this is a self-test:

```rust
set_brightness(50);
assert_eq!(get_brightness(), 50);
```

It deletes nothing. `set_brightness` is an ordinary mutating setter called
exactly the way any caller would call it — there is no vocabulary tell and
no arity tell, and there cannot be one, because nothing distinguishes the
suite's call from a legitimate one except intent. `fs/brightness.rs` sat in
the bucket labelled *"quiet — probably safe, still worth a glance"*, and
typing `brightness test` at the kernel shell sets the user's screen
brightness four times and leaves it wherever the last assertion put it.
`colorscheme test` changes their accent colour and theme mode;
`dpiscaling test` changes their display scale. All three were `quiet`.

**The conclusion is not a fourth detector.** A self-test mutates its
module's state because mutating state is what a self-test is for; the
answer to "which of these are destructive?" is "essentially all of them",
and the two suites that genuinely are not (`net/http`, `pciids` — pure
parser tests over local values) are cheap to identify because they declare
no module state at all. `with_pristine` restores whatever the suite did —
a wipe and a settings change are the same operation to it — so **it can be
applied without knowing which, and applying it everywhere is both cheaper
and safer than any sequence of increasingly clever greps.**

Generalising the rule: *when a safety check enumerates the ways something
can go wrong, and each revision of the enumeration finds more cases than
the last, stop revising it.* The enumeration is not converging on the
answer; it is sampling an open set. Find the operation that is safe
regardless — here, restore-after — and apply it unconditionally.

**Measured shape of the remaining work** (`build/static_shapes.py`), which
is what makes "convert everything" tractable rather than heroic:

| shape | count | conversion |
|---|---|---|
| one lazy static (`Mutex<Option<T>>`) | 144 | mechanical; pristine value is `None` |
| one eager static (`Mutex<T>`) | 22 | needs a `const fn new()` extracted from the initialiser |
| two independent statics | 2 | `net/bridge`, `net/qos` — nested `with_pristine` |
| no module state at all | 13 | nothing to do; these are the genuinely inert ones |

The 13 with no state are *the same 13* the mutation scan independently
found to mutate nothing, which is the only cross-check available here and
it agrees.

Superseded scripts: `build/survey_destructive.py` (name list, `fs/` only),
`build/widen_check.py` (arity tell). `build/survey2.py` generalises both
off the hardcoded `fs/` prefix — it was written to cover the 37 suites in
`kernel/src/net/` and at the top of `kernel/src/` that the first survey
literally could not open — and `build/mutation_check.py` is the scan that
retired all of them.

---

### Resolution — confirmed 2026-09-11, by measurement rather than by memory

**They all run.** `scripts/check-self-tests-wired.py` reports **1315 self-tests
reachable from `main.rs`, 0 reachable from nothing**, with two allowlisted
exceptions (`hardlockup::self_test_fire`, `proc::spawn::self_test_ctest_pty`) and
six conditional call sites covered by five `// RAN-IF:` markers.

Checked independently against a boot log rather than trusting the gate: of the
**437** modules under `kernel/src/fs` that define a `pub fn self_test`, **435**
leave their name in the serial output of the run of 2026-09-11. The two that do
not are both explained and both run — `vfs_impl::self_test()` is called from
`fs/ext4/mod.rs:94` as an ext4 submodule, and `sevenz` prints under the tag
`[7z]`, so its module name never appears.

**A note on how nearly this went wrong, because it is the entry's own subject
one level up.** The first pass matched each module name against `self-test` with a
hyphen, while the code prints `a11y::self_test` with an underscore, and reported
**298 of 446 modules missing**. That is a false alarm large enough to have
re-opened a closed investigation, produced by one character. The corrected
measurement gives 2, both benign. An instrument that disagrees with a gate should
be suspected before the gate is.

Still open, separately: `TD-A-SELFTESTS-REACH-OUTSIDE-THEIR-OWN-MODULE` and
`TD-A-PRISTINE-STATE-CAN-BE-TOO-BIG-FOR-THE-STACK`. `with_pristine` restores one
module's table, so state living outside it is saved and restored by hand at each
site — visible at the top of `fs/a11y.rs::self_test`, which saves two atomics
`with_pristine` cannot see. The suites run; that limitation is worked around
per-site rather than fixed.
