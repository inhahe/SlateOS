## A-SIX-SELF-TESTS-SKIP-ON-EVERY-BOOT-AND-HAVE-NEVER-ONCE-RUN (lane A)

**Status:** FIXED 2026-08-31 (the six instances). The *class* gate is **built**
as of the same day — see "The class gate is now built" below — but it cannot
return a verdict until ten boots have been recorded with the new field. Its
first pass named four more instances, of which **one** survived being checked
against the source; the other three were false positives and are written up
below, because the way they were false is the more useful finding.

**In short:** Six self-test cases ask the mount table whether `/` is mounted
read-write, find that it is not, record an honest SKIP, and return. They run
at boot Steps 11 and 19; the root filesystem mounts at Step 20f. So the
condition is not "sometimes false" — it is false on **every boot, always**,
and these six cases have never executed a single time. They are green in the
sense that nothing red is printed.

**The six** (names as they appear in the serial log):

| Subsystem | Case | Gate |
|---|---|---|
| `syscall` (dispatch.rs:991) | Native openat2 | `is_mounted_rw("/")` |
| `syscall` (dispatch.rs:3861) | Dispatch FS roundtrip | `is_mounted_rw("/")` |
| `syscall/linux` (linux.rs:72597) | mkdir/rmdir/unlink native-VFS round-trip | `is_mounted_rw("/")`, inline inside `self_test` |
| `syscall/linux` (linux.rs:72925) | rename native-VFS round-trip | `is_mounted_rw("/")`, inline inside `self_test` |
| `spawn` (spawn.rs:32078) | Spawn with fd_map | `is_mounted_rw("/")` |
| `spawn` (spawn.rs:32401) | take_initial_fds one-shot | `is_mounted_rw("/")` |

Reproduce against any boot log:

    grep -n "SKIP.*not mounted read-write" build/serial-test.txt

**Nothing here is dishonest, which is why nothing caught it.** Each skip is
exactly what design-decisions.md §270 asks for — a fact the test *looked up*
in the mount table rather than inferred from a failed `open`, so an open bug
cannot silently switch the section off. `check-selftest-skips.py` enforces
that honesty and passes. What no gate has a notion of is a skip that is
honest and yet **unconditional in practice**: a predicate that is a constant
at the point it is evaluated.

**The fix pattern already exists in-tree, in three places.** A case that needs
the filesystem is split into a second entry point called from `main.rs` after
Step 20f:

- `ipc::io_ring::self_test_fh()` — `main.rs:1577`, and its two cases are
  visibly *passing* in the serial log rather than skipping.
- `syscall::linux::self_test_rename_noreplace()` — `main.rs:1878`, with the
  comment "which only sees a read-only root".
- `syscall::linux::self_test_file_mmap()` — `main.rs:1934`, same shape.

So the work is mechanical: move each of the six bodies behind a
`self_test_*_fs()` entry point, call it from the post-mount block, and leave
the pre-mount `self_test()` free of the case entirely — rather than leaving a
skip behind that reports a condition which can no longer occur.

Four of the six were already standalone `fn`s and moved by relocating one
call. The two in `syscall/linux` were **inline blocks inside `pub fn
self_test()`**, which spans some 15,000 lines; they had to be lifted into
functions of their own first. That was the larger half of the work and the
reason they were listed separately above.

**What was done (2026-08-31).** Three new post-mount entry points, all called
from the `main.rs` block that already hosts `io_ring::self_test_fh`:

| New entry point | Cases moved |
|---|---|
| `syscall::dispatch::self_test_fs` | Dispatch FS roundtrip, Native openat2 |
| `proc::spawn::self_test_fs` | Spawn with fd_map, take_initial_fds one-shot |
| `syscall::linux::self_test_fs` | `test_linux_mkdir_rmdir_unlink_roundtrip`, `test_linux_rename_roundtrip` |

Three further changes fell out of the move and are worth naming, because each
was a second way the same six cases could have gone unnoticed:

- **The guards became assertions.** Each case still calls `is_mounted_rw("/")`,
  but a `false` now prints FAIL and returns `InternalError` instead of
  recording a SKIP. Before the mount, "root is not writable yet" is a fact
  about the boot stage; after it, it is a defect, and answering a defect with a
  SKIP is how these six came to spend their whole lives unrun.
- **The `Skips` recorders were deleted**, not left idle. With no case in
  `dispatch::self_test`, `spawn::self_test` or `linux::self_test` able to skip,
  a recorder there could only ever report zero — machinery that can no longer
  record anything is one more thing for a reader to mistake for coverage.
- **Two swallowed-error sub-gates became hard failures.** Inside the two
  `syscall/linux` bodies, `if Vfs::write_file(..).is_ok() { .. }` wrapped three
  assertions in the mkdir case and the whole `RENAME_NOREPLACE`/EEXIST leg in
  the rename case. A failing write dropped those assertions and the section
  still printed OK — the same defect as the outer skip, one level down and with
  no log line at all. Both now fail loudly.

The calls are FATAL rather than WARNING, matching the halves of the same
suites that still run at Steps 11 and 19 and halt the boot on failure.
Demoting them would have moved the six cases from never running to running and
not mattering.

**Then close the class, not just the instances.** Still open. Extend
`scripts/check-selftest-skips.py` (or the boot-history machinery in
`bench/boot-history.jsonl`, which already has the data) to fail when a named
skip fires on 100% of recorded boots. A skip that has never once *not* fired
is not a skip; it is a deletion with a log line.

**Why this matters beyond the six.** `test_dispatch_openat2_native` is the
test design-decisions.md §648 relies on for native `dirfd == 0` coverage, and
it is one of the six. Adding cases to a test that provably never runs is the
precise self-deception §648's own entry criticises elsewhere — so §648's
test half was blocked on this (it is now unblocked), and any future "it is
covered by a self-test" claim about these six subsystems is worth checking
against the log first.

### And the follow-up question: is the shipped code EXECUTED?

Gate 40 proves posix compiles for the target it ships to. It says nothing
about whether any test runs the shipped arms. Measured 2026-09-13:

**231 items are `#[cfg(target_os = "none")]` -- 2,397 lines, median 7.**

Most of that is thin shims and the median says so. The tail was inspected item
by item rather than counted:

