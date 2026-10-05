## 1304. Artifact recovery: a compositor-owned Ctrl+Super+R that redraws everything from scratch

**Date:** 2026-09-25
**Lane:** F
**Decided by:** Claude (autonomous). The feature and its shortcut are the
design's own (`roadmap-detailed.md` §3.3, "Full-screen redraw /
artifact-recovery path", which names Ctrl+Super+R); what is recorded here are
the shapes chosen to build it.

**In short:** if something stray is left on the screen — a tooltip that
outlived its program, a patch of an old frame — pressing Ctrl+Super+R now
throws away everything the desktop believes about what is on screen and draws
it all again from scratch: windows whose program has gone are removed, the
display is reset, and every program is asked to draw its windows whole. A
shell can ask for the same thing over the display protocol. Everyday drawing
is unchanged; this is the way back when it has gone wrong, and a way to tell
whose fault it was — an artifact that survives it is in the compositor itself.

### What it does, in order (`Server::recover`)

1. Drops every window no live client owns (`Compositor::drop_windows_without_owner`),
   counted in `ServerStats::orphans_swept` — non-zero means ordinary reaping
   missed one.
2. Forgets the compositor's records of the screen
   (`Compositor::reset_for_recovery`): the whole screen becomes damage, the
   frame history that partial frames trust (§1300) is emptied, and the
   framebuffer is declared stale so a direct-scanout frame cannot stand in.
3. Resets the display (`Present::reset`): the DRM presenter programs every
   live head's mode again and forgets what each buffer holds; the host window
   drops its staged copy.
4. Forgets the server's own copies (filtered frame, drawn pointers) and forces
   the next frame onto a new serial, so no presenter can skip copying it.
5. Asks every client to draw its windows whole: a new `RPNT` frame
   (`guiremote::repaint`), which oswindow turns into `Dispatch::Repaint` and
   `app::drive` into a full redraw.

### The choices with two sides

1. **The chord is the compositor's own, checked before the grab table.**
   *For:* recovery is for when something has gone wrong, and the shell that
   holds every other desktop shortcut may be the thing that went wrong; a
   grabbable chord could also be taken by any client. *Against:* one chord no
   application can ever use. Accepted, and the match is exact (Ctrl+Super+R
   and nothing else held) so that Ctrl+Shift+Super+R stays an application's.
   The R's repeats and release are swallowed with its press, so the focused
   window never sees half a chord.
2. **The client repaint is a frame of its own, not an input event.** *For an
   event:* a per-event handler would see it unchanged. *Against, and
   decisive:* `guitk::event::Event` is what the user did to a window, and every
   widget matches on it; a repaint is the display asking, which already has
   frames of its own (window lists, the tray). A fake `Resize` to the same
   size was also rejected — it would work, and it would be a lie every
   application had to be told was not a resize.
3. **Orphans are found from the server's live connections, not by asking
   every client which surfaces it owns.** The design's full form re-enumerates
   by asking. The connection-level sweep catches the case the design is
   written about — a surface whose owner died — with no round trip and no
   timeout policy for a client that does not answer; asking each client, and
   deciding what to do about a live client that disowns a window, is left for
   when a transport can attest who a client is (the same gap as
   `TD-C-ANY-CLIENT-CAN-READ-EVERY-WINDOW-TITLE`).
4. **`RecoverDisplay` goes through the shell privilege seam**
   (`ClientLink::require_shell`), since it redraws and may drop other clients'
   windows. Today that seam admits everyone, as it does for every shell
   request; the day it checks, this is covered.

### Not done here

Transient-surface TTLs (tooltips that need a heartbeat to stay up) are the
design's other orphan defence and are not built; hardware overlay and cursor
planes are not reset because nothing in this tree uses one yet — the pointer is
drawn in software (§1301).

### How it is held

`ctrl_super_r_asks_for_recovery_and_no_window_sees_the_r`,
`a_chord_with_another_modifier_is_not_recovery`,
`a_grab_on_the_recovery_chord_does_not_take_it`,
`recovery_drops_orphans_and_asks_every_client_to_repaint`,
`the_frame_after_recovery_is_drawn_whole_under_a_new_serial`,
`a_shell_can_ask_for_recovery_over_the_wire`,
`the_loop_recovers_when_the_chord_is_pressed`, the DRM
`a_reset_programs_every_mode_again_and_forgets_what_the_buffers_hold`, the
`RPNT` codec's own suite, and oswindow's `Dispatch::Repaint` and
`app::drive` tests.
