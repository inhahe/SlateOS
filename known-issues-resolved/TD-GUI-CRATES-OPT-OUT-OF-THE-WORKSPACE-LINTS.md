## TD-GUI-CRATES-OPT-OUT-OF-THE-WORKSPACE-LINTS (lane C, 2026-08-17) - **fixed**

**Status:** FIXED (lane C). Re-checked 2026-10-05: every crate under `gui/`
sets `[lints] workspace = true`, the five in the table below included, and
none keeps a lint policy of its own. Filed among the resolved that day.

**What.** `CLAUDE.md` requires every crate to enable `clippy::all` +
`clippy::pedantic` and five defensive lints (`unwrap_used`, `expect_used`,
`panic`, `indexing_slicing`, `arithmetic_side_effects`). The workspace root
defines all of them in `[workspace.lints.clippy]`, and a crate opts in with
`[lints] workspace = true`. **Of the five `gui/` crates, exactly one —
`gui/font` — opts in.** The other four are in two different states of partial
compliance:

| Crate | `clippy::all` / `pedantic` | The five defensive lints |
|---|---|---|
| `gui/font` | via `[lints] workspace = true` | **yes** |
| `gui/remote` | inner attributes | **no** |
| `gui/toolkit` | inner attributes | **no** |
| `gui/window` | inner attributes | **no** |
| `gui/compositor` | **neither** | **no** |

So `gui/compositor` — 4900 lines, the display server every application depends
on — is effectively unlinted: `cargo clippy -p compositor` reports the default
warn-level `clippy::all` and nothing else, which is why a clean clippy run on
that crate is a much weaker signal than it looks.

**How big the backlog is.** Measured 2026-08-17 with
`cargo clippy -p compositor --all-targets -- -W clippy::pedantic`: roughly 1200
warnings. The large groups, in order: 370 `must_use_candidate` on methods and
61 on functions and 27 returning `Self`; 162 `doc_markdown` (an identifier in a
doc comment without backticks); ~250 numeric-cast warnings across
`cast_precision_loss` (76 `u32`→`f32`, 59 `usize`→`f32`),
`cast_possible_truncation` (35+35 `f32`→`u8`, 33 `f32`→`i32`, 26+26
`f32`→`u32`, 20+20 `f32`→`usize`) and `cast_lossless` (41 `u8`→`f32`); 24+24
`cast_possible_wrap` (`usize`→`i32`); 22 `missing_errors_doc`; 17
`uninlined_format_args`; plus long-literal and identical-match-arm hits.

**Why it matters unevenly.** Most of that list is cosmetic — backticks and
`#[must_use]` are style. The numeric casts are not: a `f32`→`u8` truncation in
a *compositor* is a colour channel, and an out-of-range float becomes a
saturating or wrapping value depending on the cast, which is a rendering bug
that shows up as a wrong pixel rather than a panic. The defensive five matter
more still, and none of the four non-compliant crates has them on: an
`indexing_slicing` hit in the compositor is a panic in the display server,
which takes the whole desktop down, and the compositor indexes framebuffers by
computed offsets constantly.

**The proper fix**, and it should be done crate by crate rather than in one
sweep:

1. `gui/compositor` first, since it is both the largest exposure and the only
   one with *no* lint header at all. Add `[lints] workspace = true`, then clear
   the backlog in themed commits — the numeric casts first (they can hide real
   bugs), `must_use`/`doc_markdown` last (they cannot).
2. `gui/remote`, `gui/toolkit`, `gui/window`: replace the inner
   `#![deny(clippy::all)] #![warn(clippy::pedantic)]` attributes with
   `[lints] workspace = true`, which adds the defensive five. The inner
   attributes are then redundant and should go, so there is one place that says
   what the lints are.
3. Test modules keep the standard three-line-comment `#![allow(...)]` header
   the rest of the tree uses — a test that indexes out of range should fail
   loudly.

**Severity.** Medium. Nothing is known to be broken, and `clippy::all` is clean
everywhere; what is missing is the enforcement that would *find* the next bug
of these kinds. The reason it is logged rather than fixed on the spot is that
clearing 1200 warnings in the middle of the transport work would have buried a
functional change under a formatting one — but it should not wait long, because
the backlog grows with the crate.

