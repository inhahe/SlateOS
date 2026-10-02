## TD-B-A-CARGO-RUN-IN-THIS-TREE-IS-82-PERCENT-ONE-REPEATED-WARNING (lane B, 2026-09-04)

### Re-measured 2026-09-12 -- the scale is gone, the mechanism is not explained

Flagged by `check-stale-blockers.py`'s third pass because `Cargo.toml` had moved
under it. What the re-read found:

**The population collapsed.** Measured with `git ls-tree` at both revisions,
not estimated:

| | 2026-09-04 | 2026-09-12 |
|---|---:|---:|
| workspace manifests | 2,950 | **412** |
| under `userspace/` | 2,757 | **208** |
| members with no `[lints]` | 2,733 | **168** |

So the entry's central argument -- that fixing this means "a 2,733-file commit
the operator's answer may largely revert" -- is no longer about 2,733 files.
The stub consolidation happened; §1005 and §1006 took most of those crates into
`coreutils`.

**And the symptom did not reproduce, in four probes:**

* today's full `cargo test -p coreutils` log: **247 KB, 0 occurrences** (the
  entry measured 5.02 MB and 8,166);
* `cargo metadata --no-deps` and `cargo tree --workspace`: 0 stderr lines;
* `cargo check -p quoting` -- a member that **survived** and still has no
  `[lints]` -- 0 occurrences, under *both* toolchains.

**WHAT IS NOT ESTABLISHED, stated plainly so nobody reads the above as a fix.**
Why it fired 8,166 times on 2026-09-04 is unexplained. Deletion cannot be the
whole answer: `userspace/acl` had no `[lints]` then, still has none, and does
not warn now. Something about the invocation shape or the toolchain differs and
I did not isolate it. **This entry is therefore NOT closed**, and the warning
may return the moment whatever suppresses it changes.

**A mistake worth keeping, because it nearly became the finding.** My first
three probes all ran the *Windows* cargo (1.95.0, 2026-03-21). The entry
measured a `--only linux` run -- the **WSL** toolchain, cargo 1.98.0
(2026-08-05). Two different compilers six months apart, and I was about to
write "not reproducible today" on the strength of the one that may never have
emitted the lint at all. It only came out because the version string was worth
checking. *Same shape as the rest of this week: the probe ran correctly and
answered a narrower question than the one asked -- here, "does THIS cargo warn"
in place of "does the cargo that warned still warn".* Re-running it under WSL
1.98.0 gave the same answer, which is the only reason the paragraph above is
allowed to say "under both toolchains".

**In short:** Every `cargo build`/`clippy`/`test` in this workspace prints the
same warning about two thousand times, once per crate, and that one warning is
**82% of the output by volume**. Nothing is broken by it; what is broken is
anyone's ability to read a build log, including this session's — a real error
scrolls past inside four megabytes of identical paragraphs.

**Measured, 2026-09-04**, on the `--only linux` run of `oils`+`cpio`+`stat`
(four cargo invocations, log kept at 5.02 MB):

| | |
|---|---:|
| log total | 5.02 MB |
| `missing [lints] to inherit [workspace.lints]` | **4.11 MB (81.9%)** |
| everything else — the compile, the lints, 4 test binaries | 0.91 MB |
| occurrences of the warning | 8,166 (≈2,000 per cargo invocation) |
| manifests in the tree | 2,950 |
| manifests with a `[lints]` section | 217 |
| manifests without one | **2,733**, of which **2,718 are under `userspace/`** |

**Where it lives.** Each crate's `Cargo.toml`. The fix per crate is three
lines:

```toml
[lints]
workspace = true
```

`cargo::missing_lints_inheritance` is `warn` by default, and the workspace root
does define `[workspace.lints]`, so every member that does not opt in is told
so on every invocation.

