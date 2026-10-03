## C-VPNMANAGER-IMPORT-EXPORT-HAVE-NO-FILE-PICKER (lane C, 2026-08-26) -- **fixed 2026-09-03**

**In short:** the VPN Manager's Import and Export buttons work, but they always
read and write **one fixed file** — `$HOME/.config/slateos/vpn/profiles.txt` —
rather than letting the user choose where. Export names that path in the status
bar afterwards, so nothing is hidden; but there is no way to export two
different sets of profiles to two different files, or to import one a colleague
sent you without first moving it to that exact path.

**Where it lives:** `apps/vpnmanager/src/main.rs` — `PROFILE_FILE`,
`profile_file()`, `VpnManager::export_to_file`, `VpnManager::import_from_file`.

**Why it is a fixed path rather than a chooser.** `guitk::dialog::FileDialog`
exists but cannot serve as one here, for three separate reasons:

| What a picker needs | What `FileDialog` does |
|---|---|
| respond to clicks | keyboard-only; it records no hit targets, so nothing in it can be clicked |
| know what is in the directory | does not read the filesystem — the caller must hand it a listing |
| be one widget | is a second full application's worth of state to drive from inside this one |

So wiring it in would not be a button; it would be a second program. A fixed,
*named* path is honest and usable today. The alternative that was rejected is
`apps/archivemanager`'s current precedent of writing "not yet implemented" into
the status bar, which is strictly worse: it gives the user nothing.

**The proper fix:** `guitk::dialog::FileDialog` needs (a) hit-target recording
via `guitk::frame`, and (b) a directory-listing source. That is toolkit work
that every app with an Open/Save button will need — the file manager, the text
editor, the image viewer and the archive manager all want the same widget — so
it should be done once in `gui/toolkit`, not once per app. Tracked separately
alongside
`TD-C-FILEDIALOG-PATHS-ARE-STRINGS-SO-A-NON-UTF-8-FILENAME-OPENS-THE-WRONG-FILE`,
which the same rework has to fix; this entry is the caller waiting on both.

**Until then:** Export says where it wrote, Import says how many profiles it
read and names the first failure if a block was malformed, and both refuse
clearly (`"Cannot export: $HOME is not set"`) rather than failing silently.

**Fixed 2026-09-03.** The blocker named in the table above —
`C-FILEDIALOG-IS-KEYBOARD-ONLY-SO-EVERY-PICKER-IN-THE-OS-IGNORES-CLICKS` — was
fixed the day before, and the third row of that table (`list_directory`) had
already been answered. Both buttons now open a real chooser: Import a
`FileDialog::open()` filtered to `*.txt`, Export a `FileDialog::save()`
pre-filled with the filename the fixed path used to end in.

Four things were worth deciding rather than merely typing:

- **The chooser opens where the old fixed path was**, and offers that path's
  own filename. `picker_start()` derives both from the same `profile_file()`
  the old code wrote to, rather than re-deriving `$HOME/.config/...`
  independently. Had it opened at `$HOME` or at `/`, this "upgrade" would have
  lost every existing user's exported file behind a chooser that no longer
  pointed at it. Pinned by
  `the_default_start_agrees_with_the_path_export_used_to_write`.
- **Export creates the directory it was given, on the way out, not on the way
  in.** `$HOME/.config/slateos/vpn/` usually does not exist on a fresh account,
  so `create_dir_all` runs in `export_to` just before the write. Doing it when
  the chooser *opens* would leave a directory behind for a user who then
  cancelled.
- **`NavigatedTo` is answered with a real listing every time.** The widget reads
  nothing itself, so an unanswered navigation shows the *previous* directory's
  files under the *new* directory's name — the same trap that made the widget's
  own Alt+Backspace bug invisible.
- **There are two public doors onto the pointer, and the second one is the
  interesting one.** `handle_event` is the normal door; `handle_click` is the
  other, and it is the one `Probe` uses. Both forward to the chooser.
  `the_chooser_keeps_clicks_and_keys_off_the_window_behind_it` sweeps a 12x8
  grid over the whole window through *both* and asserts nothing behind moved —
  but that half of the test turned out to pass with the forwarding deleted
  entirely, because `render_frame` calls `frame.discard_hits()` while the
  chooser is up, so the window's own targets are gone before any hit-test
  happens. Blocking is therefore free here; what is *not* free is that
  `handle_click` still **reaches** the chooser, since a press that finds
  nothing in an emptied frame is silently discarded. So the test also clicks
  Cancel through `handle_click` and asserts the chooser closes, which does fail
  when the forwarding is removed. Worth recording because the obvious
  assertion — "nothing behind the modal moved" — is exactly the one that
  cannot fail in a window that empties its own frame, and it would have been
  believed.

Seven tests cover it, including a real round trip: export to a scratch
directory through the chooser, wipe the profile list, import the file back
through the chooser, and compare. The scratch directory is a
`scratchdir::ScratchDir` (new dev-dependency) rather than a fixed temp path,
because a test that writes a real file and then fails would otherwise leave it
behind for the next run to read.

**Still open:**
`TD-C-FILEDIALOG-PATHS-ARE-STRINGS-SO-A-NON-UTF-8-FILENAME-OPENS-THE-WRONG-FILE`
now bites here too: the chooser hands back a `String`, so a profile file whose
name is not UTF-8 cannot be picked. That is the toolkit's bug, tracked there.
