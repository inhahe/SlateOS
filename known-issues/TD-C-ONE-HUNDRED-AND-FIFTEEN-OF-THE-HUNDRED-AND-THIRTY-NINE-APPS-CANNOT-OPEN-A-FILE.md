## TD-C-ONE-HUNDRED-AND-FIFTEEN-OF-THE-HUNDRED-AND-THIRTY-NINE-APPS-CANNOT-OPEN-A-FILE

**In short:** the invented data in these programs is not the disease. It is the
symptom. **115 of the 139 applications with a `main.rs` have no filesystem
access of any kind** — no file picker, no `std::fs`, no `safeio`. They cannot
open a document, save one, or read anything the user has. The seeded libraries
and sample records exist because there is no other way for a window to have
anything in it, and one of them says so in its own comment: *"so the first
window is not an empty grid"*.

**Date:** 2026-09-15. **Lane:** C. Found by asking, after fixing the photo
manager, how many other applications were in the state it had been in.

**The measurement.** For each `apps/*/src/`, count references to `FileDialog`,
`std::fs` and `safeio::`. 115 of 139 score zero. Games are a legitimate part of
that number — `chess` needs no files — so the count alone overstates it, and
the named cases below are the argument rather than the total.

**Three that are worth reading twice:**

* **`apps/filesearch` cannot search files.** Its dependencies are `globmatch`,
  `guitk`, `oswindow` and `appearance` — nothing that reads a directory. The
  search machinery is *complete*: search by name, by glob, by regular
  expression, by category, with sorting. It runs against `Index::entries`, and
  `index.add(...)` is called from **tests only**. So a finished search engine
  runs against an index production never fills.
* **`apps/filediff` cannot read files.** It depends on `diffcore`, which is a
  real diff implementation, and on nothing that opens a file.
* **`apps/email` has neither network nor storage.** It depends on `guitk`,
  `oswindow` and `appearance`, and seeds itself with `seed_sample_mail`.

`apps/photomanager` was in exactly this class until 2026-09-15: a real EXIF
parser, a real album model, an invented library, and no picker. It took one
`FileDialog` and `guitk::dialog::list_directory` to fix, both of which already
existed.

**Why this changes the order of the remaining work.** The 63 fixture functions
and the 32 self-declared stubs are two views of one cause. Fixing them
app-by-app — replacing invented records with an honest "nothing here" — makes
each program truthful and leaves it useless, which is the right trade when
nothing better is available and a poor one when something is. For this class
something is: the toolkit has had a working file picker all along.

**So the cheap fix is a shared one.** Every app in this class needs the same
three things the photo manager needed: a control that opens
`guitk::dialog::FileDialog`, a handler that reads the chosen path, and an
empty-state that says what to do. That is a per-app change, but it is the same
change, and it converts "honest and empty" into "works".

**What this does not cover.** The other class — `sysinfo`, `devicemanager`,
`partmanager`, `netmanager`, `netscan`, `sysmonitor`, `undelete`, `speedtest` —
needs data the *system* must produce, not data the user can hand over, and
those stay blocked on kernel work whatever this does. `sysinfo` is the worked
example: its client is finished and waiting on `/sys/devices` producers.

**The honest caveat on the number.** 115 counts every `main.rs` app including
games and toys. I have not classified all 139, and the per-app judgement of
"should this open files" is exactly the kind of thing that should be made when
someone picks the app up rather than pre-decided in a list here.

**A SHARPER NUMBER, AND THE SAME SENTENCE THREE TIMES.**

115 of 139 counts games, which need no files. A better discriminator is an app
that **has no filesystem access and whose own code talks about files anyway** —
ten or more mentions of save, load, export, import, document, filename or
file_path. That is **28 applications**, and the list reads like a list of
document editors: `hexeditor`, `jsonviewer`, `pdfviewer`, `diagram`, `slides`,
`notes`, `kanban`, `renamer`, `contacts`, `dbviewer`, `email`, `clipmanager`,
`credmanager`, `flashcards`, `reminders`, `rssreader`, `screenrecorder`,
`soundrecorder`, `startupmanager`, `systemrestore`, `undelete`, `netscan`,
`speedtest`, `remotedesktop`, `defrag`, `camera`, `alarmclock`, `terminal`.

(`terminal` is probably a false positive — it names paths without needing to
open them. The rest are not.)

**The evidence that this is one cause rather than 28 coincidences is that three
authors wrote the same sentence.**

* `apps/photomanager`: *"A library with something in it, so the first window is
  not an empty grid."*
* `apps/filesearch`: *"Until a real index exists this is what there is to
  search. It is one call so that the moment `indexer` can be asked, this is the
  line that changes."*
* `apps/hexeditor`: *"Until a file can be opened this is what there is to edit:
  every byte value once, which is also the most useful thing to look at while
  the rendering is being worked on."*

