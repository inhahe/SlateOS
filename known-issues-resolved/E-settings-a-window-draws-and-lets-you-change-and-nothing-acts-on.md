### [E] Settings a window draws and lets you change, and nothing acts on -- 2026-09-27

**Status:** `apps/` -- every row fixed or answered, the same day. `gui/` --
33 rows, lane C's to triage; listed by the scanner, not here.

**In short:** a settings row that draws its value, lets you change it, draws
the new value -- and nothing else in the program ever reads it. The user did
the only check they could, and it passed. `apps/videoplayer` had six of eight
rows like that (fixed the same day: four act now, and the ones that need a
decoder say "Not applied"). `scripts/find-drawn-only-settings.py` asks the
question for every program with a window; `find-echoed-settings.py` asks it
for command-line programs and could not see these -- the reads are not in a
`println!`, and it does not match structs called `...Preferences`.

| App | Setting | Verdict |
|---|---|---|
| `videoplayer` | six rows | **fixed** -- Auto-load Subtitles, Remember Volume and both languages act; Resume, Hardware Decode, On Finish and Deinterlace say "Not applied: nothing here decodes video" |
| `pomodoro` | Notification Sound | **fixed** -- the settings say nothing here plays sound |
| `diskimager` | `CreateOptions::format` | **fixed** -- the field is gone; the label reads what the copy writes |
| `remotedesktop` | Scaling, Color Depth, Refresh Rate | **fixed** -- a VNC session asks for the profile's bits a pixel and keeps its frame rate; the screen is shown at its scale (fitted, 50-200%, full size) with scrollbars to pan, and Z chooses it -- it had no control, every preset set Auto-fit |
| `torrent`, `fontmanager` | 9 | answered -- each panel says its settings are not applied |
| `netscan`, `diskimager` | a method, an output path | answered -- labels and records, not controls |
| `netmanager` | the VPN rows | answered -- the list is empty in the shipping program |
| `screenrecorder` | `auto_increment` | answered -- data (a file name), and no take is made |
| `settings` | `remote.rs`'s three | answered -- the page is reached from nowhere (C-Q17) |
| `mediaconvert` | 6 | set aside -- read only by `summary()`, which only tests call |

**How it decides.** A read is not acting when it turns the value into its own
text, computes the value's own next state, or writes it to a file; anything
else acts -- including a render function's `if prefs.show_grid`, which is what
a display setting correctly does. A field is reported only beside a sibling
the program does act on, the echo checker's rule for telling a dump from a
straggler.

**Where it is blind:** a read in another crate. `gui/appearance`'s wallpaper
settings are read by the desktop, from the file, so they report there as
"kept" and nothing else; the same will be true of most of `gui/`'s rows.
