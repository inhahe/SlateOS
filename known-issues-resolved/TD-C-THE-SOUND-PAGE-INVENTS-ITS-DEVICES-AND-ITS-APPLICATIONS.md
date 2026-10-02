## TD-C-THE-SOUND-PAGE-INVENTS-ITS-DEVICES-AND-ITS-APPLICATIONS -- FIXED 2026-09-14

**Date:** 2026-09-14. **Lane:** C. **OPEN.**

**In short:** the Settings app's Sound page shows two speakers, a microphone and
four running programs with individual volume sliders. None of them exists. The
names are written into the source as constants, the machine is never asked what
audio hardware it has, nothing is ever asked what programs are playing sound,
and moving any slider changes a number in memory that nothing reads. A user
would set their volume, close the window, and find nothing had happened.

**The invented lists**, `apps/settings/src/main.rs`:

| list | contents | reality |
|---|---|---|
| `output_devices` | "Speakers (Built-in)", "HDMI Audio Output" | no device enumeration exists anywhere in the tree |
| `app_volumes` | "System", "Browser", "Music Player", "Video Player" | hardcoded. There is no browser in this tree at all; `musicplayer` and `videoplayer` exist but are not being asked |

**And nothing downstream.** `apps/mixer` exists and reads no configuration
file. There is no audio service under `services/`. So even a slider that
persisted correctly would have nothing to persist *to*.

**This is the Mouse page's defect, in a page that did not restrain itself.**
`TD-C-THE-MOUSE-SETTINGS-PANEL-REACHES-NOTHING` is the same finding, and the
Mouse page's response was to offer exactly one control -- the one with a
consumer -- and say so in a comment: *"Each gets its control here when it gets
a consumer, and not before."* The Sound page offers seven controls and a
per-application section, and has no consumer for any of them.

**It is also the third fictional catalogue found in this tree**, after
`apps/fileassoc`'s eight programs that did not exist (fixed 2026-09-14 by
pointing it at eight real binaries) and the four disagreeing lists of installed
programs in C-Q20. Fiction in a UI is not a cosmetic problem: it is
indistinguishable from a working feature until someone tries it, which is the
worst possible time to find out.

### What to do about it, and why this is not simply "delete the controls"

Three options, and the choice is user-visible:

**A. Make the page honest now.** Empty the two invented lists and say what is
true: no audio devices detected, no applications playing. The sliders that
remain are the ones with nothing behind them, so they go too, leaving a page
that says the feature is not built.
*What changes:* a user sees an empty Sound page instead of a convincing one
that does nothing.

**B. Leave it until there is an audio service, then wire it.**
*What changes:* nothing today; the page goes on inviting a change it cannot
make, for as long as that takes.

**C. Keep the controls, mark them visibly unavailable** -- greyed rows with one
line saying audio support is not built yet.
*What changes:* the user can see the intended shape and cannot be misled by it.
`guitk::disabled` already exists for exactly this.

**DONE: C, on 2026-09-14.** The rows are drawn and inert -- "No devices
detected", "Unavailable" -- through a new `unavailable_row` that passes no hit
band, so they cannot be clicked *by construction* rather than by a handler
remembering to refuse them. A note above them says audio is not wired up.

The invented data is gone with them: `AudioDevice`, `AppVolume`, nine state
fields, seven control ids and `round_u8` (whose every caller was a sound
slider). Two tests went with it. A third -- a good test of slider *drag
mechanics* that merely used the volume slider as its vehicle -- was re-pointed
at `TextSize` rather than deleted, since 50..250 gives the same round numbers
to assert on.

One consequence worth recording, because it is the pattern this whole sweep is
about: removing the indexed per-application sliders made `slider_raw` unable to
fail, and clippy said so. It had returned `Option<f32>` for a case that could
no longer arise, which teaches every caller to handle something impossible.
Both it and `slider_fraction` are plain `f32` now.

**Original recommendation, kept for the reasoning: C**, and it is the one that fits the tree's own precedent
best -- the Mouse page hid what it could not deliver, but a whole subsystem is
a different case from one slider, and a page that shows its shape without
pretending to work is more useful than an empty one. A is more honest than
today and less useful than C. B is what happens by default and is the only one
that can mislead somebody.

**If never answered:** nothing degrades, but the page stays a working-looking
lie, and it is the kind that surfaces as "I set my volume and it did not
stick", which sends the finder to the audio code rather than to the page.
