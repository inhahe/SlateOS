## MODULE 86 (lane C, 2026-08-25) -- the calendar's event store, its file format and its reminders

**In short:** modules 68-70 swept how the calendar *draws* itself and module 82
swept its date arithmetic, so what was left unasked was the bookkeeping
underneath: adding, finding, updating and deleting events; the query that asks
"which events fall in this range"; the plain-text save/load format; and the
whole of the reminder manager (fire, snooze, dismiss). Seventy-six of the file's
114 tests had never been named by any defect. Twenty-six deliberate faults,
nineteen caught. **All seven escapes are the same shape**: a comparison the
suite exercises only with values that sit clearly on one side of it, never
exactly on it. Closing them found a real bug in production code that no defect
had modelled.

**The pass, 26 defects:**

```
26 defects: 19 caught, 7 escaped, 0 never asked, 0 under-caught,
7 under-declared
```

Caught: ID assignment, the delete's report of whether it deleted anything, the
delete's predicate itself, `update_event`'s found/not-found return, `get_event`
by ID versus by position, the range query with its overlap test removed
entirely, colour parsing (all three channels), the importer's mid-input record
flush, the `all_day` flag, the recurrence keyword table, the `color:` key's
trailing space, the reminder's lead-time direction and magnitude, the
already-fired filter, the snooze direction, the dismiss loop's `break`,
`dismiss_all`'s lack of one, `active_count`, and `clear_dismissed`'s polarity.

### Lesson 38: an ordering comparison is only proved by a fixture that sits exactly on it

Three escapes -- `G`, `H` and `P` -- are one line each, and all three are the
same edit:

```rust
-        if event.start_timestamp < range_end && event.end_timestamp > range_start {
+        if event.start_timestamp <= range_end && event.end_timestamp >= range_start {
```

```rust
-        if occ_start >= range_end {
+        if occ_start > range_end {
```

The rule is mechanical and worth stating as one: **for a test to distinguish
`a < b` from `a <= b`, the suite must contain a case where `a == b`.** No amount
of coverage elsewhere substitutes. A suite in which the two operands are never
equal cannot see the difference, because on every input it runs the two
expressions agree.

The calendar's fixtures never produce that equality by accident, and the reason
is structural rather than careless: events are written at plausible human times
-- 09:00, 10:00, 14:00, 23:00 -- while every range boundary in the suite is a
midnight, because `events_for_date` is the only caller and it hands
`events_for_range` exactly one midnight to the next. Two families of value that
never collide.

**What that costs a user.** `events_for_date(y, m, d)` calls
`events_for_range(midnight, next_midnight)`. Under defect `G` or `H` an event
that ends exactly at midnight is reported on the following day as well, and one
that begins exactly at midnight is reported on the preceding day -- so every
all-day event, which is precisely the kind anchored to midnight, is drawn twice.
Under `P` a daily series is one occurrence too long at its end. None of these
is a crash; all of them are a calendar that quietly shows the wrong thing.

**The fix is a fixture built around the boundary, not around a plausible day.**
`the_range_is_half_open_at_both_ends` places four events: one ending exactly at
`range_start`, one beginning exactly at `range_end`, and two that overlap the
range by a single second at each end. The two one-second overlappers are the
part that matters -- without them, a "fix" that made *both* ends exclusive would
pass, and the test would have traded one wrong answer for another. The
recurrence sibling, `a_recurring_series_is_half_open_at_the_end_too`, asserts
the occurrence count *and* separately asserts that `events_for_date` on the
excluded day still returns 1, so the guard cannot be satisfied by dropping the
occurrence altogether.

### Lesson 39: "not found" from an empty container is not evidence that lookup works

Defect `E` changed `update_event`'s search:

```rust
-        if let Some(e) = self.events.iter_mut().find(|e| e.id == id) {
+        if let Some(e) = self.events.iter_mut().find(|e| e.id >= id) {
```

The suite has a test named for exactly this -- `event_store_update_nonexistent`
-- and it escaped, because that test asks an **empty** store for ID 999. An
empty store answers "not found" to every predicate, correct or not; the
assertion holds for `==`, for `>=`, for `<`, and for a search that always
returns `None`.