**Why this is not simply "add the three lines to 2,733 manifests", and why it
is filed rather than fixed.** Those 2,718 `userspace/` manifests are
overwhelmingly the stub crates behind the open question at
`open-questions.md` → *"2,288 of the 2,756 commands in `userspace/` report
success for work they never did. Which ones do we keep?"* (lane B, 2026-09-02),
which is **still awaiting the operator**. The three options there are keep /
delete / mark-unimplemented, and two of the three delete most of these
manifests outright. Writing `[lints] workspace = true` into all 2,733 now would
be a 2,733-file commit that the operator's answer may then largely revert —
and, worse, it would make the stubs *inherit the workspace's strict lints*,
which is a real behaviour change (`unwrap_used`, `indexing_slicing`,
`arithmetic_side_effects` and friends) applied to code we may be about to
delete, on no one's decision.

So this is **deliberately blocked on that question**, not overlooked. It is
recorded separately because it is a distinct cost — the unreadable logs are
paid today, by every build, whatever is eventually decided about the stubs —
and because whoever answers the open question should know that the answer
carries this with it.

**Do not "fix" it by silencing the warning** (`--cap-lints`, `-A
cargo::missing_lints_inheritance`, or `[workspace.lints]` removal). The warning
is correct: those crates genuinely are not covered by the lint policy
`CLAUDE.md` requires of every crate. Silencing it would convert a loud,
accurate statement that 2,733 crates are unlinted into silence on the same
fact, which is the failure shape half this file is about.

**If it is never fixed:** build logs stay unreadable at 5 MB a run, which makes
every future diagnosis in this lane more expensive and makes a genuine warning
easy to miss — and 2,733 crates stay outside the lint policy the project
requires.

**Interim workaround** for reading a build log:

```sh
grep -v -e 'missing `\[lints\]`' -e 'missing_lints_inheritance' \
        -e 'to inherit `workspace.lints' build.log | less