### Correction, 2026-08-17: the "~1200 warnings" figure was wrong, and so was the crate list

Two errors in the original entry, both of which made the job look bigger and
narrower than it is.

**The count.** `-W clippy::pedantic` on the command line *bypasses the
workspace's `[workspace.lints.clippy]` table entirely*, including its ~26
deliberate, documented `allow`s (`doc_markdown`, `format_push_string`,
`uninlined_format_args`, `single_match_else`, …). So that measurement counted
warnings the workspace has already decided not to care about — 162
`doc_markdown` hits, to pick the clearest example, are allowed workspace-wide
and would never have appeared. **The only honest way to measure this is to add
`[lints] workspace = true` and build**, which is what the numbers below are.
The corollary is general: never quote a warning count obtained with `-W` flags
in a workspace that has a lints table.

**The crate list.** `gui/` has ten crates, not five; the entry's table omitted
`appearance`, `clipboard`, `credentials`, `desktop` and `notifications`.

**Where it now stands.** Five of ten opt in:

| Crate | `[lints] workspace = true` | Warnings remaining |
|---|---|---|
| `gui/appearance` | yes | 0 |
| `gui/clipboard` | yes | 0 |
| `gui/compositor` | yes | 0 |
| `gui/credentials` | yes | 0 |
| `gui/font` | yes | 0 |
| `gui/desktop` | no | ~1752 |
| `gui/toolkit` | no | ~946 |
| `gui/remote` | no | ~720 |
| `gui/window` | no | ~660 |
| `gui/notifications` | no | ~598 |

**What the opt-in has actually found**, which is the reason to keep doing it
crate by crate rather than by adding `allow`s: every warning family
investigated so far has contained at least one real defect, not a style
complaint.

- `gui/clipboard`, `gui/compositor`: logged separately (a negative-spread
  shadow filling the screen; four security defects).
- `gui/credentials`: a two-time pad (every encryption reused nonce zero), a
  key-derivation function that did not stretch, a verifier that made a
  password guess cost one SHA-256 rather than one unlock, two non-constant-time
  secret comparisons, a `% bound` modulo reduction in the password generator, a
  byte/character confusion in the hex decoder, and five copies of a
  check-then-index whose failure branch was silently wrong. Fixed in
  `765949194`, `d8ad84f54` and `eb6e77799`.

**One thing the credentials pass changed about the method.** That crate did not
merely lack the opt-in; it had *its own* lint policy — `#![deny(clippy::all,
clippy::pedantic)]` plus forty-two `#![allow(...)]` lines — which is worse than
having none, because it looks compliant. Deleting the private policy outright
and letting the workspace govern left **eleven** warnings, all fixable, none
needing an allow: thirty-seven of the forty-two allows were suppressing lints
that do not fire at all. So for the remaining five crates the first step is
**delete any inner lint attributes and see what is left**, not translate them.

**Also note** the counts above are pre-fix and will shrink faster than they
look: 96 of `gui/credentials`'s 97 were in one hand-copied SHA-256, and were
removed by deleting it rather than by fixing 96 sites (see
`C-SHA-256-IS-IMPLEMENTED-ELEVEN-TIMES-IN-THIS-TREE`). Expect the same shape
elsewhere — a large warning count usually means a duplicated *shape*, and the
cheap fix and the correct fix are the same one.

### Second correction, 2026-08-17: the per-crate counts above are wrong too, and two crates are free

The five figures in the table (`~1752`, `~946`, `~720`, `~660`, `~598`) are
still not measurements of the crate they name. **`cargo clippy -p X -- <flags>`
applies those flags to X's *dependencies* as well**, because the flags go to
the clippy driver through `CLIPPY_ARGS` and the driver applies them to every
crate it compiles in that invocation. Every one of these crates depends on
`guitk`, so each figure was mostly a count of `gui/toolkit`'s warnings
attributed to whichever crate happened to pull it in. That is why the five
numbers were all the same order of magnitude and all wrong.