The state that actually distinguishes them is the one a *delete* leaves behind:
a store that holds ID 2 but not ID 1. `>=` then finds event 2 when asked for
event 1, and the user's edit lands on the wrong appointment.
`an_update_to_a_deleted_event_does_not_land_on_a_later_one` builds that state
and asserts three things -- that the call reports false, that the closure never
ran at all (via a flag it would have set), and that the surviving event's title
is untouched -- because "returned false" alone would still pass a version that
mutated something and then reported failure.

**Generalising:** a negative test over an empty collection is nearly content-free.
Its assertion is satisfied by the correct implementation, by every incorrect one,
and by a stub. Negative tests earn their keep only against a populated container
whose contents are *close* to the thing being looked for.

### Lesson 40: closing an escape is an audit, not a formality

The escape that mattered most was `W`, which deleted a `break`:

```rust
             if r.event_id == event_id && !r.dismissed {
                 r.dismissed = true;
-                break;
             }
```

Writing the test to catch it -- "dismissing one reminder must not cancel the
event's others" -- required stating something the defect did not model: what
happens on the *second* call. `W` is a statement about one dismissal; the test
had to be a statement about a sequence, because that is the only way to observe
that the loop stopped in the right place rather than merely that it stopped.

That extra assertion failed, and it failed against the *unmodified* production
code. `ReminderManager::dismiss` matched on the event ID alone:

```rust
-            if r.event_id == event_id {
+            if r.event_id == event_id && !r.dismissed {
                 r.dismissed = true;
                 break;
             }
```

Without the `!r.dismissed` half of the test the loop stops on the reminder it
retired *last* time, so every call after the first is a no-op. An event with two
reminders -- a day-before notice and an hour-before one, which is the ordinary
configuration -- could never have its second reminder dismissed at all: it fires,
you wave it away, and it fires again forever. `snooze` already carried the guard;
`dismiss` did not, and nothing had ever asked. The fix is in production code and
carries its own defect (module 87, `A`) so the guard is now proved rather than
merely present.

**The lesson is about the order of operations.** It is tempting to treat a
closed escape as bookkeeping -- write the assertion the defect implies, watch it
go red under the patch and green without it, move on. But the assertion a defect
implies is usually narrower than the assertion a *reader* would want, and the
gap between them is where untouched bugs live. Write the test the behaviour
deserves, not the test the defect requires, and run it against unmodified code
before you run it against the patch.

### The other two escapes are earlier lessons recurring

`O` changed the colour writer's padding:

```rust
-        text.push_str(&format!("color: {:02X}{:02X}{:02X}\n", r, g, b));
+        text.push_str(&format!("color: {:X}{:X}{:X}\n", r, g, b));
```

Every colour in every fixture has all three channels at or above `0x10`, so the
`02` padding never has anything to do. This is **Lesson 35** again -- a
mitigation is untested until a fixture creates the hazard it mitigates. The
closing test, `a_dark_colour_keeps_its_leading_zeros_through_the_file`, uses
`0x0A0B0C` and asserts both the literal text written and that a re-import
recovers the colour, because the written form is what another program would
have to read.

`V` and `W` are both **Lesson 36** -- a compound condition whose operands never
disagree in any fixture. No test in the suite ever dismissed a reminder and then
snoozed or re-dismissed the same event, so `&& !r.dismissed` was decoration.
`a_snooze_moves_the_live_reminder_not_the_dismissed_one` creates that
disagreement directly: it dismisses the day-before notice, snoozes, and asserts
the *live* reminder is the one that moved, by exactly
`SnoozeDuration::FiveMinutes.secs()`.

### Result

```
27 defects: 27 caught, 0 escaped, 0 never asked, 0 under-caught,
14 under-declared
```

Twenty-seven rather than twenty-six because the confirmation pass includes
module 87's single defect, which proves the `dismiss` guard the module forced
into existence. The fourteen under-declarations were folded back into the
palette so each defect names every test that catches it.

`gui/desktop/src/calendar.rs` is now 65 of 120 tests never-asked, down from 76 of
114. The swept corpus stands at **2949 tests, 2314 unproved -- 21.5 % proved, 0
dangling, 204 single-prover**; the figure recorded above after module 85 was
21.0 %.
