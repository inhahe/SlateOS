## TD-C-THE-NUMERIC-KEYPAD-TYPES-NOTHING-BECAUSE-NOTHING-TRACKS-NUM-LOCK (lane C, 2026-08-24) -- FIXED 2026-08-24

**In short.** On a real SlateOS machine the number keys on the right-hand
block of the keyboard — the calculator-style pad — type nothing at all. The
main number row across the top works fine, so this is not "digits are broken",
it is "one particular set of keys is". The reason is that the keypad's keys are
double-duty (`4` or Left-arrow, `7` or Home) and which one you get depends on
the Num Lock light, which the compositor does not currently pay any attention
to. Rather than guess, those keys were left typing nothing.

**Where.** `gui/compositor/src/keymap.rs` — `text_outside_the_block`, the
fallback for scancodes no [`Layout`] describes. It answers `Some(' ')` for
`0x39` and `None` for everything else, keypad included. `ModifierState`
(same file) tracks Caps Lock as a latch but has no Num Lock equivalent, and
`ModifierState::update` ignores `0x45` entirely.

**Why not just add the digits.** The keypad reports the *same* set-1 scancodes
whichever way the latch is set — `0x4B` is keypad-4 and keypad-Left both — so a
table that unconditionally maps `0x4B` to `'4'` would type a `4` every time a
user pressed keypad-Left with Num Lock off. `key_for_scancode` already maps
those codes to `Key::Numpad*` names, and
`the_navigation_cluster_is_not_conflated_with_the_keypad` guards that the pad
and the arrow cluster stay distinct, so the `Key` half is already right; it is
only the character half that is unanswerable without the latch.

**Reproduce.** Boot the evdev backend (any bare-metal or QEMU run), focus a
text field, press keypad-1 with Num Lock on. Nothing is inserted. On the host
backend it works, because Windows hands the compositor its own character and
that branch wins before the layout is ever consulted — the same masking that
hid the space-bar bug fixed in this commit.

**Proper fix.** Give `ModifierState` a `num_lock: bool` latch alongside
`caps_lock`, toggled on press of `0x45` and ignored on release exactly as
`0x3A` is, defaulting to **on** (which is what firmware sets on essentially
every desktop keyboard, and what a user who bought a keypad expects). Then let
`key_for_layout` consult it: with the latch on, `0x47`–`0x53` type
`7 8 9 - 4 5 6 + 1 2 3 0 .`; with it off they keep answering `None` and stay
navigation keys. `0x4A`/`0x4E`/`0xE035` (`-`, `+`, `/`) and `0x37` (`*`) are
not double-duty and could type unconditionally. The decimal separator is the
one genuinely locale-dependent key — a German keypad is engraved `,` — so it
should come from the layout rather than the table, which is an argument for
`LayoutSpec` growing a `decimal_separator` field rather than for hard-coding
`'.'`.

**Severity.** Medium. It is a whole physical key block that does nothing, and
data-entry users reach for it first. Not urgent only because the top row works.

**Two more found in the same function while reading it for the fix** (2026-08-24):

* **`-n` swallows an argument it could not read.** The parse is
  `max_args = rest.get(..end).and_then(|s| s.parse::<usize>().ok())`, and the
  word is consumed from `rest` whether or not the parse succeeded. So
  `xargs -n abc rm` leaves `max_args` at `None`, drops `abc` on the floor, and
  runs **one** invocation with every input word — the opposite of what was
  asked, reported as success. `-n 0` does the same, via the `Some(n) if n > 0`
  guard falling through to the single-invocation arm. GNU: `xargs: invalid
  number "abc" for -n option`, exit 1. Same family as the `sed` flag that was
  ignored rather than refused: a wrong answer, not a missing one.
* **Empty input runs nothing, where GNU runs the command once.** `words
  .is_empty()` returns early, so `printf '' | xargs false` exits 0. GNU runs the
  command once with no arguments unless `-r`/`--no-run-if-empty` is given, and
  would exit 1 here. **BSD/macOS `xargs` does not run it** — so unlike the two
  bugs above there is no single correct answer to match, and our behaviour is
  already one of the two real ones. Left alone deliberately; noted so the next
  reader does not "fix" it into GNU's shape without knowing it is a fork.

**Fixed 2026-08-24** in `a3eea79a1`. The status is now the worst of the
invocations, and the `-n` swallow is a usage error. The empty-input fork is
still a fork and is still deliberately left as-is.

The open question this entry left — 123 or a flat 1 — is answered in
`design-decisions.md` §291, and the answer is **neither GNU's 123 nor a third
value**: the child's own status is propagated unchanged. Two existing
precedents settled it without a coin-flip.

