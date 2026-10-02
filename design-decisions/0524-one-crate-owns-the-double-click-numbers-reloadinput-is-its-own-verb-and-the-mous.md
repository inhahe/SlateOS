## 524. One crate owns the double-click numbers, `ReloadInput` is its own verb, and the Mouse page offers exactly the settings something consumes

**Date:** 2026-08-22
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** The desktop had a Mouse settings panel whose double-click slider
moved, showed a number, saved nothing and reached nothing — the value was gone
at the next login and had never been seen by the compositor at any point in
between. Fixing it needed four pieces, and three of them turned on judgement
calls rather than mechanics: which process *owns* the range of legal values,
whether the compositor should be told "input changed" or just "something
changed", and how much of the mouse configuration the new page is allowed to
show. This records those.

Terms used below, once each: *the compositor* is the display server — the
process that draws window frames and decides whether two clicks on a title bar
are one double click. *`input.yaml`* is the user's mouse and keyboard
preferences file. A *verb* here is one of the fixed set of request kinds a
client may send the compositor over its socket.

### The problem, concretely

`gui/desktop/src/mouse_settings.rs` drew a double-click slider bound to a
`MouseConfig` that existed only in the shell's memory. Nothing wrote it to disk
and nothing read it back. Meanwhile the compositor timed double clicks against a
private constant of its own. Two numbers, no connection.

Four pieces were needed: **(a)** a shared `gui/inputsettings` crate holding the
model and the file format; **(b)** a `ReloadInput` control verb; **(c)** a real
Mouse page in `apps/settings`; **(d)** the compositor re-reading (a) when it
receives (b).

### Decision 1: `inputsettings` owns the range, and the other two read it

**What was true before.** Three processes each held their own copy of the
double-click bounds. `inputsettings` clamped a value being stored; the
compositor clamped the value it had been handed before timing anything against
it; the Settings slider's two ends *were* that range, written as literals. The
three copies did agree.

**Why that was still wrong.** Nothing anywhere would have failed if one of them
had moved. A slider whose track runs wider than the setter's clamp has a dead
zone at each end — the handle slides, the number stops — and no test on either
side of the boundary can see it, because each side is individually correct. The
whole point of the reload path being added alongside was that the number the
user chose, the number the file clamps to, and the number the compositor
compares two timestamps against are *one* number.

**What was decided.** `MIN_DOUBLE_CLICK_MS`, `MAX_DOUBLE_CLICK_MS` and
`DEFAULT_DOUBLE_CLICK_MS` became `pub` in `inputsettings`, the crate that owns
the file. The compositor's private copies were deleted, and
`SliderId::DoubleClickMs::range()` returns the constants rather than literals.
The ordering invariant (`MIN < DEFAULT < MAX`) is a `const _: () = assert!(…)`
rather than a test, because there is no run in which it could hold here and fail
somewhere else.

*The alternative was* leaving the literals in place and adding a test that
compares them. Rejected: that is a test which exists only to detect a
duplication that need not exist, and it fires *after* the divergence rather than
preventing it.

*The cost:* `inputsettings` is now a dependency of the compositor. That is not
free — a display server linking a settings crate is a real coupling — but the
crate depends only on `guitk`, `settingsfile` and `yamldoc`, so there is no
cycle, and the compositor was already reading the file's *contents*. It was
reading them with its own private idea of what they mean, which is the worse
coupling of the two: an undeclared one.

### Decision 2: `ReloadInput` is a second verb, not a payload and not a shared one

Two sub-decisions, pulling in opposite directions.

**Why not a payload.** `ReloadAppearance` and `ReloadInput` carry no data, and
that is the entire security argument for both. A verb that *set* the values
would let any process able to open the display socket restyle the machine or
swap the user's mouse buttons — or make a double click a two-second affair. A
verb that merely *notifies* makes the compositor re-read the *user's own file*,
so the worst a hostile client achieves is making it read a file it already
trusts, at some rate. `control.rs` asserts the emptiness on the encoded bytes
rather than trusting the enum's shape, so the day someone adds "just a corner
radius, to save a file read", that assertion fails and says why.

**Why not one shared verb.** A single `ReloadSettings` would be less code. It
was rejected because the two reloads have very different costs: an appearance
reload sets `full_recomposite`, repainting every window on screen, while an
input reload changes a `Duration` that no pixel depends on. Sharing the verb
means a user dragging the double-click slider repaints the entire desktop on
every motion event, and a user picking an accent colour makes the compositor
re-read its pointer configuration. Both are invisible in ordinary testing and
both are wrong.

The tests therefore assert the *negative* in each direction — a double-click
change must produce no `ReloadAppearance`, and a colour change no `ReloadInput`
— because a `ReloadInput` accidentally wired to the appearance path passes every
positive test anyone would think to write.

### Decision 3: the compositor applies only what it consumes

`reload_input` reads the *whole* file — the model has one owner, and parsing
half of it is how two readers begin to disagree — and applies the one setting
the compositor is actually the consumer of: the double-click window.

Pointer speed, acceleration, button mapping and scroll mode are deliberately
read and dropped. They have to be applied where raw device deltas arrive, and
the compositor has no local input source yet (`known-issues.md`
`TD-COMPOSITOR-HAS-NO-LOCAL-INPUT`); cursor size and scroll mode belong to the
toolkit.

*The alternative was* storing them in the compositor now, ready for later.
Rejected as strictly worse than ignoring them: a stored value invites a getter,
and a getter would report that a preference was in force while the pointer went
on behaving the old way. Ignoring a setting is a gap; reporting an unapplied one
is a lie — and it is the same lie this whole task was filed to remove.

### Decision 4: the Mouse page shows one control, and that is a finished state

`input.yaml` holds fifteen mouse fields and three keyboard ones. The new page
offers one.

That is not an unfinished page — it is the page refusing to repeat the defect it
was written to fix. A pointer-speed slider today would save a value to a file,
look exactly as though it had worked, and change nothing: precisely the bug
described at the top of this entry. Each setting gets its control when it gets a
consumer, and not before.

This is asserted rather than merely commented.
`the_mouse_page_offers_only_settings_that_reach_something` checks the page's hit
bands as an *exact set*, not as "contains the double-click slider" — because a
control added without a consumer is the same bug, and a containment check cannot
see it.

*The tension worth naming:* a settings panel exposing one of eighteen settings
looks broken to a user who came looking for pointer speed, and "looks broken" is
a real cost. It was accepted because the alternative is not "looks complete" but
"looks complete and lies", and because the fix — wiring a consumer — is work
that was going to be needed anyway.

### How this was verified

Every test added across the four pieces was put through a reintroduction proof:
`scripts/reintro-mouse-page.py` (11 defects) and
`scripts/reintro-reload-input.py` (10 defects) each reintroduce, one at a time,
the exact bug some test claims to guard, then check that the suite goes red and
names it back. Both restore by unconditional byte-snapshot write-back verified
by SHA-256, per §522.

That found one thing worth recording.
`a_hand_edited_input_file_cannot_make_a_double_click_impossible` carried a
comment saying the value was clamped "twice over", by `InputSettings::read_from`
and again by `set_double_click_ms` — but its assertion could not distinguish the
two, because the first clamp alone satisfies it. Deleting the compositor's clamp
left that test green. The clamp *is* guarded, by
`the_double_click_interval_is_clamped_to_a_performable_range`; the comment now
says which layer each test proves. A test whose comment claims more than its
assertion is how a defence quietly stops being defended.