```
---

### Lesson 400: a control that is drawn from live state looks more wired than one that is not (lane C, 2026-09-04)

*(Lessons 400–403 were numbered 110–113 until 2026-09-24. Lane B had
written a different 110–113 a day earlier, so the second writer moved, into
lane C's band. See
`requests/b-ac-lesson-numbers-have-the-disease-the-section-numbers-were-cured-of.md`.)*

`apps/spreadsheet` drew a toolbar of twelve buttons. The bold button was filled
in when the selected cell was bold; the alignment buttons showed which of the
three was in force; the border button lit when the cell had borders; the freeze
button lit when panes were frozen. All of that was read from the live document
on every frame, and all of it was correct.

None of the twelve could be clicked. `handle_left_click` had no hit test for the
toolbar at all -- the renderer walked a private `bx` accumulator that existed
only inside `render_toolbar`, so nothing outside it knew where any button was.
The handler's own comment said so, and said what to do about it:

> Not a cell: the toolbar, the formula bar, the scrollbars. Left unconsumed so
> that whatever eventually handles those can see it

Nothing ever did. The same file's find-and-replace panel drew a "Replace:" field
that no key could type into and three buttons -- Find Next, Replace, Replace All
-- that no click could press; `replace_current` and `replace_all` were written,
tested, and had no caller anywhere. Five of the nine features the file's own
module header advertised were reachable by no key and no click.

**The lesson is about how this hides.** An unwired control that is drawn from a
constant looks dead: the border button would have been permanently unlit and
someone would have asked why. An unwired control drawn from *live state* looks
alive. Select a bold cell and the B button lights up; the panel is clearly
tracking the document, so the natural conclusion is that it works. Every signal
the eye uses to check "is this connected?" is present, and all of them are
answering a different question -- whether the *renderer* can see the state, not
whether the *handler* can see the click.

The repair is the same one this tree has reached for repeatedly (`ScrollbarGeometry`,
`col_screen_x`): **one geometry function, two callers.** `toolbar_buttons()`
returns where each button is, what it does, and how it is drawn;
`render_toolbar` draws that list and `toolbar_button_at` hit-tests it. A button
that is drawn is a button that can be pressed, by construction, and a new button
cannot be added to one without the other.

The corollary for reviewing: to check whether a control is wired, do not look at
what it draws. Search for its *action* and count callers outside the test
module. Fifteen functions in this file had none.

### Lesson 401: two copies of a rule agree on whichever one was written second (lane C, 2026-09-04)

`Sheet::set_cell_input` and `Sheet::set_cell` each ended with the same four
lines: if the cell has no value and no raw input, remove it from the map,
otherwise insert it. Not stored blank is a real optimisation -- an empty sheet
costs nothing and `export_csv` derives its bounds from the key set.

The rule was wrong, in both copies, in the same way: **a cell's formatting is
also something worth keeping.** Selecting an empty column and making it currency
-- which is how anyone lays out a sheet before typing into it -- built a cell
with a format and no text, which both copies dropped. The format vanished with
no error, nothing on screen, and no test noticing, because the two callers
agreed with each other perfectly.

It stayed invisible for as long as the toolbar was unclickable (lesson 400):
there was no way to ask for the thing that did not work. Wiring the toolbar made
it a bug you could hit in the first ten seconds. Two defects that each conceal
the other are not twice the work to find; they are indefinitely hidden until one
of them is fixed.

Both copies are now one private `store`, whose doc comment states the rule and
why formatting counts. The same invariant was broken at the other end --
`delete_selection` wrote `Cell::empty()`, resetting the format along with the
contents -- so Delete now clears what is in a cell and not how it is drawn,
which is what every spreadsheet does.

### Lesson 402: a getter the harness calls once is a getter the app cannot use to report anything (lane C, 2026-09-04)

`oswindow::app::App::title` was read exactly once, when the window was created.
The trait said so, with a reason: making it live "would mean re-reading it on
every event to find out whether it had changed, which is a round trip per mouse
move to answer no".

Every part of that is either wrong or avoidable. Re-reading is a local call that
builds a `String` -- no round trip. Only a *difference* costs one, because the
loop compares against the title it last sent. And the natural place to do it is
the batch boundary, not the event: a drag of two hundred mouse moves is one
boundary.

What the rule bought was a fleet of applications whose titles were true for one
instant. `apps/slides` was wired an hour before this was noticed; it opens saying
"slide 1 of 6" and says it on slide six. Its test asserts that the title follows
the current slide -- and passes, because the test calls `title()` itself. That is
the shape this window has hit five times now: **a check that runs, passes, and is
about the wrong thing.** The test was not wrong about the app; it was wrong about
who its reader is.

The general form: when a trait method's contract is "called once", every
implementor that computes it from mutable state has written a latent bug, and no
test of that implementor can find it, because the fault is in the *frequency of
the call* and the implementor does not make the call. Look for these by grepping
the harness for the call site, not the implementors for correctness.


### Lesson 403: when the feature *is* the timer, "no clock" is not a stale display but a missing program (lane C, 2026-09-04)

`apps/systemrestore` is a snapshot manager. Its headline feature, named in the
first line of its own module doc, is "scheduled automatic snapshots with
retention policies". It has a `ScheduleConfig` with a frequency and an enabled
flag, a `RetentionPolicy` with count, age and size limits, a `check_schedule`
that takes a snapshot when one is due, an `apply_retention` that prunes by the
policy, and a Schedule view that draws a countdown to the next one.

`check_schedule` and `apply_retention` had no caller. Neither did thirteen other
methods. The program had no key handler, no mouse handler, and no `handle_event`
of any kind: `main` built the UI, rendered one frame, and returned.

This is *not* the same defect as lesson 47 (`an app that keeps time but never
receives the clock`), though it looks like it and shares a cure. In lesson 47
the app does something and displays a stale number while doing it. Here the
number **is** the product. A snapshot manager that never takes a snapshot on a
schedule is not a snapshot manager with a cosmetic fault; it is a viewer of
sample data with a countdown drawn on it.

The distinction matters when triaging a fleet of unwired apps, because it
changes the priority. "The clock is frozen" reads like polish. "The scheduler
never runs" reads like the program. They were the same line of code.

**How to spot it without reading everything:** list the crate's functions with
no caller outside `#[cfg(test)]` and read the *names*. Here the list was
`check_schedule`, `apply_retention`, `delete_snapshot`, `simulate_restore`,
`simulate_create`, `import_snapshots`, `unlock_snapshot` — take, prune, delete,
restore, create, import, unlock. Seven verbs, and they are the seven things the
program is for. A dead-code list that reads like a feature list is not dead
code; it is a missing caller at the top.