* **Why not 123.** GNU reserves 124–127 for "child exited 255", "child killed
  by a signal", "not executable" and "not found", and 123 exists to keep those
  four rows free. This shell produces none of them — an unknown command is a
  flat 1 (`kshell.rs:7681`) — so adopting one row of a table whose other rows
  we do not implement leaves a caller unable to tell which convention it is
  reading. That is the same argument that kept the `Syntax error: …` sites at 1
  rather than borrowing bash's 2.
* **Why not §275's third value.** §275's trigger question is *"can this
  command's 'no' mean either 'I checked' or 'I couldn't check'?"* For `xargs`
  the answer is **no**: its non-zero is not an assertion about anything, it is
  a relay of somebody else's. So there is nothing for a third value to
  distinguish.
* **Why `max` and not "any failure → 1".** Under §275 a larger status is a
  worse one, so `max` propagates "I could not check" over "the answer is no" —
  it carries §275's precedence out of a single command and into a batch,
  without `xargs` knowing what any particular number means. It also makes
  `cmd | xargs foo` behave exactly like `foo args` in the single-invocation
  case, which is what the rest of the shell already does.

**What reading this function for the fix taught, again:** a mechanical screen
is a filter, not a verdict. The entry above asked for one thing (worst-status
tracking) and the function held two more silent guesses. That is now three
functions in a row (`cmd_xargs_input`, `parse_sed_command`, and the five pipe
forms) where the filed bug was the smaller half of what was actually there.

#### TD-A-CUT-ANSWERED-EVERY-UNREADABLE-ARGUMENT-BY-DROPPING-IT (lane A, 2026-08-25) — ✅ FIXED (`e43ef0307`)

**In short:** `cut` had no error path at all. Any argument it could not read was
*dropped* and the command carried on, so a mistake anywhere in the argument list
came out as a confident, successful, wrong answer. Worst of the set: `cut` with
no `-f`/`-c` left printed every line **verbatim** and exited 0, and `cut -f0`
printed a **blank line per input line**, which makes the file look empty.

**Where.** `parse_cut_args` and `cut_process` in `kernel/src/kshell.rs`. Found by
reading the function after the `sed` fix (`da9e7a0ca`) landed, not from a report.

**The whole set, every one of them exiting 0:**

| written | what it did | what it looks like from outside |
|---|---|---|
| `cut file` | printed every line verbatim | `cut` succeeded, so the fields must be right |
| `cut -f1,a,3 f` | cut fields 1 and 3 | a typo'd field number silently vanishes |
| `cut -f0 f` | cut nothing; a blank line per line | the file appears to be empty |
| `cut -f1-3 f` | printed every line verbatim | ranges were unimplemented, so nothing parsed, so pass-through |
| `cut -c1,3 f` | printed every line verbatim | same, by way of the `-c` list |
| `cut -c1-abc f` | cut to the end of the line | `abc` became `usize::MAX` via `unwrap_or` |
| `cut -q -f1 f` | opened a file named `-q` | the real file was never read |
| `cut -f1 a b` | cut `a` only | `b` silently ignored |
| `cut -c1 -d:` | ignored `-d` | the delimiter looked honoured |
| `cut -f3,1 f` | wrote field 3 then field 1 | POSIX writes selected input in *line* order, once each |

**Why the pass-through row is the important one.** Four separate mistakes above
reach it — no list, an unimplemented range, an unimplemented `-c` list, an
unreadable item — and they all land on `cut_process`'s final
`else { shell_println!("{}", line); }`, which is an identity transform reporting
success. That is byte-for-byte the bug `0a785652a` fixed for the `sed`/`awk`
pipe forms, sitting in a command nobody had re-read since.

**What "distinguishing absent from unparseable" means here.** `-c-5` (from the
first) and `-c3-` (to the last) are real forms in which an endpoint is *omitted*.
The old parser reached those two defaults with `unwrap_or(1)` and
`unwrap_or(usize::MAX)`, which cannot tell an omitted endpoint from an
unreadable one — so `-c1-abc` silently became `-c1-`. The fix is not a tighter
parse of the number; it is testing `is_empty()` before parsing at all.