The honest measurement filters the JSON diagnostics by the span's file path,
keeping only those inside the crate's own directory. Doing that:

| Crate | logged above | actually |
|---|---|---|
| `gui/desktop` | ~1752 | 1561 |
| `gui/toolkit` | ~946 | **1488** |
| `gui/remote` | ~720 | 281 |
| `gui/notifications` | ~598 | **20** |
| `gui/window` | ~660 | **0** |

So the job is not five comparable crates. It is two crates that hold 95% of
the debt (`toolkit` and `desktop`, 3049 between them), one middling one
(`remote`), and **two that are essentially already done**: `gui/window` is
clean and needs only the opt-in plus the deletion of its private
`#![deny(clippy::all)] / #![warn(clippy::pedantic)]` pair, and
`gui/notifications` has twenty.

The composition is also uniform and worth stating, because it says what the
work actually is:

| Lint | toolkit | desktop | remote | notifications |
|---|---|---|---|---|
| `indexing_slicing` | 772 | 491 | 153 | 11 |
| `arithmetic_side_effects` | 567 | 785 | 94 | 6 |
| `unwrap_used` | 52 | 170 | 20 | — |
| `expect_used` | 35 | 68 | 10 | — |
| `panic` | 25 | 17 | 4 | 3 |
| everything else | 37 | 30 | 0 | 0 |

Over 96% of it is the two defensive lints, and — per the note above about
duplicated shapes — `gui/toolkit`'s 600 worst are concentrated in one file,
`svg.rs`, whose top repeated source lines are `i += 1;` (74), `*pos += 1;`
(34) and `while i < tokens.len() && is_number_token(&tokens[i])` (18). That is
one hand-rolled tokeniser cursor written out several hundred times, not
several hundred problems. The fix is a cursor type, once.

**Methodological rule, third version.** Measuring clippy against a workspace
lints table has now been got wrong twice in the same entry. Both times the
error inflated the number and blurred which code it belonged to. The rule:
*add `[lints] workspace = true` and build the crate alone*, and if that is not
possible yet, reproduce the table on the command line **and filter the
diagnostics by file path**. Never quote a raw total from a `-p` invocation.

### Three crates done, 2026-08-17 — and a third figure that was wrong

`gui/notifications`, `gui/window` and `gui/remote` now opt in and are at **0
warnings**. Eight of ten `gui/` crates are compliant; `toolkit` and `desktop`
remain.

| Crate | predicted above | actual, opted in | after |
|---|---|---|---|
| `gui/notifications` | 20 | 20 | 0 |
| `gui/window` | **0** | **16** | 0 |
| `gui/remote` | 281 | **137** | 0 |

**Two of those three predictions were wrong, in opposite directions, and for
the same reason.** Both were measured *before* the opt-in, under whatever lint
set the crate already had. `gui/window` measured 0 because its private
`#![deny(clippy::all)] #![warn(clippy::pedantic)]` does not include the
defensive five — they are restriction lints, in neither group — so the
measurement was of a strictly smaller table than the one being adopted, and
"0 warnings, a one-commit no-op" was a prediction the measurement could not
support. `gui/remote` went the other way: 281 double-counted sites appearing
in both the lib and test builds of the same file, where 137 is the count of
distinct source locations. **Deduplicate by `(file, line, column, lint)`, and
never predict a post-opt-in count from a pre-opt-in build.**

**What the three found.** The pattern from `credentials` and `compositor` held
in two of three: the warnings were not style complaints.

- `gui/notifications` — a real bug. `DndSchedule` was four public `u8`s
  validated nowhere, so `set_dnd_schedule(25, 0, 7, 0)` was accepted silently,
  produced a start of 1500 minutes (past the largest time of day, 1439), and
  compared as an overnight window that then never opened. Quiet hours simply
  stopped happening with nothing reporting it. Fixed by making the state
  unrepresentable: private minutes-from-midnight, a checked constructor, and a
  setter that refuses and keeps the previous schedule.
