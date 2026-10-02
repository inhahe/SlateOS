## TD-C-FOUR-DISPLAY-FEATURES-EXISTED-ONLY-IN-A-DEAD-SHELL-PANEL

**Date:** 2026-09-08. **Lane:** C.
**Where:** nowhere any more. They were in `gui/desktop/src/display_settings.rs`,
deleted under `design-decisions.md` §815 along with `appearance_settings.rs`
and `mouse_settings.rs` — 4 814 lines between them, none of it reachable.

**In short:** the shell had three settings panels nothing ever opened. §815
says screens you open move to the Settings app and the shell's copies go, so
they went. Almost everything they offered is in the Settings app already. Four
display features are not, and this is the record of them.

**The four.** None was working before — the panels were unreachable, so every
one of these was a control nothing read. Nothing regressed.

| feature | implemented anywhere? | note |
|---|---|---|
| **Gamma calibration** (`GammaSettings`, per-channel `GammaChannel`) | **No.** No `gamma_ramp`, `set_gamma` or `gamma_lut` anywhere in the tree; the `gamma` hits are sRGB colour maths and the DRM uapi's unused constants | Needs a real pipeline — a per-CRTC LUT pushed through KMS — before a slider means anything. |
| **Colour profiles** (`ColorProfile`: sRGB / AdobeRGB / Native) | **No.** Zero matches outside the deleted file | ICC handling is a colour-management subsystem, not a dropdown. |
| **Test patterns** (`TestPattern`: grayscale, colour bars, hue gradient, checkerboard) | **No.** Zero matches outside the deleted file | Cheap to rebuild when there is something to calibrate; pointless before that. |
| **Night-light *schedule*** (`NightLightSchedule`: Off / AlwaysOn / SunsetToSunrise) | Partly. The Settings app has `night_light_enabled` and `night_light_temperature`, but no schedule — its only two `schedule` matches are unrelated comments about saving | The smallest of the four and the only one whose feature half already exists. Sunset/sunrise also needs a location or a manual time pair, which nothing currently supplies. |

**Colour temperature is *not* on this list**, though the deleted panel had a
`ColorTemperature` type: the Settings app carries it as
`night_light_temperature`, which is the same control under the name a user
would recognise.

**Why they were not ported.** Adding four controls to a screen users can
actually reach, that read nothing and change nothing, is worse than deleting
four that nobody could reach. Three of them need a subsystem first.

**Method note, because it nearly bit.** The sweep that found these panels
counted `pub struct`, `pub enum` and `pub fn` and called a module unreachable
at zero external references. It did **not** count `pub const` — and
`appearance_settings.rs` re-exported a live `CONFIG_NAME` that the shell's
config watcher used. The build caught it, but a sweep of this kind should
enumerate *every* kind of public item, not the three that happened to come to
mind. The constant now comes from `appearance::CONFIG_NAME`, which owns it.
