### TD-C-FILEDIALOG-PATHS-ARE-STRINGS-SO-A-NON-UTF-8-FILENAME-OPENS-THE-WRONG-FILE — 2026-08-25 — **FIXED 2026-09-03**

**Where:** `gui/toolkit/src/dialog.rs` — `DirEntry::name`, `DialogAction::Selected(String)`,
`DialogAction::NavigatedTo(String)`, `FileDialog::with_initial_path(&str)`. First
embedder: `apps/diskimager/src/main.rs` `read_directory` / `dispatch_to_open_dialog`.

**What it is.** The dialog's whole path surface is `String`. An embedder listing a
real directory therefore has to convert `OsString` to `String`, and the only
total conversion is `to_string_lossy`, which replaces every byte it cannot decode
with U+FFFD. `read_directory` does exactly that. A file whose name is not valid
UTF-8 is listed under a name that is *not the name on disk*, and the string the
dialog hands back on `Selected` no longer identifies it: `fs::read` on it fails
with "not found" if nothing else matches, or — worse — succeeds against a
different file that happens to contain a literal U+FFFD in the same position.

This directly violates `CLAUDE.md` self-review item 7: *"Never force UTF-8 on
filesystem paths... No `from_utf8_lossy` — that's silent data corruption. Our
paths allow all bytes except `/` and `\0`."* SlateOS deliberately permits every
byte but those two in a filename, so this is not a corner case imported from
another OS's rules; it is a filename our own filesystem is specified to accept.

**Why it is debt and not a fix in this commit.** The defect is in the toolkit's
API, not in the embedder. `apps/diskimager` was the first program ever to use
`FileDialog` (the widget had zero callers tree-wide until 2026-08-25 — see
lesson 46), so it is also the first to hit this. Changing `DirEntry::name` and
the `DialogAction` payloads from `String` to `PathBuf`/`OsString` touches the
dialog's rendering (which measures and elides the name), its filter matching,
its sort comparators and its 40-odd tests. That is a lane-C change and squarely
ours to make — it was separated from the wiring commit so that "the parser now
has a caller" and "the toolkit's path type is wrong" are two reviewable changes,
not one.

**The proper fix.** `DirEntry::name: OsString`; `DialogAction::Selected(PathBuf)`
and `NavigatedTo(PathBuf)`; `with_initial_path(impl AsRef<Path>)`. Rendering
converts to a display string *at the draw call and nowhere else*, which is the
one place lossy conversion is correct because it is producing glyphs, not a
lookup key. Filter matching moves to `Path::extension` on the `OsStr`. Then
`read_directory` in diskimager drops its `to_string_lossy` entirely and
`load_image_from_path` takes a `&Path`.

**Test that should exist and cannot yet.** A file created with an invalid-UTF-8
name, listed, selected, and read back byte-identically. Note this needs a host
filesystem that permits such a name — on Windows it does not, so the test
belongs with the SlateOS-target suite, not the host one. Until then a unit test
on the type signatures is the most that can be pinned.

**Severity while open:** low frequency, silent, and wrong-file rather than
crash. Nothing in lane C currently *creates* non-UTF-8 filenames, so the way to
meet it today is an image copied from another system.

### Fixed 2026-09-03

The type surface is now what the "proper fix" above prescribed:

| item | was | is |
|---|---|---|
| `DirEntry::name`, `DirEntry::extension` | `String` | `OsString` |
| `DialogAction::Selected`, `::NavigatedTo` | `String` | `PathBuf` |
| `FileDialog::current_path`, `history_back/forward` | `String` | `PathBuf` |
| `current_path()`, `confirm()` | `&str` / `Option<String>` | `&Path` / `Option<PathBuf>` |
| `with_initial_path`, `navigate_to`, `list_directory` | `&str` | `impl AsRef<Path>` |
| `with_filename`, `set_filename` | `&str` | `impl AsRef<OsStr>` |

**Three lossy conversions survive and no more, each carrying a comment saying
why** — two are draw calls (the path bar and the entry-name row) and the third
fills the Save box's text field, which has to be a `String` because the user
edits it a character at a time. That third one is the reason `filename_exact`
exists (case 2 below): the decoded text is what is *shown*, never what is
opened. Nothing used as a lookup key is ever decoded. `apps/archivemanager`
dropped its
`to_string_lossy` entirely; `apps/diskimager` carries `ImageInfo::path`,
`RecentImage::path`, `WriteOptions::image_path` and `CreateOptions::output_path`
as `PathBuf` all the way down to `File::open`, and its three remaining lossy
calls are likewise all text for the screen: a `RenderCommand::Text`, the
elision inside `truncate_path`, and a sentence in a confirmation dialog. (Its
status-line messages use `Path::display`, which is the same bargain stated in
the type system.)