| item | lines | verdict |
|---|---|---|
| `setjmp.rs` | 145 | raw `global_asm!`. Cannot run on the host at all -- it exports the symbol `setjmp`, which would collide with the host libc -- and its `JmpBuf` layout is checked by `const` assertions that compile on BOTH targets. Fine as it stands. |
| `file.rs` `tee_transfer` | 88 | **was a real gap; fixed.** See below. |
| `spawn.rs` `execl_body` | 80 | varargs-ABI handling. Not host-testable, and deliberately not forced -- see below. |
| `sys_capability.rs` `store` | 72 | has a host twin at line 338, and the twin IS tested. |
| `aio.rs` `imp` | ~30 | paired storage: a `static mut` bare metal, `thread_local!` on the host so 20,703 tests can run in parallel at all. Deliberate, and the shared logic above it is tested. |

**`tee_transfer` was the one real gap.** No host counterpart, zero test
references, and the host build returns ENOSYS from `tee` before reaching it --
so the argument validation was tested and the 88-line copy loop, with short
writes and EAGAIN in it, was not. Fixed: the three pipe operations are behind a
`TeePipes` trait, only the syscall impl is target-gated, and eleven tests drive
the loop against an in-memory fake. Two of those tests were wrong first, and
one of the corrections fixed a comment in the shipped code that described a
re-peek the loop does not do.

**`execl_body` was left alone, on purpose.** Its risky part -- the stack/heap
threshold -- was checked by hand and is correct: `argc < 64` uses the 64-slot
stack array, so 63 arguments plus the terminating NULL exactly fit, and 64
goes to the heap. Everything else in it is x86-64 SysV varargs handling, and
the host target has a different varargs ABI. A trait over the varargs cursor
would let the loop be driven by a fake, but the fake would have nothing to do
with the ABI, which is where the entire risk lives. That is testing the wrong
thing, and it is worth writing down so the analysis is not repeated.

**What actually covers the bare-metal arms** is the Path-Z ring-3 rungs: bash,
pkgconf, CPython and make all link `libc.a` and run on target every boot, and
CPython alone references 478 libc symbols. That is real coverage; it is just
not unit-test coverage, and the distinction matters when someone reads "20,703
tests" and concludes the libc is tested.

### The class gate is now built — and three of its first four findings were wrong (lane A, 2026-08-31)

**Status of the class:** the gate exists and is wired into `boot-test.sh`. It
is **not yet able to fail**, and will not be for about ten more boots; see
"the ten-boot wait" below, which is a property of the design and not an
oversight.

**What was built.**

| | |
|---|---|
| The names | `scripts/boot-history.py` → `partition_skips`, `Serial.skips` / `Serial.skips_covered`, and `skips` + `skips_covered` fields on every row of `bench/boot-history.jsonl` |
| The verdict | `scripts/check-boot-skips.py` — fails when a named skip has fired on 100% of the last N ≥ 10 qualifying boots |
| The gate | `scripts/boot-test.sh` → `check_boot_skips`, run before the build alongside the design-decisions band check |
| The tests | `scripts/test-check-boot-skips.py`, 32 cases |
| Rationale | `design-decisions.md` §651 |

**The first version recorded every SKIP line, and on the first real log that
made three of its four findings wrong.** They are worth naming individually,
because each was wrong in the same way and it is not a way that looks wrong:

| Claimed | Verdict | The line that refutes it |
|---|---|---|
| `[io_ring] File handle read/write` | **false positive** | `[io_ring]   File handle read/write (1 entry): OK` — 1045 lines later in the same boot |
| `[io_ring] Positioned I/O (pread/pwrite)` | **false positive** | `[io_ring]   Positioned I/O (pread/pwrite preserve the cursor): OK` |
| `[mm] Zero-on-free` | **false positive** | `[mm]   Zero-on-free: OK (counter=2, settled=false)` — 46,883 lines later |
| `[mm] Zeroed frame allocation` | **genuine** | nothing; it really never runs — **FIXED 2026-08-31, see below** |

#### The one genuine finding, fixed (lane A, 2026-08-31)

`test_zeroed_alloc`'s only caller was `mm::frame::self_test()`, which runs at
`main.rs:654`; `page_table::init` is at `main.rs:774`. So its `hhdm()` guard
was not "sometimes false" but false on **every** boot, and the case had never
executed once.

**What was untested is not a formality.** `alloc_frame_zeroed` is the primitive
that stops a new owner reading the previous owner's bytes — `virtio/gpu.rs`
states it outright ("anything but `alloc_frame_zeroed` leaks"). Nothing else in
a booting kernel asserted that it returns zeros. `test_zero_on_free` is *not*
that assertion: it exercises zeroing at **free** time, behind a sysctl it
toggles on and back off — a different mechanism on a different code path. So an
information-disclosure boundary had no live test at all.

**The comment is why nobody looked.** The skip said the case "will be exercised
indirectly by the demand paging self-test". There is no such assertion; no
demand-paging test checks the contents of a freshly allocated frame. A skip
carrying a plausible coverage claim reads as a *decision* rather than a hole,
so it survives every review that a bare skip would not. The claim had evidently
never been checked against the tree. **Treat an unverified coverage claim in a
comment as the more dangerous half of a skip** — the skip is visible in the
log, the claim is not.

**What was done.**

| | |
|---|---|
| Post-init entry point | `pub fn test_zeroed_alloc()`, called from `main.rs` in the same block as `test_zero_on_free` |
| Shared body | `test_zeroed_alloc_inner(&mut skips)` — still called early, still skips, kept **as the tripwire** (`partition_skips` marks it *covered* while the real call happens, *uncovered* if that call is ever deleted) |
| The guard became an assertion | new `require_hhdm_post_init()`: before `page_table::init` an absent HHDM is a boot stage and a SKIP is honest; after it, it is a defect |
| Diagnostics | the failure now names the **offset and value** of the first non-zero byte, not just "not zero" |

**A second instance of the same hole, four lines away.** `test_zero_on_free`'s
post-init wrapper reused its shared body unchanged, so it too could record a
silent SKIP for the one reason a post-init entry point cannot legitimately
have. It now asserts the HHDM first. The lesson generalises: **splitting a test
into an early and a late entry point is only half the fix — the late one must
also stop treating the prerequisite as optional**, or it inherits exactly the
hole the split was meant to close.