Three programs, three authors, one structure: *this is placeholder, the real
thing is blocked, here is why the placeholder is reasonable.* None of them was
wrong about the reasoning. All three were wrong about the blocker — the file
picker and `std::fs` were there the whole time. `filesearch`'s comment even
names the wrong dependency: it waits for the `indexer` service, and the
filesystem was nearer.

**`apps/hexeditor` is the sharpest of the three** and should probably be next.
It opens on `(0..=255).collect()` — every byte value once — with
`file_path = Some("/demo/sample.bin")`, a path that does not exist, so the
window names a file it is not showing you. A hex editor is for looking at a
specific file's actual bytes; there is no version of that which a synthetic
buffer satisfies.

**Two are done.** `apps/photomanager` and `apps/filesearch` both took the same
three pieces — a control that opens `guitk::dialog::FileDialog`, a handler for
the chosen path, and an empty state that says what to do — and all three
already existed in the toolkit.

**THE RECIPE, AFTER DOING IT THREE TIMES.**

`photomanager`, `filesearch` and `hexeditor` were the same change. Written out
so the remaining twenty-five are cheaper, and because the parts that took the
longest were not the obvious ones.

1. **A field** — `file_dialog: Option<FileDialog>` on the app state.
2. **A way in** — `Ctrl+O`, or a toolbar control. Check which chords the app
   already uses; `filesearch` had six taken.
3. **`FileDialog::open()`** for a file, **`select_folder()`** for a directory,
   filled by `guitk::dialog::list_directory(dialog.current_path())`. The widget
   does no I/O by design: the host reads the listing and hands it over.
4. **Intercept events while it is up** — `Event::Key(..) if self.file_dialog
   .is_some()` ahead of the app's own handlers, or a click meant for a filename
   lands on whatever is drawn beneath.
5. **`DialogAction`** has four arms and all four matter: `NavigatedTo` must
   re-list, or the dialog shows the old directory under the new name.
6. **Render it last**, so it is above everything — the same order in which the
   events reach it.
7. **Delete the seeded data** and make `main` start empty with a line saying
   how to begin.
8. **Move the seeder into `#[cfg(test)]`.** All three had tests resting on it.
   A fixture production can reach is a fixture that ships.

**The three things that cost the most time, none of which are in that list:**

*The picker must actually be drawn.* In `hexeditor` I wrote a comment saying it
was, above a `render` that was not. It compiled, 189 tests passed, and the
result would have been a dialog swallowing every keystroke while invisible. Now
pinned by `the_picker_is_drawn_when_it_is_open`, which counts render commands
before and after opening — a test that is hard to write vacuously.

*Bounds must announce themselves.* `filesearch` caps the walk at 20,000 entries
and `hexeditor` caps a read at 16 MiB. Both say so when they bite. A silent cap
turns a partial answer into a confident wrong one: "no results" reads as "no
such file", and a truncated hex view lies about a specific address.

*The app must agree that something changed.* `apps/jsonviewer` decides whether
to redraw by comparing a `state_fingerprint()` before and after the key
handler, because `handle_key` reports nothing. Opening the picker changes no
*other* watched state, so until `file_dialog.is_some()` joined that tuple the
dialog would have been invisible until something else moved — the same
invisible-picker bug as `hexeditor`'s, reached by a completely different
mechanism. **Step 6 of the list above is not sufficient on its own.** Before
writing the picker, find out how the app decides to draw a frame: an
`EventResult`, a dirty flag, a fingerprint, or nothing at all. Two of the four
apps done so far needed something beyond "render it last", and they needed
different things.

*Names are bytes.* `guitk`'s `DirEntry` is deliberately `OsString`, and its own
doc explains why — decoding lossily "could make it match one it should not". In
`filesearch` that is the whole game, so non-UTF-8 names are **skipped and
counted** rather than decoded, and the count is shown. `IndexEntry` holding
`String` and `globmatch::glob_match` taking `&str` is the real limit; fixing it
properly means byte-capable matching, which is its own task.

**FOUR FOR FOUR ON THE FIXTURE POINT.**

Every one of `photomanager`, `sysinfo`, `filesearch` and `jsonviewer` had tests
resting on the invented production data, and in every case they went red
together the moment it was removed — six, sixteen, sixteen and six of them.
None of those tests said they depended on it; they read as self-contained and
were not.

The consistency is the finding. **Production fixture data is always
load-bearing for tests that nobody recorded as depending on it**, because a
test needs *something* to act on and the seeded data is there. So the red is
not a complication of this work, it is the reliable second half of it, and
budgeting for it is the difference between "delete the fixture" being a
ten-minute job and a surprise.

Worth adding: the failures are loud only when the data becomes **absent**
rather than merely **different**. `apps/settings` surfaced because an account
list became empty; had I replaced three invented accounts with three real ones,
all six tests would have kept passing against whatever the machine happened to
have, and I would have called it a clean migration. `apps/sysinfo` was the
opposite and the better case: its fixture asserts its own precondition, so the
sixteen failures named the problem — *"fixture's property table fits on screen:
4 rows in 144 px"* — instead of passing vacuously.