- `gui/window` — no user-visible bug, but `pub mod testing` is a test double
  that *ships in the library*, so nothing had ever held it to production
  standards though it compiles as production code. Its decode loop could spin
  forever on a decoder reporting zero bytes consumed: a hang, which is the one
  failure mode a test harness must not have, because the suite then dies on a
  timeout naming no test at all.
- `gui/remote` — 137 warnings that were four shapes, not 137 problems. The
  largest: `Reader`'s fields carried no `pub`, which reads like encapsulation
  and is not, because **a field private to the crate root is visible to every
  descendant module**. All five sibling modules reached past the checked
  accessors and indexed `buf` directly, with the same four lines each — in a
  decoder whose module doc promises that all reads are bounds-checked. Moving
  the type into its own module made the fields private in the sense originally
  intended, and turned any future reach-around into a compile error.

**The generalisable finding**, which is new and worth carrying to `toolkit`
and `desktop`: *a lint firing many times in a crate that looks well-factored
is often reporting a broken abstraction rather than sloppy call sites.* Three
of the four `guiremote` shapes were an abstraction that existed, was correct,
and was bypassed everywhere — the cursor, the count back-fill, the capacity
hint. The fix was never "check the bound at the call site"; it was to make the
bypass impossible or the mechanism unnecessary. Fixing 137 sites would have
left the design that produced them intact.

### Nine of ten done, 2026-08-18: `gui/toolkit` is at 0, from 1488

`gui/toolkit` (`guitk`) opted in at **1488** distinct sites and is now at
**zero**, with the crate's test count up from 660 to 865 over the sweep. Only
`gui/desktop` (1561) remains.

**The generalisable finding from `gui/remote` held for all 1488.** Not one of
the ~35 files in the sweep was fixed by adding a bounds check at a call site.
Every file was fixed by finding the abstraction that should have existed, and
in about a third of cases the abstraction *already existed in the same crate*
and was simply not reached for. Reusing what was there:

| Written once | Files that had re-derived it |
|---|---|
| `cycle::before` / `after` / `indices` | `menubar`, `tree`, `pathbar`, `textview`, `modal`, `menu`, `tabs` |
| `TextCursor::prev_in` / `next_in` | `pathbar`, `modal` |
| `Canvas` | `svg`, `screenshot`, `colorpicker` |
| `tzrules` (the whole-tree TZ engine) | `dialog` |

The new abstractions the sweep had to write, each replacing a shape repeated
many times: a lexer cursor type in `svg` (the single largest group in the
crate — `i += 1;` seventy-four times was one cursor, not seventy-four
problems), a non-backtracking glob matcher in `context_ext`, `Color::mean`,
and a `NonZeroU32`/`NonZeroU64` divisor wherever a computed denominator had
been guarded by an `if` two statements away.

**The recurring fault, stated once.** Almost every one of the 1488 was the same
sentence: *a proof that lives in a different statement from the code it
justifies.* `if slot <= MAX` then `TABLE[slot]`; `if out_a == 0 { return }`
then `/ out_a`; `if len > 1` then `len - 1`; `if header.len() >= end` then
`&header[..end]`; a signed cursor stepped off the end and pushed back by the
next iteration's first `if`. The lint is not asking for the proof to be
repeated. It is pointing out that the proof and the use can drift apart,
and in this crate they had — see the defects below, every one of which is an
instance of exactly that drift.

**What it found.** Eleven defects that were live, not hypothetical:

- `svg::parse_transform` **panicked** on a malformed `transform` attribute —
  reachable from any SVG file the user opens.
- `Canvas`/`SvgRenderer::new` computed `width * height` and let `Vec` abort on
  the byte count; an image header can claim any dimensions it likes.
- `scaling::set_monitor_scale(usize::MAX, ..)` wrapped past an upper-bound-only
  check onto slot 0 — which was the **global** scale factor. One bad monitor id
  rescaled the whole desktop.
- `dialog`'s Modified column showed **a date that does not exist**: no leap
  years and twelve 30-day months, so by 2026 it was about two weeks early, with
  the year advancing early and the day-of-month almost never right. It now
  reads through `tzrules`, the same engine as the libc, the shell's `%(…)T` and
  the taskbar clock.