**Why the failure message contains the words "self-test failed".** The harness's
`check_selftest_failures` greps case-insensitively for that wrapper phrase, and
it is the only thing that catches a self-test which reports failure and lets
the boot continue to `BOOT_OK`. It deliberately does not grep raw
`FAIL:`/`WARNING:`, because passing logs contain both legitimately
(`[drm-atomic] check FAIL: CRTC 9999 not found`). A failure message without the
phrase would print on a red boot and still let the run be recorded PASSED.

**The `io_ring` pair is a deliberate tripwire, and the source says so.** The
comment above the pre-mount calls (`kernel/src/ipc/io_ring.rs:1290`):

> The two file-handle sections cannot run here — this entry point is called
> before /tmp exists — so their skips are expected. They are still reported,
> because "expected" and "invisible" are different things: `self_test_fh` below
> re-runs them once /tmp is mounted, and if *that* call ever stopped happening
> the only evidence would be these two lines never being followed by an OK.

So the gate as first written would have produced three **permanent** false
positives — the exact failure `scripts/test-ki-dupes.py`'s docstring warns
about ("a permanent false positive is worse than no check") — and the fix its
own message invites, *move or delete the pre-mount call*, would have destroyed
the tripwire that catches the case it was worried about.

**The fix is not a suppression, it is the tripwire's own condition.**
`partition_skips` splits each boot's skips into `uncovered` (no other line
under that tag reports the section as having run) and `covered` (it ran
elsewhere in the same boot). Only `uncovered` reaches the gate. "The only
evidence would be these two lines never being followed by an OK" is *exactly*
the condition being computed, so the day `self_test_fh` stops being called, the
pair moves from `covered` to `uncovered` and starts accumulating against the
gate by itself. The tripwire got a bell.

Three matcher rules, each earned against the live log and each a bug that was
present before it:

- **Match the section only up to its first `(`.** The kernel spells a section's
  parentheses differently between the skip and the result — `SKIP: Positioned
  I/O (pread/pwrite)` vs `Positioned I/O (pread/pwrite preserve the cursor):
  OK`. Whole-string matching found nothing and called a section dead two lines
  from its sibling's proof. A `_MIN_COVER_KEY` floor keeps a short key like
  `RX` from matching half a driver's output.
- **Reject any line mentioning "skip" in any spelling.** `[hotplug]
  Single-CPU: skipping offline/online cycle` and `[iso9660]   No ISO 9660
  filesystem mounted — skipping integration test.` narrate the skip in prose on
  the line *above* the machine-readable one. Read as ordinary lines they say the
  section ran; they say the opposite. Both were wrongly marked `covered`.
- **`FAIL:` counts as having run.** The question is whether the case executed.

Where the matcher errs it errs toward `uncovered`, deliberately: a wrong
`uncovered` becomes a visible accusation a human resolves with an allowlist
entry, while a wrong `covered` excuses a section from the gate forever with
nothing to see.

**One row of `bench/boot-history.jsonl` was rewritten**, the single row written
during the few hours the field held every skip. Its nine names are exactly the
partition of its own serial log, so the correction is derived, not guessed —
and leaving it would have left one row whose `skips` field meant something
different from every row after it.

**The one genuine instance is not fixed.** `[mm] Zeroed frame allocation` skips
with `HHDM is not mapped yet (running before page_table::init)`, and
`mm::frame::self_test` runs before `page_table::init` on every boot, always. It
needs a post-`page_table::init` entry point that does not exist yet — the same
shape as `io_ring::self_test_fh`, which is the pattern the six above were
modelled on. Not fixed in the same change on purpose, and for the reason §650
gives for splitting the gate off from the six: a gate landed together with the
failures it finds is a gate whose first act is to be worked around.

**The ten-boot wait, and why it is not a defect.** No row in
`bench/boot-history.jsonl` written before today carries a `skips` field, and a
row that predates the field is deliberately *not* counted — treating "this row
does not say" as "this skip did not fire" would break the 100% streak of every
genuine offender, which is failure in the direction that hides the bug. So the
gate reports `1 qualifying boot(s) recorded, need 10 … -- no verdict` and exits
0 until ten green boots have accumulated. It prints the count rather than
staying silent, because silence reads as "checked, nothing found".

Backfilling was considered and is not possible: the historical rows have no
serial log retained, so the names cannot be recovered. Lowering the floor was
considered and rejected — a skip that fired on both of the last two boots is
not evidence of anything, and a gate that is wrong most of the time is a gate
that gets bypassed.

