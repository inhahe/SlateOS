## 852. A wallpaper setting carries the picture and nothing else

**Date:** 2026-09-14. **Lane:** C. **Decided by:** Claude (autonomous).

**In short:** the desktop can now show a picture you chose, and it can show it
in exactly one way -- scaled to cover the screen. The code underneath can also
letterbox it, stretch it, tile it, centre it, span it across monitors, rotate a
folder of them on a timer and tint them by time of day. None of those are
offered, and this records why that was a choice rather than an oversight.

**What was built.** `AppearanceSettings::wallpaper` is an `Option<String>`: a
path, or nothing. The shell's `sync_wallpaper` hands it to
`WallpaperManager::set_image` with `ImageFit::Fill`, and the Settings app's
Wallpaper page picks the file.

**The alternative, and it is a real one.** `ImageFit` has six variants and
`set_slideshow` takes a directory, an interval and a shuffle flag. The setting
could carry all of it, and the page could offer all of it, for about the same
work.

**Against carrying more:**

* `ImageFit` lives in `gui/desktop`, which depends on `appearance` and not the
  other way round. Putting the fit in the setting means either moving the enum
  down into `appearance` or **declaring a second one beside it** -- and a second
  model of one idea is the defect this lane spent 2026-09-14 removing from five
  other places (fifteen private clipboards, four lists of installed programs,
  three startup managers, two ICMP codecs, two copies of "is this extension
  slow"). Writing a sixth on purpose, the same day, would be hard to defend.
* The Mouse settings page already states the rule this follows. It offers one
  slider out of fifteen possible, because the other fourteen have no consumer,
  and *"a slider drawn for any of those would save a value to a file, look as
  though it had worked, and change nothing"*. Here the fit mode *has* a
  consumer and the **setting** for it does not exist, which is the same failure
  one layer along.

**For carrying more:** a wallpaper that can only be cropped is a worse
wallpaper, and the machinery is right there. Someone with a portrait photograph
on a landscape screen gets it cropped and has no way to say otherwise. That is
a real cost and it is paid today.

**The decision:** ship the picture, and add the fit mode when it can be added
as *one* model rather than two -- most cleanly by moving `ImageFit` into
`appearance` beside the other presentation enums (`ThemeMode`, `SurfaceStyle`,
`StripStyle`, `ColorFilter`), which is where it would have belonged had the
setting existed first.

**Why this is a Claude decision rather than an operator one.** It chooses
*when* to offer a control, not what the system does: the user-visible outcome
of every option here is a wallpaper that works. The operator's answer to C-Q6
settled the shape of settings pages, and the Mouse page set the precedent this
follows. If the trade were "crop everything for ever" it would be theirs; it is
"crop for now".

**What would reverse it:** a second consumer for the fit mode, or a user who
wants a tiled pattern. Both point the same way, and this entry says what the
clean version looks like.

### Followed through the same day, 2026-09-14

The deferral lasted about two hours, which is worth recording rather than
quietly editing away: the entry above was written to explain shipping a gap,
and the gap is closed.

Done exactly as this entry said it should be, and in that order:

1. `ImageFit` moved into `appearance` beside `ThemeMode`, `SurfaceStyle`,
   `StripStyle` and `ColorFilter`. `gui/desktop` re-exports it, so all 55
   references compiled unchanged; `as_str`/`from_str_config` became
   `yaml_name`/`from_yaml_name` on the same six strings, so a wallpaper config
   written by the older code still reads.
2. `AppearanceSettings::wallpaper_fit`, defaulting to `Fill` -- what the shell
   did unconditionally before, so no existing desktop changes shape.
3. The Wallpaper page offers it, and only when there is a picture to place.

**One thing the plan did not anticipate, and it is the useful part.**
`WallpaperManager::set_fit` had to be added, narrower than `set_image`. The fit
is applied when the wallpaper is *drawn* -- `get_render_commands` hands it to
`compute_image_rect` -- so changing it needs no new pixels, while `set_image`
issues a fresh image id and the background surface re-inflates any id it has
not seen. Routing a fit change through `set_image` would have decoded a
full-screen photograph again to learn nothing new about it, six times over for
a user trying each option. A test fails if it is swapped back.

**Was the deferral worth having made?** Yes, and not because the gap was
valuable. Writing down *why* the fit was missing turned a vague "the wallpaper
should support more" into a named blocker -- one model, not two -- which was
then a thing that could be removed in one move. The entry cost ten minutes and
the follow-through was shorter than the discussion would have been.