### Lesson 405: a walk that drops build directories from its results has already walked them (lane C, 2026-09-25)

A push from lane C sat in its pre-push hook for twenty minutes, in
`scripts/check-cfg-unix.py`, with no child process -- a gate whose docstring
says the whole check takes "about 7 seconds warm". The cargo half was never
reached. `candidate_crates` found manifests with `REPO.rglob("Cargo.toml")`
and then skipped any whose path contained `target` -- a filter on the
*results*, so the walk still descended every `target/` first. That lane's
`target/` had just gained a `-Zbuild-std` userland, and the function ran three
times per push (once for the list, twice more for the summary's count).
`scripts/check-crate-names.py`, also in the hook, had the identical loop.
Both now walk through `gittree.WorkTree.files_under`, which prunes while
walking: 0.3 s for the same 421 manifests git tracks.

The comment in `gittree.WorkTree.files_under` already said this ("descending
into `target/` to throw the results away is minutes of stat() on this tree");
the two scripts predated or bypassed it. **To enumerate the repository, use
the `gittree` seam, never `rglob` from the root: a filter after the walk
decides what you keep, not what you pay for.** How long it takes depends on
the size of an untracked, per-lane directory, so the same gate can be quick
on one lane and stuck on another.

### Lesson 404: a variant's name is not its behaviour, and a tick that trusts the name ticks nothing (lane C, 2026-09-25)

`design.txt` asks for "two options for desktop icon placement: snap to grid,
or place freely". `roadmap.md` §3.4 ticked "free placement + auto-arrange
modes" from the day `gui/desktop/src/icons.rs` was written, and the code
seemed to agree: `ArrangementMode` had a variant called `FreeWithSnap`. It
snapped every drop — "free" only meant the icon went to whichever cell it was
dropped nearest. There was no free placement at all; the other mode re-sorted
by name after every drop, so a drag in it did nothing; and no control let a
user choose either. Three claims — the tick, the variant's name, the list of
modes — and each was true only of its words. Fixed 2026-09-25
(`design-decisions.md` §869).

The tell was one `grep`: every arm that handled the `Free…` variant called
`snap`. **Before trusting a done-mark, find the code path that would behave
differently if the feature were missing — here, a drop in the free mode — and
read that, not the type that names it.**

### Lesson 114: a constant used as a size is a window that ignores its window (lane C, 2026-09-04)

Every layout in `apps/systemrestore` -- 46 lines of it -- read the `WINDOW_WIDTH`
and `WINDOW_HEIGHT` constants directly. The status bar spanned `WINDOW_WIDTH`,
the dialogs centred themselves in `WINDOW_WIDTH / 2.0`, the action buttons were
right-aligned to `WINDOW_WIDTH - ...`.

That is correct exactly once: in a window the compositor happens to grant at
1050x700. Everywhere else the picture is the wrong size in a way that is
*self-consistent* -- every element agrees with every other element, and all of
them disagree with the frame. Widen the window and the status bar stops short of
the edge with the background showing through; narrow it and the action buttons
hang off the side, still perfectly spaced relative to one another.

It survives review because it looks like a layout that has been thought about.
Nothing in the file is inconsistent; the file simply has the wrong idea of how
big it is, once, in a constant, and repeats that idea faithfully in 46 places.

The cure is two fields set from `App::render`'s arguments and a mechanical
substitution, and the reason to do it as a *substitution* rather than by
reasoning about each site is that all 46 are the same mistake -- picking through
them one at a time invites deciding that some of them "are fine as constants",
which is how a layout ends up half-relative and genuinely inconsistent.

The gate for this shape already exists in spirit: `App::render` is *handed* the
width and height rather than being expected to ask, precisely so that an app
which ignores them has to ignore an argument in front of it. This one did.