**The allowlist is the part most likely to rot.** Five skips fire on every boot
*on this host* and are not defects — `[pcid] live alloc_pcid tests` wants a CPU
feature QEMU does not expose, `[hotplug] offline/online cycle` wants a second
core, and so on. From the log they are indistinguishable from the six, which is
exactly the problem, so `ALLOWED` in `check-boot-skips.py` carries each one
with the **observable condition that would stop it firing** ("boot with `-smp
2`"). Two properties keep it honest: an allowlisted skip that is still firing is
printed on every run, and an allowlisted skip that has *stopped* firing is a
hard failure rather than a shrug — either the entry is now a false statement
about the tree, or the section was renamed and the entry names nothing.

One of the five is worth a second look when the gate goes live:
`[smep_smap] STAC/CLAC, with_user_access and the access counter` skips with
"SMAP not available on this CPU", yet the harness boots QEMU with
`-cpu qemu64,+smep,+smap,+umip`. Either the feature is requested and not
delivered, or the detection is wrong. It is allowlisted for now on the reading
that took the skip's word for it; if the detection is what is broken, this is a
security self-test that has never run, and the allowlist entry is hiding it.
That is the failure mode an allowlist has, stated here so the next reader
checks rather than trusts.

### Lesson 81 addendum: the inventory is 55 sites in 20 apps, not seven (lane C, 2026-08-30)

**In short:** Lesson 81's "where else to look" said "the seven `is_some()`-only
visibility assertions". Counted rather than recalled, there are **55** of them
across **20** apps. The number matters because a backlog of seven is something
you fix in passing and a backlog of 55 is a task that has to be scheduled, and
because "seven" was a guess written from memory while the lesson was fresh --
exactly the kind of unmeasured number this campaign keeps finding inside the
production code it audits.

    grep -rn 'rect_of\(_sized\)\?(.*)\.is_some()' apps/*/src/main.rs

| app | sites | | app | sites |
|---|---|---|---|---|
| snippets | 8 | | taskscheduler | 2 |
| pdfviewer | 6 | | mahjong | 2 |
| netmanager | 6 | | credmanager | 2 |
| sokoban | 5 | | charmap | 2 |
| vpnmanager | 4 | | calculator | 2 |
| diskanalyzer | 4 | | breakout | 2 |
| stickynotes | 3 | | worldclock, tictactoe, nim, memory, defrag, checkers, calendar | 1 each |

**Not all 55 are faults, and the audit is per-site.** The form is only wrong
when the test's *claim* is that something is visible. Where the claim is that
something is **clickable** -- and especially where it is paired with an
`is_none()` for a target that should have been dropped, as in `apps/calculator`'s
scrolled-away history row or `apps/charmap`'s "the Copy button goes but the grid
stays" -- a recorded box is exactly the right evidence and no paint assertion is
owed. `apps/mahjong`'s two sites are of a third kind: each sits on the line
after a `text_saying(&f, ...)` that has already established the paint, so the
`is_some()` is a separate and legitimate claim about the hit box. Read the test
name before touching the assertion; the fix for a genuine one is Lesson 83's
containment-the-other-way (a paint command that *fits inside* the box), not a
point-containment check, which the background fill makes vacuous.

### Lesson 86: a two-window comparison cannot separate two formulas that both scale with the window (lane C, 2026-08-30)

**In short:** the standard test for "this measurement is derived from the window
rather than hard-coded" is to draw the app at two sizes and require the
measurement to differ. That test is only as good as the *difference* between
the right formula and the wrong one. If the wrong formula also grows with the
window — or worse, if it produces the very same answer at both sizes tried —
the comparison passes and proves nothing. Mahjong's sweep produced five of
these in one run.

**The three shapes it takes.**

1. **The wrong formula grows too, for a different reason.**
   `the_legend_column_widens_with_the_font_it_is_drawn_at` compared a 1400x420
   window with a 1400x1400 one. The column is `widest + font + pad * 3`, and
   between those two heights the *padding* grows from 6.3 to 14 — so the column
   widened by the padding alone, and a mutant that measured the legend's text at
   a fixed 12pt was invisible. The fix is to hold the confounder still and say
   so in the test: both windows are now past the padding's ceiling, and the
   fixture asserts `short.pad == tall.pad` before it compares widths.

2. **The two sizes chosen straddle nothing.** `the_small_font_never_outgrows_
   the_font_above_it` swept a fixture whose tallest window is 1000 pixels.
   `status` stops climbing at 783 and `small` only reaches `status`'s ceiling at
   1058, so a ceiling written as a constant instead of as `status` is wrong only
   *above* 1058 — the fixture stopped 58 pixels short of where the bug lives. A
   growth test needs a size on each side of every plateau in the function.

3. **The quantity under test is scale-invariant, so no pair of sizes can
   differ.** The arrow-key search divides every tile offset by the tile width,
   and the test for it pressed the same keys in a 700x500 window and a 1900x1300
   one and required the same tile. That comparison *can never fail*: every
   distance in the search is a ratio of two lengths that scale together, so the
   winner is the same tile at any size whether the unit is a tile or a pixel.
   What the unit actually changes is the direction *threshold* — a tenth of a
   tile, which a layer's stagger does not clear, versus a tenth of a pixel,
   which every stagger clears. The test that catches it is not about size at
   all: it is a two-tile board where the only candidate to the right is one
   layer down in the same column, and the arrow must find nothing.

**The tell.** Any test of the form "draw at size A, draw at size B, assert the
numbers differ". Before trusting it, write down what the mutant computes at A
and at B. If those two numbers also differ, the test is satisfied by the mutant.
If they are *equal to the real ones*, the quantity is scale-invariant and the
whole approach is wrong — find the observable the parameter actually controls.

### Lesson 87: a bound that restates a clamp's own range is satisfied by every constant inside it (lane C, 2026-08-30)

**In short:** if the code says `x.clamp(1.0, 4.0)` and the test says
`assert!((1.0..=4.0).contains(&x))`, the test has restated the code. It cannot
fail on `let x = 2.0;`, which is the exact bug — a hard-coded size — that the
derivation was written to remove.

Mahjong's cursor border is `(tile_w * 0.05).clamp(1.0, 4.0)`, and its test
asserted the value lay between one and four pixels and was less than half a
tile. Both are true of a flat `2.0` at every window the app is drawn at, so the
old fixed border — which vanished on a large window and swallowed the tile on a
small one — passed the test written to catch it. The bounds are worth keeping;
what had to be added is a *comparison* the constant cannot satisfy: the border
at 1600x1000 must be strictly thicker than at 360x700.

**The tell.** Read the assertion and the expression side by side. If the
assertion's constants are the expression's constants, the test is a tautology.
This is Lesson 85's one-sidedness with both sides present and still saying
nothing, and it generalises past `clamp`: a `.min(c)` invites `assert!(x <= c)`,
a `.max(c)` invites `assert!(x >= c)`, and neither is evidence of derivation.

### Lesson 88: a mutant can survive by being equivalent, and the table has to say which (lane C, 2026-08-30)

**In short:** a surviving mutation means one of two very different things —
either the tests missed a real change in behaviour, or the "change" was not one.
An equivalent mutant is not a coverage hole and cannot be fixed by writing a
test; chasing it as though it were wastes the sweep's most valuable signal.
Eight of mahjong's 25 first-run survivors were equivalent, and the honest fix is
to record *why* in the row and mutate something that can actually break.

**The kinds seen so far.**

| Kind | Example |
|---|---|
| The mutant computes the same number | `small = (h*0.017).clamp(7.0, 18.0)` where the real ceiling is `status`, whose own ceiling is 18.0 |
| The guard can never bind | `.max(0.0)` on a ratio whose inputs `inset` already guarantees non-negative; `.max(header.bottom())` on a height already capped at `h - header_h` |
| The term is zero | deleting `- min_y` when the topmost tile is layer 0 row 0 |
| One side already implies the other | `remaining() == 0` widened to `&& find_hint().is_none()`: an empty board has no hint |
| The change is a relabelling | `seed.wrapping_add(1)` permutes the seeds without merging any two, so one seed still gives one game |
| Both branches return the same thing | `_ => return EventResult::Ignored` rewritten as `_ => false`, which reaches the same `Ignored` two lines later |
| The comparison is always true | `let moved = self.cursor.tile_idx != Some(bi)` where the search has already skipped that tile |
| **Arithmetic accident** | deleting the `tile_w <= 0.0` guard: a zero unit makes every delta `0.0 / 0.0`, and NaN fails every comparison the direction test makes, so the search falls through to the same answer |

**What to do with each.** If the mutant is equivalent because the *program* has
dead code, delete the dead code — three of mahjong's did, and the removals are
the real value of the finding. If it is equivalent because the *mutation* was
badly chosen, pick a different one that expresses the same fault: mutate the
answer the guard gives rather than deleting the guard, subtract the other axis's
minimum rather than one that is zero, ignore the seed rather than shifting it.
The one thing not to do is leave the row in place and go looking for a test —
there is no test, because there is no difference.

**The last row deserves care.** The NaN case is the reason to keep a guard whose
deletion changes nothing: correctness that rests on `0.0 / 0.0` producing a
value that fails every comparison is correctness by accident, and the next edit
to the arithmetic takes it away silently.

### Lesson 81 audit closed: 15 of the 55 sites were faults, 40 were the right assertion (lane C, 2026-08-30)

**In short:** the previous addendum counted 55 `rect_of(...).is_some()` sites
across 20 apps and said the audit had to be per-site because the form is only
wrong when the test's claim is about *visibility*. Every site has now been read.
**Fifteen** were genuine — a test that said "drawn", "still there", "on screen"
or "appeared" while proving only that a click would land somewhere — and have
been given a paint assertion. **Forty** were already correct: they claim
*clickability*, and for that claim a recorded box is the evidence, not a proxy
for it.

**The fifteen that were fixed**, and the word in each that gave it away:

| app | test | the word that made it a visibility claim |
|---|---|---|
| checkers | the board's squares are reachable | "reachable" over a drawn board |
| memory | a face-down card is still a card | "still a card" |
| nim | every heap offers a row to click | "offers" |
| tictactoe | an empty cell is clickable | the empty cell draws nothing else |
| stickynotes | a note is where its own text is | "is where" |
| snippets | a twisty is drawn only where there is something to open | "is drawn" |
| snippets | a row is where its own title is drawn | "is drawn" |
| snippets | every control the toolbar draws can be clicked | "the toolbar draws" |
| sokoban | the menu scrolls the cursor into view | "into view" |
| vpnmanager | the selected row must be one the user can see | "can see" |
| charmap | the End key scrolls the last character onto the screen | "onto the screen" |
| netmanager | the switch moved but Apply never appeared | "appeared" |
| netmanager | add profile is reachable when there are no profiles yet | the fault was *nothing drawn* |
| breakout | the buttons offer every action the keys do | "has no button" |
| worldclock | a narrow window drops the UTC readout before a button | "every button is still there" |

**Three patterns account for all 40 that were right.**

1. **Paired with an `is_none()`.** The pair is the whole test: the target is
   dropped in one state and recorded in the other. Both halves are about the
   hit box, and a paint assertion on the `is_some()` half would be testing a
   different thing than the `is_none()` half denies. `pdfviewer`'s six sites,
   `netmanager`'s DNS up/down buttons, `taskscheduler`'s selection-gated
   toolbar, `credmanager`'s buttons left of the fold, `sokoban`'s menu-versus-
   warehouse buttons and `vpnmanager`'s reconnect are all this.
2. **The test name says "clickable", "reachable" or "records".**
   `diskanalyzer`'s `every_control_is_still_reachable_in_a_narrow_window` and
   `sokoban`'s `a_button_the_screen_does_not_show_is_not_clickable` name the hit
   box outright. Strengthening those would not sharpen the claim, it would
   replace it.
3. **A companion test already proves the paint.** `snippets`' four scroll-window
   sites lean on `a_row_is_where_its_own_title_is_drawn`, which — now that it is
   one of the fifteen — establishes that a row's box is where its title lands.
   Once that is proved once, the scroll tests may ask about the box alone.
   `mahjong`'s two sites sit directly after a `text_saying(...)` in the same
   test, which is the same argument in one file.

**The general shape, for the next audit of this kind.** A grep gives sites, not
faults; the fault rate here was 27%. The discriminator is a single question
asked of the test's *name and message*, not of its body: does it promise the
user can **see** this, or that a click **reaches** it? Fixing the 40 would have
been worse than leaving them — a clickability test with a paint assertion bolted
on fails for the wrong reason and teaches the next reader that the two claims
are one.

### Lesson 89: an assertion behind an `if` about the code under test can retire without failing (lane C, 2026-08-30)

**In short:** a test that computes something from the production code, then only
asserts when that something looks a certain way, stops testing the moment the
code stops looking that way -- and it stops silently, because a test that
asserts nothing passes. This is Lesson 84's shape (the expectation computed with
the function under test) moved from the *value* to the *decision to check it*.

typingtutor's `a_body_too_short_for_a_row_shows_none` was written to hold the
one interesting case in `Layout::rows_visible`: a body with no room for even one
row must answer zero, not one, because a row drawn in a body that cannot hold it
is a row drawn over the footer. It read:

```rust
let l = Layout::solve(620.0, 40.0);
if l.body.h < l.row {
    assert_eq!(l.rows_visible(), 0, ...);
}
```

At 40 px the layout gives up all three bands of chrome and hands the body 34 px
against a 20.8 px row, so `l.body.h < l.row` is false and the body has room for
a row after all. The `if` never held. The test ran, passed, and asserted nothing
whatsoever -- against the real code, and against the mutant that returns
`.max(1)`, which is precisely the bug it names in its own doc comment.

**Why it is easy to write.** The `if` looks like defensive care: "only check
this when the case actually arises." But the case is decided by the same
function being tested, at a size the test chose without checking. The author
believed 40 px was too short; nothing in the test held that belief to account.

**The fix is one line, and it is not deleting the `if`.** State the precondition
as an assertion:

```rust
let l = Layout::solve(620.0, 20.0);
assert!(l.body.h < l.row, "this test needs a body too short for a row: ...");
assert_eq!(l.rows_visible(), 0, ...);
```

Now the test fails two ways: if the answer is wrong, and if the situation it was
written for has stopped existing. The second failure is the valuable one -- it
says "your test has drifted off its case," which is information no green run can
carry.

**The tell.** Any `if`, `let ... else { return }`, `filter`, or `continue` in a
test body whose condition mentions the code under test. Each is a silent exit.
Ask of every one: what makes it certain this branch is taken? If the answer is
"the layout works out that way," assert it. If the answer is genuinely "some of
these are not applicable" -- a loop over sizes where a control does not fit at
the smallest -- then count the ones that *were* checked and assert the count is
non-zero, which is the same discipline at the level of the loop.

### Lesson 90: a test can run every assertion and still prove nothing, if its fixture never enters the regime the rule governs (lane C, 2026-08-30)

**In short:** lesson 89 was about an assertion that never ran. This is the
same damage from the opposite direction: an assertion that runs, on data
where the right answer and the wrong answer are the same. `typingtutor`'s
typing panel is supposed to scroll to follow the cursor. Its test typed a
lesson into the panel, checked after *every single keystroke* that the
cursor was still drawn, and passed -- and when the mutation sweep deleted
the scrolling outright, replacing it with "always show the top of the
text," the test went on passing. The lesson it typed was 51 characters and
the panel held about two hundred. The text fit whole, so the panel never
had to scroll, so showing the top of it *was the correct behaviour* on the
only fixture the test ever built.

**The test was not weak.** It is worth being clear about this, because the
instinct on reading "a mutant survived" is to look for a sloppy assertion.
There wasn't one:

```rust
let frame = app.frame(size.0, size.1);
let highlight = frame.commands().iter().any(|c| {
    matches!(c, RenderCommand::FillRect { color, .. }
        if *color == hex(COL_SURFACE1))
});
assert!(
    highlight,
    "after {n} characters the cursor's highlight is not drawn -- \
     the panel is not following the typist"
);
```

`COL_SURFACE1` appears in exactly one place in the whole program -- the box
drawn under the character being typed -- so the assertion is not confusable
with other paint (lesson 83's hazard, and it was avoided). It runs on every
iteration of a loop over every character in the lesson. It reads the
commands the renderer would actually emit. The fixture even *tried* to be
adversarial: it picked the longest lesson in the list rather than the first.

That is the trap. "The longest lesson" sounds like the hard case, and it is
the hard case *among the lessons* -- but the quantity that matters is not
which lesson is longest, it is whether any of them is longer than the
panel. None is. The fixture selection reasoned about the wrong axis, and
reasoning about the wrong axis feels exactly like reasoning about the right
one.

**The arithmetic nobody did.** At 300x300: `font` is 9, the panel's type is
`(font * 1.25).max(8.0)` = 11.25, a line is about 17 px, and the panel is
roughly 90 px tall -- five lines. The panel is `body.w` = 285 px wide and a
mono glyph at 11.25 px advances about 6.75 px, so a line holds about 42
characters. Five lines is about 210 characters. The longest shipped lesson
is 51. The panel could have held four of them stacked.

**Why mutation testing is the only thing that finds this.** Every other
signal says the test is fine. It is green, it asserts something specific and
true, its name describes a real rule, and reading it top to bottom raises no
question. Coverage tools call the scrolling line covered, because it *is*
executed -- it runs, computes `cursor_line.saturating_sub(lines_visible - 1)`,
and correctly returns 0 every time. Only replacing the line with `0usize` and
watching nothing go red distinguishes "this test checks the rule" from "this
test agrees with the rule by accident."

**The fix is two lines, and the second is the one that lasts.** Give the test
a fixture that overflows -- here, a lesson of its own rather than one of the
shipped ones -- and then *assert that it overflows*, in terms of what the
program actually did:

```rust
assert!(
    drawn < total,
    "the panel shows all {total} characters at once, so it never has \
     to scroll and this test cannot tell whether it would"
);
```

`drawn` is counted from the glyph commands in the frame, not computed from
the layout. That matters: a precondition derived from the code under test is
lesson 84's fault, and would go on being satisfied by a layout that had
stopped meaning what the test assumed. Counting the ink is a statement about
the picture.

**The tell, which is different from lesson 89's.** Lesson 89's tell is
syntactic -- look for a branch. This one is not visible in the test at all;
it lives in the relationship between the fixture's size and the rule's
threshold. The questions that surface it:

- **For any rule of the form "X follows/scrolls/wraps/clamps when Y exceeds
  Z"** -- does the fixture make Y exceed Z? Not "is the fixture large," but
  large *relative to the specific threshold the rule turns on*. Write the
  comparison down; if you cannot write it down, you do not know.
- **When a fixture is chosen by a superlative** (`max_by_key`, "the longest",
  "the biggest board", "the deepest tree"), ask what it is the superlative
  *of*. The maximum of a set that is entirely below the threshold is still
  below the threshold. `typingtutor` picked the longest of fourteen lessons
  that were all too short.
- **When a rule has a "nothing to do" answer** -- scroll offset 0, no wrap,
  no clamp, no elision -- that answer is what a deleted rule also returns.
  A test that only ever observes the no-op answer cannot see the deletion.
  Ask: does this fixture produce a *non-trivial* answer?

**Where else to look.** Every windowed app in `apps/` has tests of this
family -- panels that scroll, lists that clamp, text that elides, grids that
wrap -- and the fixture is usually one hardcoded window size chosen when the
test was written. The systematic check is cheap and does not need a full
sweep: for each such test, find the rule's threshold, find the fixture's
value, and confirm the fixture is on the far side. Where it is not, the test
is green today for a reason unrelated to the rule it names.

**Postscript (lane C, 2026-08-31): the fixture is often a *list*, and a list
is no safer than the single size it replaced.** `hangman`'s wiring sized its
keyboard `by_width.min(by_height)` -- the key is the smaller of what the
width can pay for and what the height can. The mutation sweep replaced that
with `by_width` alone and *nothing failed*, in a suite with nine window
sizes, a containment test that checks every band is inside the window, and
an overlap test that checks no two bands touch.

The nine sizes were 320x240, 640x480, 740x560, 1280x800 and five more, and
every one of them is roughly four-by-three. The height term only binds when
the window is much wider than it is tall, so across the whole list
`min(by_width, by_height)` *is* `by_width` -- the mutation changed no
arithmetic that any fixture performed. Worse, the two tests that look like
they would catch it cannot: a keyboard that has eaten the window is still
*inside* the window, and its keys still do not overlap each other. In a
1200x200 window the width offers 116-pixel keys and the height offers 18,
so the unmutated program draws an 18-pixel keyboard and the mutant draws one
five times the height of the window it is in -- and every assertion in the
suite is about the parts being contained and disjoint, which a keyboard that
has eaten everything else satisfies perfectly.

Two things generalise from it:

- **A list of fixtures samples one axis of variation, usually the one whose
  name is in the variable.** `SIZES` varies *size* -- and a rule about aspect
  ratio is invisible to every entry in it, because they were all chosen to
  look like windows people use. The question from the main lesson still
  applies, but it has to be asked of the *quantity the rule turns on*: here
  `w/h`, which ranged over 1.25-1.6 in a fixture that needed 6.0. When a
  rule is a `min` or a `max` of two terms, the threshold is the crossover,
  and a fixture on one side of it tests one term.
- **"Contained and disjoint" is not "laid out".** Those two properties are
  what a layout test naturally asserts and they are jointly satisfied by
  degenerate layouts -- one band taking everything, bands stacked
  left-aligned, a band pushed to zero. Each needs its own assertion about
  *proportion*: `hangman` now asserts the keyboard takes at most 36% of the
  window height, that each key row's left margin equals its right, and that
  the word row ends above the keys.

**Second postscript (lane C, 2026-08-31): a clamp has two flat ends, and a
fixture pair can sit on one of them without either value looking extreme.**
`dots`' very first wiring test -- `the_layout_follows_the_window_rather_than_
a_constant`, whose whole job is to prove the board is no longer drawn from a
constant -- solved the layout at 400x400 and at 1200x1200 and asserted the
second board was wider. It failed, and the failure message was the lesson in
one line: *"a window three times as wide drew a board 291.6 wide against
291.6."* The dot spacing is `min(fit, MAX_SPACING)` with `MAX_SPACING = 90`,
and both windows are far above the crossover, so tripling the window changed
nothing the rule computed. Retrying with 200 against 400 failed the same way
-- 400 still saturates.

Two things about it are worth keeping, and neither is in the main lesson:

- **The saturated end does not announce itself.** 400x400 is an unremarkable
  window; nothing about writing it down suggests "this is the flat part of a
  clamp." The only way to know is to compute the threshold and compare. The
  fix was to make the test say so out loud: the pair is 150x150 against
  300x300 now, and the test *asserts* both spacings are strictly below
  `MAX_SPACING` before it compares them. A fixture that has to prove it is in
  the regime cannot silently leave it when a constant is retuned later.
- **Two clamps in one layout can have disjoint interiors, and then one pair
  cannot serve both.** The same test also wanted to prove the font follows
  the window, but the font is clamped to `9..18` and is *already at its
  ceiling* by the time a window is big enough to be below the spacing cap --
  the interiors do not overlap. The test is two explicitly-guarded pairs now,
  150x150/300x300 for the board and 400x340/400x600 for the font, with a
  comment recording that no single pair can exercise both. When a layout has
  several clamped quantities, "one representative window" is not a fixture;
  it is a coincidence about which clamp happens to be loose.

The same trap bit a second dots fixture the same hour, in its milder form:
`a_window_too_small_for_a_board_draws_none_and_offers_no_lines` listed 900x60
among its "too small" windows. The program draws a real 8.65-pixel lattice
there with 1.6x1.07 hit boxes -- 900x60 is *small*, not *absent*, and the
threshold the test was named after does not exist at that size. The fixture
is 30x30, 10x10 and 0x0 now, and a comment records that 900x60 was removed
because asserting emptiness there would be asserting a threshold the program
does not have.

### Lesson 91: a needle the frame says twice cannot tell you which band said it (lane C, 2026-08-30)

**In short:** `gomoku` draws the phrase "White is thinking" in two places --
the header, beside the title, and the status band along the bottom. Its
tests asked whether the *frame* contained that phrase. So when the mutation
sweep broke the status band, collapsing its `Thinking` arm into the
`Playing` one so the bottom of the window read "Arrows move, Enter places"
while White was searching, both tests that name the status band went on
passing. The header was still saying it, and the tests could not tell the
two apart. Two tests, one of them written specifically to catch this fault,
and the mutant survived.

**The helper is the fault, not the test.** The tests read fine:

```rust
assert!(
    says(&frame, needle),
    "{phase:?} with {winner:?} winning does not say {needle:?}",
);
```

and the test is called `the_status_band_says_what_the_game_is_doing`. The
name is a claim about a band. The assertion is a claim about a frame. The
gap between the two is invisible at the call site, because the gap lives in
`says`:

```rust
fn says(frame: &Frame<Target>, needle: &str) -> bool {
    texts(frame).iter().any(|t| t.contains(needle))
}
```

That is a perfectly good helper for "the window tells the player X
somewhere," which is a real thing to want to assert -- `says(&frame,
"Moves: 0")` is exactly right, because only the panel ever prints that. It
becomes wrong the moment the string it is given is one more than one part
of the program prints, and nothing in the helper's name or signature warns
you which case you are in.

**Why the duplication is not a bug to remove.** The obvious reaction is
that a program should not say the same thing twice. But it should, here: the
header's turn indicator sits at the top where the player's eye is on the
board, and the status band is the running instruction line. During a search
both must be truthful, and the two are separately deletable -- which is
precisely what makes each one worth its own test, and precisely what a
frame-wide search cannot express. Deduplicating the *program* to make the
*test* work is fixing the wrong artifact.

**The fix is to name the band, and to name it from the layout:**

```rust
fn says_in(frame: &Frame<Target>, needle: &str, r: Rect) -> bool {
    frame.commands().iter().any(|c| {
        matches!(c, RenderCommand::Text { text, x, y, .. }
            if text.contains(needle) && r.contains(*x, *y))
    })
}
```

with the rect taken from `Layout::solve(W.0, W.1).status`. The mutant now
kills both tests. And the same call, pointed at `.header`, turns the
weaker half of the pair into a second real claim: the frame after Black's
move is now required to say "White is thinking" in *both* bands, which is
two independent facts where there was one ambiguous one.

**The tell.** This is not lesson 83 (a colour confusable with other paint)
and not lesson 81 (a hit box mistaken for ink) -- it is text confusable with
*the same text elsewhere in the same picture*, and it has its own question:

- **For every whole-frame text assertion, grep the production code for the
  needle.** If it appears in more than one drawing function, the assertion
  cannot distinguish them and any test naming one of them is misnamed. This
  is a mechanical check -- one grep per needle -- and it does not need a
  sweep to run.
- **Watch for needles that are prefixes of each other.** `says(&frame,
  "White wins")` is satisfied by the status band's "White wins. Z to take it
  back" *and* by the header's "White wins", so shortening a needle to make it
  match both bands quietly converts a specific claim into a frame-wide one.
- **When a test's name contains a noun the layout has a `Rect` for** --
  band, header, panel, sidebar, toolbar, footer -- the assertion should
  mention that `Rect`. If it does not, either the name is too specific or
  the assertion is too broad, and the sweep will tell you which.

**Where else to look -- done, and it is `scripts/check-frame-needles.py`.**
The check above is mechanical, so it was written down rather than left as
advice: for every bare `says(&frame, "X")` in a crate's test module the
script reports which production functions paint a literal containing "X",
and exits 1 when any needle has more than one painter. Run over the whole
of `apps/`, only three crates carry the helper at all -- `gomoku`,
`towers`, `wordsearch` -- and after the fix above none has an ambiguous
needle. It is worth re-running as each further app is wired, since the
helper is copied forward with the suite.

**The script was wrong three times before it was right, which is the part
worth remembering.** It was checked against the *pre-fix* `gomoku` source,
which it must flag, and did not:

1. It matched needles against whole function *bodies*. "A" is a substring
   of nearly every function in the file, so the needle the check existed to
   find was buried under two dozen spurious owners.
2. It found needles with a regex. But `says(&frame, needle)` sits inside
   `assert!(..., "message")` and its first argument is itself a call
   (`&app.frame(W.0, W.1)`), so scanning forward for a quoted string
   returned the *failure message*: it invented eleven needles for
   `wordsearch` that no test ever passes. Arguments need a depth-tracking
   scanner, not a pattern.
3. It assumed the needle is the second argument. `wordsearch` declares
   `says(a, size, needle)` -- it re-renders at a size rather than taking a
   frame -- so the script read the window size as the needle and reported
   every call unresolvable. The position is now read off the helper's own
   declaration.

The general point: **a checker that has only ever been run against code you
have already fixed has had its pass path tested and its fail path not.**
Keep a known-bad input -- here, one `git show` of the commit before the fix
-- and require the tool to fail on it. All three faults were invisible
against the repaired tree, where every version of the script printed a
confident green.

The script also now prints the needles it *cannot* resolve
(`&format!("{k}:{what}")`, `a.category().label()`, a loop variable) rather
than omitting them, because omitting them is how a partial check reads as a
complete one -- and a loop variable is the exact shape `gomoku`'s surviving
mutant hid behind.

### Lesson 92: a condition written twice has one copy no test can reach (lane C, 2026-08-30)

**In short:** `gomoku` asked "is White searching?" in two places -- once in
the event handler, to decide whether a clock tick counted as handled, and
once at the top of the function that clock tick calls. Deleting the *second*
copy changed nothing whatsoever: the mutation sweep ran the whole 78-test
suite against a `think()` with no guard at all and every test passed,
because no caller could ever reach it with the guard false. A guard that
cannot be reached is not defence in depth. It is code that looks tested and
is not.

**The two copies:**

```rust
Event::Tick { .. } if self.phase == GamePhase::Thinking => {
    self.think();
    EventResult::Consumed
}
```

```rust
fn think(&mut self) {
    if self.phase != GamePhase::Thinking {
        return;
    }
    ...
```

`think` is private and has exactly one call site. So the inner `if` is
dead in the strict sense -- no input to the program makes it taken -- while
looking exactly like the sort of defensive check a reviewer would ask for.
Coverage tools report the line as covered, because it *runs*; it just never
branches.

**Deleting the duplicate is the wrong repair.** That was the first instinct
and it is worth saying why it is wrong: the two copies are not redundant
by accident, they are two *different* jobs that happen to share a
predicate. The handler needs the answer to pick `Consumed` over `Ignored`;
`think` needs it to know whether to run. Delete the inner one and the
precondition becomes a comment; delete the outer one and the event result
is wrong. Either way the predicate is still stated once for each job.

**The repair is to make one copy the source of the other.** Have the
function that owns the precondition report what it did, and let the caller
answer from that rather than from a second evaluation of the same
question:

```rust
fn think(&mut self) -> bool {
    if self.phase != GamePhase::Thinking {
        return false;
    }
    ...
    true
}
```

```rust
// The tick is answered by whether a search actually ran, rather than by
// re-deciding here whether one should have.
Event::Tick { .. } if self.think() => EventResult::Consumed,
```

Now there is one guard, it is live, and the mutation that deletes it kills
`a_tick_with_nothing_to_think_about_is_ignored`. Note also what the
rewritten call site *stops* being able to do: the old form could return
`Consumed` for a tick during which nothing happened, because it was
reporting its own opinion rather than the outcome. Deriving the result from
the work is strictly more truthful, not merely tidier.

**The tell.** This one is findable by reading, unlike lessons 89 and 90:

- **Grep the predicate.** If the same comparison against the same field
  appears in a caller and in the callee, one of them is unreachable. Which
  one depends on the call direction, but there is always exactly one.
- **A private function with one call site should not re-check what the call
  site checked.** For a `pub` function the guard is real -- the caller is
  unknown. For a private one, the caller is right there and can be read.
- **When a guard "cannot be tested," that is the finding, not an excuse.**
  The reflex when a test for a defensive branch looks impossible to write is
  to skip the test. The right reading is that the branch is unreachable and
  the design has stated something twice.

**Where else to look -- checked, and `gomoku` was the only one.** The
pattern is generated by the `impl App` shape every windowed app shares:
`on_event` matches on the event and must return `Consumed`/`Ignored`, which
tempts a guard in the match arm, while the method it dispatches to carries
its own precondition. Fourteen apps have a guarded event arm:

```
grep -n "^            Event::[A-Za-z]* {\?.*} if " apps/*/src/main.rs
```

Twelve of them are the same guard, `Event::Key(k) if k.pressed =>`, and in
every one of the twelve the `handle_key` it dispatches to does *not* re-test
`pressed` -- the predicate is stated once, at the arm. (Several pass
`key.key` rather than the whole event, which makes the duplication
impossible to write.) The remaining two guard on state the arm body handles
inline, with no callee to duplicate it. So the fault was `gomoku`'s alone,
and the check costs two greps.

Worth keeping the check, though, for the reason the campaign exists: the
twelve are twelve *because* the `pressed` guard is copied forward with the
`impl App` block. The first app that moves it into `handle_key` without
removing it from the arm reproduces this exactly -- and will pass its whole
suite while doing so.