- `Color::lerp` returned **transparent black** for a NaN factor, which is what
  an animation produces the instant it divides elapsed time by a zero duration.
  `f32::clamp` passes NaN through; `NaN as u8` is 0.
- `Color::over`'s `out_a == 0` guard was dead code given the `sa == 0` early
  return, and stood apart from the four divisions it was supposed to protect.
- `textview::scroll_by` overflowed on `i32::MIN`.
- `colorpicker` measured its palette grid in the drawing code and again in the
  hit-test, and the two had drifted — clicks landed on the wrong swatch. The
  hue readout truncated through `u8` besides.
- `context_ext`'s glob matcher was exponential on a pattern with several `*`s.
- `tree::items_in_rect` walked rows without an upper bound.
- `FormValidator` (`disabled`) kept two parallel `Vec`s that had to agree about
  which fields exist, and did not: a query for an unregistered widget *pushed*
  a state entry so it had something to return a reference to, after which the
  form counted a field nobody had registered as part of its own validity. Its
  `label` was also stored, documented as "for error messages", and never read —
  so the disabled submit button's tooltip said "Required" and named none of the
  five boxes the user had to go back to.

**One methodological note to carry into `gui/desktop`.** The largest single win
in the crate came from *deleting* code, not fixing it: `svg.rs`'s ~600 sites
were one hand-rolled tokeniser cursor, and `textview`'s ANSI parser was
hand-decoding UTF-8 it had already been handed decoded. Before fixing a group,
check whether the group is one shape — the repeated *source line* is the tell,
and `scripts/clippy-sites.py --sites` prints it.


### 2026-08-20 — `gui/desktop`, the tenth and last crate. Sweep complete.

**Correcting the figure in this entry.** The 1561 recorded for `gui/desktop`
was measured with `cargo clippy -p desktop -- -W clippy::…`, which is not what
that crate is graded on: the flags leak into every dependency, and they bypass
the workspace `[workspace.lints.clippy]` table rather than adding to it.
`gui/desktop` had *already* opted in with `[lints] workspace = true`. Built
alone, with `--message-format=json`, filtered to `gui/desktop/src`, and
deduplicated by `(file, line, column, lint)`, the real backlog was **66**.

The methodology matters more than the number, so state it plainly: **never
quote a raw total from a `-W`-flagged run.** Measure the crate as configured,
filter to its own files, and deduplicate — the same site reported once per
target (lib, bin, test) triples a count for free.

Note also that `clippy::arithmetic_side_effects` **does not lint
floating-point arithmetic**, so none of desktop's f32 layout code fires. Every
site in this crate was integer arithmetic. That is worth knowing before
budgeting a sweep of a rendering crate.

**Three more abstractions the crate had re-derived**, each found the same way —
a lint firing many times turned out to be one shape repeated, and *the copies
disagreed with one another*. The disagreement is the finding; the lint was only
what pointed at it.

| Copies | What they disagreed about | Written once as |
|---|---|---|
| 14 id counters | What happens at the top of the range: `+= 1` (6), `saturating_add` (6), `wrapping_add` (3), `checked_add` (2) — **four answers** | `guitk::idseq` |
| 15 percentages | What "percent of nothing" is: `0`, `0.0`, `100`, `100.0` — **four answers**; and only one of the fifteen clamped | `guitk::ratio` |
| 3 quiet-hours windows | Nothing — but all three shared one hole, and one shipped it | `guitk::daywindow` |
| 22 index steps | What happens at the end of the list: 13 wrapped, 9 clamped — **and no call site said which it meant** | `guitk::step` |

Each of those is worth reading as a separate lesson:

- **`idseq`.** Wrapping is the worst of the four answers, and three of the
  fourteen chose it. The ids it reuses first are the *lowest*, which in a shell
  that has been up long enough to wrap are overwhelmingly the ones still alive
  — the first workspace, the pinned window, the tray notification sitting there
  since boot. A duplicate id is not a crash; it is one object answering to
  another's name, which is how a dismissal dismisses the wrong notification.
  The module offers `issue() -> Option<T>` universally and
  `issue_infallible() -> T` **only** for `T: Inexhaustible` (u64/u128), so the
  shortcut is gated by the compiler rather than by a comment. The four 32-bit
  sequences were *widened* rather than given an error path no caller could act
  on: `IconId`, `DeviceId`, `RuleId`, `RecordingId`.
