## C-NETMANAGER-CLICKED-ROWS-THAT-WERE-NOT-ON-SCREEN (lane C, 2026-08-26) — FIXED 2026-08-26

**In short:** in the Network Connections window, clicking the status bar along
the bottom could select a network profile that was scrolled off the bottom of
the list and not visible anywhere on screen. The profile list drew as many rows
as there were profiles and let the window's clipping hide the overflow — which
worked for what you *saw*, but not for what you could *click*. With 24 profiles
in a small window, twenty rows were clickable below the bottom edge of the
window entirely, and one straddled the status bar.

**Where it lived:** `apps/netmanager/src/main.rs`. The app had its own private
`Frame` type — the thing that records where each control was drawn so a click
can be matched back to it — and that copy did not track the clip stack.
`render_tab_profiles` has no visible-row limit; it loops over every profile and
relies on the enclosing `PushClip` to stop the overflow. The compositor honours
that clip for drawing. The hit-test did not honour it at all, because the copy
of `Frame` in this app had never been taught about clips.

**Why it existed:** the same `Frame` was written twice, once in
`apps/archivemanager` and once here. The archivemanager copy grew clip tracking
when a scrolling file list needed it. The netmanager copy did not, because
nothing there looked like it needed it — and the one place that did, an
unbounded profile list, was not the place anyone was looking. Two copies of an
invariant, and the bug is in whichever one you are not currently reading.

**The fix:** `gui/toolkit/src/frame.rs` now holds a single generic
`Frame<T>` and `Rect`, and both apps use it. It intersects every recorded rect
with the clip in force and drops it entirely if nothing is visible. It also
tracks `PushTranslate`/`PopTranslate`, which *neither* private copy did: a
scrolling pane that draws its rows inside a translation is painted somewhere
else entirely, so a rect recorded verbatim would be clickable at coordinates
nothing was drawn at. Both stacks mirror the compositor's own semantics
(`ClipStack`/`TranslateStack` in `gui/compositor/src/lib.rs`) so the ink and the
click target cannot end up in different places.

**Regression tests** (both confirmed to fail when the clip trimming is mutated
out of the toolkit):

| Test | Catches |
|---|---|
| `a_list_row_past_the_bottom_of_its_panel_is_not_clickable` | any `Profile` rect reaching past the panel, and all rows being reachable when only some are drawn |
| `clicking_the_status_bar_does_not_select_an_off_screen_profile` | the user-visible symptom, clicking 2px inside the status bar |

Plus 19 unit tests and a doctest on `guitk::frame` itself.

**What this means for the other 123 unwired programs:** each one needs the same
"record what you draw, hit-test by reading it back" machinery, and each would
have been a fresh transcription of it. They now get a reviewed one with an
import. Net effect on the two apps that had copies: 107 lines deleted.