**Fixed.** `parse_cut_args` now returns `Result<CutSpec, CutParseError>` with ten
named failures and GNU's wording for each. Along the way the parser moved to
[`split_words`] (the old scan tested `starts_with("-f")` against the *remaining
line*, so an operand's position decided whether it was read as a flag), `-f`/`-c`
gained comma lists and `N-M`/`N-`/`-M` ranges, multiple file operands are all
read with the worst status reported, `-s` is implemented, a line with no
delimiter is written whole as POSIX requires instead of as a blank line, and a
`-` operand names the pipe.

**The `-` operand was a regression this lane introduced.** `bb9787783` made a
file operand win over the pipe across five commands, which turned the
conventional `cat f | cut -c1 -` into a request to open a file literally named
`-`. Fixed for `cut` here; `fold`, `sed`, `awk` and `mapfile` still have it.

Rung 37 covers all of it.

#### TD-A-LIVENESS-WATCHDOG-FALSE-FIRES-ON-CAPTURED-SELF-TEST-WORK (lane A, 2026-08-25) — ✅ FIXED (`bd40ac82e`, then properly by `d5025a8d9`)

**In short:** a boot test failed with `LIVENESS WATCHDOG failure detected` even
though every self-test in that same boot passed and the run went on to reach
`BOOT_OK`. Re-running the *identical* kernel (`boot-test.sh --no-build`) passed
cleanly. So the report was false by direct evidence, not by inference — but it
still costs a ~15-minute boot cycle and, worse, teaches whoever sees it to
distrust a detector that is usually right.

**Where it fired.** Between rung 27's label and rung 28's, i.e. inside
`kshell::self_test` rung 27 (`find` over the `deep_fixture_path` tree).
`kernel/src/sched/mod.rs` ~3157 emits the report.

**What the CPU was actually doing.** The watchdog's own 16-sample RIP ring,
symbolised with `scripts/symbolize.py`:

| sample | symbol |
|---|---|
| `0xffffffff810c33ea` | `kernel::mm::heap::check_poison+0x1aa` |
| `0xffffffff810c339f` | `kernel::mm::heap::check_poison+0x15f` |
| `0xffffffff81fcf502` | `kernel::mm::rawmem::fill_u8+0x22` (×4) |
| `0xffffffff810c5b1e` | `kernel::mm::heap::HeapInner::size_class_index` |
| `0xffffffff822b0fb9` | `alloc::vec::Vec::push` |

Every sample is in the debug heap's poison fill/check path. That is forward
progress, just slow — not a hang.

**Why the detector could not tell.** This is the blind spot its own doc comment
names (`sched/mod.rs` ~2628): `USEFUL_WORK_TICKS` only advances for ticks that
preempt ring-3 or a CPU with a queued task, and `kernel_progress_count()` counts
only page faults and block I/O. A CPU-bound in-kernel loop touches neither, so
it is indistinguishable from an in-kernel infinite loop. The comment is right
that making kernel-mode ticks count as progress would blind the detector to its
primary target, and the same false positive is already recorded there for the
bzip2 self-test under KASAN.

**But there is a third signal, and this is the one worth fixing.** The report
also requires *no serial output*, and rung 27 produces none — not because it is
silent, but because `capture_command` diverts `find`'s output into a buffer.
A self-test that captures its output is invisible to a detector that watches the
serial port. The tree has ~37 capturing rungs and this is the first one slow
enough to matter, which is why it has only just appeared: `WALK_DEPTH_CAP` is
now 48, so `deep_fixture_path` builds a 50-component path and every walk over it
allocates proportionally.

**Proper fix.** Emit a serial breadcrumb from inside the expensive capturing
rungs — before the `mkdir_all` and between the `find` invocations in rung 27 —
so the watchdog sees the output that is genuinely happening. That removes a
false signal rather than dulling the detector, which is what raising the 15 s
threshold would do. Do *not* wrap the rung in the `(self-test)` drill flag: that
marks a deliberate drill, and this is not one — it would hide a real hang in
rung 27 forever.

**Severity.** Low correctness, medium friction: it fails a green tree at random
and costs a full re-run to disprove each time.

**Resolution (`bd40ac82e`) — not the fix proposed above.** The proposal was
serial breadcrumbs inside rung 27. That would have worked for rung 27 and for
nothing else. `capture_command` is also what serves `$(…)` substitution, every
pipeline stage, and the SSH and telnet servers — so a *remote user* running any
slow pipeline is invisible to the watchdog in exactly the same way, and no
amount of instrumentation inside a self-test reaches them. The blind spot is the
width of the capture mechanism, not of one rung.

The real defect is that `shell_write_bytes`, on the capture branch, is the shell
producing output and reports nothing. `kernel_output_count` now adds
`kshell::captured_output_count()` to the serial byte count, so the honest answer
to "is anything happening?" is the one the detector reads.

This is not a raised threshold, and the distinction matters: tolerating a longer
silence dulls a gate whose whole job is noticing that a boot stopped, whereas
this reports a signal that was already there. Nor can it hide a real hang — a
captured command that writes once and then blocks advances the sum once, the
silence resumes, and the report follows on schedule. What it stops reporting is
a captured command that loops *while writing*, which is a runaway rather than a
hang, is caught by the allocator, and was already unreported in the uncaptured
case for the same reason.

Regression-tested by a third phase in `test_breadcrumb_does_not_certify_liveness`,
driven through `shell_write_bytes` rather than by poking the counter — a drill
that incremented the counter itself would pass whether or not the increment was
still on the capture branch.

**Left standing:** the underlying blind spot in `kernel_progress_count`
(`sched/mod.rs`) is untouched and still real. A CPU-bound in-kernel loop that
writes *nothing at all*, captured or otherwise, remains indistinguishable from a
hang. The doc comment there argues correctly against closing it by counting
kernel-mode ticks, and this change does not revisit that; it only stops the case
where output existed and was not being counted.

**That "left standing" paragraph was the whole remaining bug, and it bit on the
very next boot.** The merged tree failed again — `[liveness] SYSTEM HANG`
between rung 27's label and rung 28's, same place, with `bd40ac82e` in it. The
reason is stated above without being recognised: rung 27 runs `find` looking for
names that *are not there*. It produces no output, captured or otherwise,
because there is nothing to produce. Counting captured bytes cannot help a
command whose correct byte count is zero.

**Resolution, part two (`d5025a8d9`).** With output silent by right, the
total-hang branch falls back on `kernel_progress_count()`, whose claim is "no
task-level forward progress of any kind". Its two sources — resolved page faults
and completed block I/O — are the trace left by work that touches *a mapping or
a disk*. A kernel-side computation over data already mapped and already resident
leaves neither, so the counter stood still for 15 s while the kernel was working
as hard as it ever does.

Allocation is the trace that work *does* leave: a directory walk allocates a
path per component per level, and the RIP ring above is four-fifths inside
`heap::check_poison` and `Vec::push`. `mm::heap::alloc_progress_count()` sums
`SLAB_ALLOCS`, `LARGE_ALLOCS` and every per-CPU cache's own `slab_allocs`,
lock-free — this runs from the timer tick, and a watchdog that blocked could
deadlock against the hang it exists to report — and `kernel_progress_count()`
adds it.

Allocations only, never frees. A loop that allocated and released one block for
ever is a *livelock*, and the busy-livelock branch makes a separate claim about
those; this counter exists to refute "nothing at all is happening". A hang
spinning on a lock allocates nothing, so nothing the branch exists to catch is
hidden by counting them. As with part one, this is not a raised threshold: it
reports a signal that was there all along.

**A second defect surfaced while hardening the drill for it.** Each of the four
phases of `test_breadcrumb_does_not_certify_liveness` pins `LIVENESS_LAST_KWORK`
so the progress discount is a no-op and the phase measures the *output* discount
it names. `liveness_check` then takes its own fresh reading, and
`without_interrupts` silences only the local core — so the pin was never
airtight. It held while the sources were faults and block I/O, which an idle AP
does not generate; an allocation on another core breaks it, and breaks it
*invisibly*: a breadcrumb that wrongly counted as output and a cross-CPU
allocation both leave the stall counter at 0. The drill would have reported the
first when it saw the second, and phases 3 and 4 — which assert a *reset* —
would have passed vacuously on exactly the boots where they were needed.

The pin is now verified rather than assumed. `liveness_check` swaps its own
reading into `LIVENESS_LAST_KWORK` *before* it branches, so reading the static
back afterwards says exactly what it compared against, with no second sample and
so no second race. A broken pin restarts the drill rather than failing it — that
is the watchdog being right, not the code under test being wrong — bounded at 32
restarts so a future change that makes every interval busy (a `serial_println!`
that allocates, say) fails loudly instead of spinning under a `cli`.

**Noticed in passing.** `mm::heap::stats` indexes `PCPU_SLAB_CACHES` with a bare
`cpu_count().max(1)`, justified by a SAFETY comment resting on an invariant held
a long way from there. `alloc_progress_count` clamps to `HEAP_MAX_CPUS` instead:
the timer tick is not where that invariant should be discovered to have lapsed.

**Still left standing, and now genuinely narrow:** a kernel loop that faults
nothing, reads no block, allocates nothing and writes nothing is still
indistinguishable from a hang — which is correct, because that description also
fits a hang. The remaining false-positive risk is a long stretch of pure
arithmetic over a fixed buffer, which no rung currently does.
