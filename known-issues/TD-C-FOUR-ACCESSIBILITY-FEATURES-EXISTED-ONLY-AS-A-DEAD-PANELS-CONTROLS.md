## TD-C-FOUR-ACCESSIBILITY-FEATURES-EXISTED-ONLY-AS-A-DEAD-PANELS-CONTROLS

**Date:** 2026-09-08. **Lane:** C.
**Where:** nowhere any more — that is the point. They were in
`gui/desktop/src/accessibility_settings.rs`, deleted under
`design-decisions.md` §815.

**In short:** the shell carried a 2 019-line accessibility settings panel that
nothing ever opened. §815 says screens you open move to the Settings app and
the shell's copies go, so it went. Twelve of the sixteen things it offered
already exist in the Settings app. Four do not, and this is the record of them
so that deleting the panel does not quietly delete the idea.

**The four, and what each would actually take.** None of them was working
before — the panel was unreachable, so every one of these was a control with
no reader. Nothing regressed.

| feature | implementation anywhere? | what it needs |
|---|---|---|
| **Magnifier settings** | The feature exists (`apps/magnifier`), the settings do not reach it | A settings page *and* a config route. `a11y::MagnifierConfig` is read only inside `a11y.rs`; `apps/magnifier` keeps its own and never consults it. So this is the same disconnection as the sticky-keys one, one layer along. |
| **Auto-click / dwell click** | **None.** Zero matches for `auto_click` or `dwell` outside the deleted panel | The feature first: hold the pointer still for *n* ms and a click is synthesised. That is compositor work, in the pointer path, and it wants the same treatment sticky keys just got. |
| **Flash screen on system sound** | **None** | The feature first. It needs a signal that a system sound played, which nothing currently emits. |
| **Closed captions** | **None.** The 254 `caption` matches in the tree are window titles and UI subtitles, not subtitle tracks | A media-subtitle pipeline, which is a far larger job than a settings toggle and belongs to whatever plays video. |

**Why they were not ported instead.** §815 says "moves to the Settings app",
and for the twelve that had somewhere to move to, that had already happened.
Porting these four would have meant adding four controls to the Settings app
that read nothing and change nothing — which is exactly the fault the whole
of this session has been spent removing, and it would have put them somewhere
a user can actually reach, making it worse rather than better.

**The order to do them in, if they are wanted:** magnifier first (the feature
exists and only the wiring is missing), then auto-click (self-contained, in
the compositor's pointer path, and the closest analogue to the sticky-keys
work just finished). Flash-screen needs a system-sound event to hang off.
Captions are not really an accessibility-settings job at all.