**Three things the original entry did not anticipate.**

1. **`std::path` is the wrong tool for splitting a SlateOS path on a Windows
   host.** Our paths are `/`-separated and admit every byte but `/` and NUL —
   including `\`. `Path::parent` / `Path::join` on a Windows host *also* split
   on `\`, so a filename legally containing one would be cut differently by the
   host test suite than by the target. The dialog therefore has its own
   byte-level `parent_path` / `join_path` / `extension_of` built on
   `OsStr::as_encoded_bytes`, cutting only at ASCII `/` — which is a character
   boundary in every encoding `OsStr` uses, so `from_encoded_bytes_unchecked`
   over the halves is sound. `PathBuf` remains the public type; it is only the
   *splitting* that is hand-written. (See `design-decisions.md` §808.)

2. **Save mode needed a second field.** `filename_input` has to stay a `String`
   because the user types into it a character at a time. But clicking an
   existing file to overwrite it *fills* that field from the listing — and a
   name with no UTF-8 spelling would round-trip through U+FFFD and then create
   a **different file beside the one the user pointed at**. The fix is
   `filename_exact: Option<OsString>`, holding the exact bytes for as long as
   the text is still the listing's, and dropped by `edited_filename()` the
   moment a keystroke changes the field. Dropping it on edit is the important
   half: kept past an edit it would overwrite the clicked file no matter what
   the user then typed, which is the worse of the two failures.

3. **The test the entry said "cannot yet exist" can exist, just not the way it
   was imagined.** Creating an invalid-UTF-8 *file* does need a target
   filesystem. But the defect is in the widget, and the widget is handed its
   listing — so feeding it a `DirEntry` whose name has no UTF-8 spelling and
   asserting the exact bytes come back out pins the whole of it on the host.
   `unmappable_name()` builds one through the platform's own safe API (a lone
   high surrogate via `OsStringExt::from_wide` on Windows, a `0xFF` byte via
   `from_vec` elsewhere) rather than asserting bytes into an `OsStr`. Four
   tests: `opening_a_name_that_is_not_utf8_returns_the_exact_bytes`,
   `activating_…` (the double-click arm is separate code),
   `overwriting_a_name_that_is_not_utf8_targets_the_file_that_was_clicked`
   (case 2 above), and `typing_in_the_name_field_drops_the_remembered_bytes`.
   The last of those drives real `Backspace` and character key events through
   `handle_event` rather than calling `set_filename`, because `set_filename`
   *fills* the remembered bytes and so cannot tell whether they are ever
   invalidated — written the other way round, the test passes with the
   invalidation deleted.

**Three more instances of the same defect found in callers on the way**, all
pre-existing and all fixed here:

- `apps/archivemanager` `default_directory()` read `$HOME` with `env::var`,
  which returns `Err` for a non-UTF-8 value — so a user whose home directory is
  not UTF-8 silently got `/`. Now `var_os`.
- `apps/archivemanager` updated `last_directory` at three sites inside
  `if let Some(dir) = ….to_str() { … }`. When the chosen path was not UTF-8 the
  update was **skipped entirely** and the next dialog reopened at the previous
  directory, with nothing said. Now unconditional.
- `apps/vpnmanager` `picker_start()` returned `(String, String)` built with
  `to_string_lossy`, so on a non-UTF-8 `$HOME` the Export box opened with a
  name full of U+FFFD and would have written a *second* file beside the user's
  existing profiles rather than over it — precisely the failure this entry
  describes, reached without the chooser being involved at all. Now
  `(PathBuf, OsString)`. Its fallback name is also now derived from
  `PROFILE_FILE`'s last component instead of being spelled out again, since
  `PROFILE_FILE` is a relative *path* (`.config/slateos/vpn/profiles.txt`) and
  the two spellings could drift.

`apps/diskcleanup` and `apps/undelete` needed no change. The
two other entries that name this one as a blocker —
`TD-C-THE-RUN-BOX-BROWSE-BUTTON-HAS-NOWHERE-TO-GO` and the vpnmanager
import/export entry — are unblocked by it.
