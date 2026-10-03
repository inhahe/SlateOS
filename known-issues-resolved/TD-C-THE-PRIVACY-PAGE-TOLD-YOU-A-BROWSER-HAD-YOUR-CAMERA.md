## TD-C-THE-PRIVACY-PAGE-TOLD-YOU-A-BROWSER-HAD-YOUR-CAMERA -- FIXED 2026-09-15

**In short:** the Settings app's Privacy page listed nine applications and
showed, with green ticks and red crosses, which of them could use your
location, camera and microphone. None of those applications exist on this
system, nothing on this system asks anything whether an application may use a
device, and every tick and cross was written into the source as a constant.
A privacy page is the one screen a user is entitled to believe.

**Date:** 2026-09-15. **Lane:** C.

**What it claimed.** Three per-device sections, each with a master toggle and a
list of applications with allow switches: Location (Maps, Weather, Camera,
Browser), Camera (Video Chat, Browser, Social Media), Microphone (Video Chat,
Voice Recorder, Browser). Then a Capabilities sub-page drawing a summary table
over nine named programs and four permissions -- a tick in green, a cross in
red, a dash where a program had not asked. `Browser` was ticked for Camera.
Also a diagnostic-data-collection level to choose, and a "Clear Activity
History" button.

**Why this could not be finished rather than removed.** `design.txt` specifies
capability-based security with no ambient authority: a program may use a device
because it holds an unforgeable handle to it, not because a central table has
its name ticked. A per-application permission list is therefore not this
system's model half-built -- it is a different system's model, borrowed from
Windows and Android. Completing the page would have meant building a permission
store that the kernel does not consult and could not be made to consult without
abandoning the capability design.

**The detail that shows the cost of leaving it.** One of the tests deleted with
this change was
`test_every_per_app_permission_switch_is_clickable`, and its comment reads:
*"Only the Location list had a handler; Camera, Microphone and Background were
drawn and inert."* Somebody had already found a bug in this page and fixed it
-- real effort spent making fabricated switches respond, on a page where
responding was never the problem. That is what a convincing fabrication costs:
not just the user's trust when they find out, but the next person's afternoon.

**The fix.** The page now says that this system does not record per-application
permissions, and why -- in the plain terms the design deserves: *"a program
here reaches a device by holding a handle to it, which it can only have been
given -- there is no central table of names to tick, and nothing has authority
simply because of what it is called."* The dead state went with it: the four
app lists, the three master toggles, `PermissionKind`, `AppPermission`,
`DiagnosticLevel`, `build_permission_list`, `render_capabilities_summary`,
`PageSink::app_toggle_row`, four `ToggleId` variants and one `DropdownId`.

**Still outstanding:** `gui/desktop/src/privacy_settings.rs` holds a second
copy of the same borrowed model -- `PermissionKind`, `AppPermission`,
`PrivacySettings`, with `is_allowed` and `revoke_all` and nothing calling
either. It is left alone deliberately, because it is a different question:
this change was about a page that lied to the user, and that file is a model
nobody uses. It belongs to the `design-decisions.md` 815 sweep of shell copies,
where the test is whether the behaviour exists rather than whether the screen
does. The answer there will be the same, for the same reason.