- **`ratio`.** Each of the four zero-case answers was right for its own caller,
  which is exactly why it is not a decision a shared helper should make for
  them. `percent` returns `Option`, so `.unwrap_or(0.0)` puts each caller's
  answer in the same expression as the division. An empty disk is 0% used; a
  battery with no recorded design capacity is assumed healthy at 100%.
- **`step`** (renamed from `cycle`, which had owned only the wrapping half).
  This one is a fault *one level up* from the rest of the sweep: not arithmetic
  without a proof, but **behaviour without a decision**. The launcher stops at
  the last result; the Wi-Fi list wraps to the first network. Both are right —
  a ranking has no meaningful "after the worst match", a short menu of networks
  is a ring you thumb through — but no call site stated which it had chosen, so
  the answer was settled by whoever typed the loop. The policy is now in the
  name (`wrapping_after` / `clamped_after`) and neither is the default.

**Live defects found.**

- `backup_settings::record_backup` advanced a `next_backup_id` counter that
  **nothing ever read**. Every caller invented its own id, and all the tests
  passed `id: 1`, so nothing stopped two history entries sharing one — which
  would have made delete-by-id ambiguous the first time it mattered. The
  function now assigns the id and returns it.
- `notif_pane` aged notifications with bare subtraction behind an
  `if now < timestamp` guard. A clock moved backwards — NTP correction, DST
  fix, or a stamp from a process whose clock ran ahead — leaves timestamps in
  the future; without the guard those age to near `u64::MAX`, i.e. sorted
  `Older` and dated half a trillion years ago. ~18 sites elsewhere in the tree
  already used `saturating_sub`; these two were the outliers.
- `security_dialog::truncate_str` was the **fifth** copy of the char-boundary
  walk `TextCursor::snapped_in` was extracted to own, and its doc comment said
  `max` was in characters. It has always been bytes — a name like that is how a
  caller sizes a field in characters and gets a third of it for text that is
  not ASCII.
- `resmon`'s sparkline indexed `i` and `i - 1` with a `.unwrap_or(0.0)` on each
  end, so a missing sample would have been drawn as a spike down to the floor —
  a fabricated reading in a graph of real ones. It walks `windows(2)` now, and
  the pair is always real.
- `login_screen`'s lockout expiry could wrap *behind* `now_ms` and clear the
  lockout on the next tick. It saturates in the safe direction now: an
  unrepresentable expiry is a lockout that does not end.

**`.get()` throws the proof away.** The last two sites in the crate were
`icons::GridConfig::columns_in`/`rows_in`, still firing after the cell size had
been moved to `NonZeroU32`, because they divided by `self.cell_width.get()`.
The lint is right to fire: `.get()` hands the compiler a plain `u32`, so the
division one expression later is by a value with no non-zero proof attached.
Dividing by the `NonZeroU32` itself uses `Div<NonZeroU32> for u32`, which
cannot panic. The cost is `const`, since operator impls are not const — no
caller wanted it. Worth remembering as the general shape: **reach for `.get()`
last, not first**, or the type you introduced to carry a fact stops carrying it
at the first call site.

**One shape deliberately left alone.** `let before = v.len(); v.retain(…);
v.len() < before` appears **27 times** across `gui/` and `apps/` — by count the
largest duplicated shape in the sweep. It was *not* extracted, because it fails
the test the other four passed: the copies do not disagree, and none of them is
wrong. 25 return the boolean, 2 return a count, and both are correct APIs.
Extraction here would buy two fewer lines per site at the cost of an
indirection over an idiom every Rust reader already knows. Recorded here so the
next sweep does not re-discover it and reach a different conclusion by
accident: **duplication alone is not the warrant — divergence is.**

**`gui/desktop` is at 0 sites.** All ten `gui/` crates are now clean under the
full workspace lint set.
