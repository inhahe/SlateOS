## TD-C-A-RECORDER-THAT-RAN-A-CLOCK-OVER-NO-AUDIO -- FIXED 2026-09-15

**In short:** `apps/soundrecorder` showed a recording timer counting up, a
count of automatic saves, and the word "Recording", while capturing no sound
at all and writing nothing to disk. Someone recording an interview would have
watched all three, pressed Stop, and had nothing.

**Date:** 2026-09-15. **Lane:** C.

`process_samples` is the door audio comes in through. **Nothing in production
calls it** -- this crate has no audio device access, no `std::fs`, and no
syscall that could find a capture device -- so the sample buffer stayed empty
for the whole of every take. Everything around it ran anyway:

| what the window showed | what was true |
|---|---|
| a clock climbing through 00:03:47 | `tick` advanced a timer that no audio fed |
| an auto-save count going up | `check_auto_save` fired on schedule and wrote nothing |
| the state "Recording" | nothing was being recorded |
| a device menu with three microphones | `mock_devices()`, enumerated from nothing |
| a time-remaining countdown of `-01:26:48` | 1 GB of free space that nothing measured, divided by the bitrate |

**This is the worst finding of the sweep on the axis that matters, and it is
not close.** Every other fabrication here misreports something that still
exists to be checked: a wrong speed can be re-measured, a wrong partition list
can be re-read, a wrong scan can be re-run. **This one destroys an
unrepeatable event.** And it destroys it while displaying the specific
reassurance a careful person would look for -- an auto-save count is what you
check *because* you are worried about losing the take.

**The one honest signal on screen was the flat VU meter**, and a flat VU meter
is indistinguishable from a quiet room. That is worth sitting with: the
program did have a true indicator, and it was the one no user would read as an
error.

**The fix is a refusal at the start of the take**, not a warning during it. A
take that begins and then reports trouble has already cost the user the thing
they were recording. Because `tick` and `check_auto_save` are both guarded on
the `Recording` state, refusing to enter it stops the clock and the save
counter too; the test drives a minute of ticks and asserts both stay at zero.

**The second fabrication in the same file is the time-remaining countdown.**
`RecordingTimer::available_bytes` was a `u64` initialised to `1_000_000_000`.
A `u64` has no way to say "unmeasured", so the only answer it could give was a
wrong one. It is `Option<u64>` now and the field reads `--:--:--`. Deliberately
**not** `00:00:00`, which says the disk is about to fill and the take is about
to stop, and **not** an omitted field, which is read as however much room you
like. The countdown also drops its leading minus when unknown, because
`-​--:--:--` reads as a negative duration rather than an absent one.

**The banner says "This is not a missing microphone -- this program has no way
to open a capture device at all."** The first line on its own reads as a
hardware fault and sends the user hunting for an unplugged cable. The
distinction between *this machine has no microphone* and *this program cannot
look for one* is the whole content of the fix, so it has to be on screen.

**A third defect surfaced during the fix, and it was mine:** `blocked_reason`
was set on a refused press and rendered by nothing, caught by
`check-fields-written-never-read`. Third time in one day that gate found a
field of mine that production writes and nothing draws. The fix is the one
`netscan`'s Send button had taught an hour earlier -- **a press that changes
nothing visible reads as a broken button** -- so the reason is drawn under the
transport even though the banner above already explains the situation.

13 of 133 tests rested on the invented device list and moved to a
`with_mock_input()` fixture.
