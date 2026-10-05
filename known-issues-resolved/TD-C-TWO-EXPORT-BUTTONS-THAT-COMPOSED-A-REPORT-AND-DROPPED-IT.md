## TD-C-TWO-EXPORT-BUTTONS-THAT-COMPOSED-A-REPORT-AND-DROPPED-IT -- FIXED 2026-09-15

**In short:** `apps/benchmark` and `apps/sysinfo` each had an Export button.
Pressing either built the whole report in memory and then threw it away, so no
file was ever written. sysinfo went further and printed "Exported system info
to file" afterwards, and its Copy button printed "Value copied to clipboard"
without copying anything. sysinfo's two toolbar buttons were worse still: they
were drawn but never wired to the mouse at all, so clicking them did nothing
whatsoever.

**Date:** 2026-09-15. **Lane:** C.

**The shape, in one line of source.** Both apps, at both call sites:

```rust
if !self.history.is_empty() {
    let _report = self.export_report();   // composed, then dropped
}
EventResult::Consumed
```

`let _name = ...` is the discard that names what it discards, which is why it
reads as deliberate. The `Consumed` is the part that makes it invisible: the
framework is told the event was handled, so nothing downstream can notice that
nothing happened.

| control | claimed | did |
|---|---|---|
| benchmark Ctrl+E / Export button | nothing (silent) | built the report, dropped it |
| benchmark Export with no history | nothing (silent) | nothing |
| sysinfo Ctrl+E | "Exported system info to file" | built the report, dropped it |
| sysinfo Ctrl+C | "Value copied to clipboard" | nothing |
| sysinfo Export button (mouse) | nothing (silent) | nothing -- never hit-tested |
| sysinfo Copy button (mouse) | nothing (silent) | nothing -- never hit-tested |

**Why this is a recurrence and not a new finding.** `sysinfo` is an app already
repaired once in this sweep, under
`TD-C-SEVERAL-APPS-DISPLAY-DATA-THAT-NOTHING-PRODUCES`: its fabricated hardware
readings were removed and the entry closed. **That audit looked at what the app
*displayed* and never at what its *controls claimed*.** The two are separate
surfaces and a pass over one reads exactly like a pass over both -- the app was
on a FIXED list while three of its controls were still lying. When an app is
revisited, the question is which surface was audited last time, not whether it
appears in the list.

**The comment that was the tell.** sysinfo's clipboard handler was headed
`// Ctrl+C = copy selected value (simulated)`. The word was right there, in the
file, for however long it stood. **A note to the next programmer was standing in
for a sentence addressed to the user** -- the parenthetical makes the code
honest to a reader while the string it guards stays false to the person holding
the machine. A `(simulated)` in a comment beside a user-visible claim is worth
grepping for on its own.

**The fix.** Both now route through `guitk::dialog::FilePicker` -- the same door
seventeen applications share -- and write through `safeio::write_str_atomically`.
The status lines report what was actually written (`Wrote N bytes to <path>`) or
why it failed. sysinfo's clipboard message now says nothing here can reach the
clipboard and names Ctrl+E, which does work: **a false denial is cheaper than a
false promise but it is the same defect pointed the other way.** benchmark grew
a `status_message` because its status bar derived its entire text from the
progress phase, leaving an action nowhere to report a result -- which is part of
how a discarded export stayed invisible. Exporting an empty history now says
so instead of going quiet.

sysinfo's toolbar geometry is now `SysInfoState::toolbar_layout()`, used by both
the drawing and the hit-test, so a moved button cannot leave its clickable
region behind. A test asserts the drawn label falls inside the rectangle the
click tests.

**Verified by sabotage**, six claims, each broken in turn with an edit that
still compiles: the write silenced, the picker undrawn, the hit-test displaced,
the picker made to swallow every event, the empty-history notice removed, and a
`Tick` under an open picker. All six went red.
