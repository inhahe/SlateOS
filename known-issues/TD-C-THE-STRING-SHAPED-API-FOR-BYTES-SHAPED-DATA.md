## TD-C-THE-STRING-SHAPED-API-FOR-BYTES-SHAPED-DATA -- METHOD 2026-09-16

**In short:** five times in one day, in five unrelated programs, a path or an
environment variable was read through the part of the standard library that
only handles text. Each time the code compiled, passed its tests and looked
ordinary. The results ranged from a completion that offers a folder and then
cannot open it, to a recycle bin that empties itself on restart.

**Date:** 2026-09-16. **Lane:** C.

**The five:**

| where | the call | what it did |
|---|---|---|
| `apps/backup` `cmd_schedule` | `dest.to_string_lossy()` | stored a schedule naming a directory the user never gave |
| `apps/fileassoc` / `gui/associations` | group name as text | (fixed as part of the shared-crate work) |
| `apps/explorer` `completions_for` | `file_name().to_string_lossy()` | offered a completion for a folder that does not exist under that name |
| `apps/explorer` `DiskCache::default_location` | `env::var("HOME")` | disabled the thumbnail cache entirely, silently, for the life of the install |
| `apps/explorer` `RecycleBin::default_location` | `env::var("HOME")` | **put the recycle bin in `/tmp`, so deleting a file lost it at the next restart** |

**Why it keeps happening, which is the point of writing it down.** Nobody was
careless. `std` offers a `String`-shaped API for a bytes-shaped thing at every
one of these boundaries, and the `String` one is shorter, needs no `?`, and
reads better at the call site. `to_string_lossy` over `to_str`, `var` over
`var_os` -- the wrong choice is the convenient one every time, which is why it
recurs in crates written months apart by whoever was there.

**The sharpest example of how local the knowledge is.** `RecycleBin` is
*meticulous* about this exact hazard: `send_to_bin` encodes the original path
losslessly, with a comment explaining that `Display` would write U+FFFD and
restore would then recreate the wrong name. That care was undone one function
earlier, by the location of the bin itself. **Understanding a trap does not
generalise across a function boundary unless somebody goes looking.**

**Two shapes, and they fail differently.** The lossy conversions produce a
*wrong value* that looks right -- a name, a path, a key. The UTF-8-only reads
produce *absence*: a feature that silently does not exist, with no bad data to
notice. The second is harder to find, because there is nothing to see.

**Triaged 2026-09-16: `apps/explorer/src/fileops.rs`, all eight sites.** One
was a defect and is fixed (the copy's scratch name, which collided for two
names differing only in undecodable bytes). The other seven are legitimate and
should be left alone:

* `progress.current_file` and the two extension/name formatters feed status
  text. Display, not identity.
* `make_id` builds a recycle entry's directory name from a lossy filename plus
  a hash — and the name is *opaque*. The authoritative original path is stored
  losslessly in `meta.txt` beside it (see `send_to_bin`), and collisions are
  settled by the filesystem in `create_entry_dir`'s retry loop rather than
  trusted from the string. A lossy component is safe precisely because nothing
  reads it back as a path.
* The id read back in `list` matches that: our own ids are always UTF-8 because
  we generate them, and a *foreign* directory in the bin is not ours to act on.

So the rate in this file was one in eight, and the seven were not near misses —
each had a reason on the spot. Recorded so the next reader spends the minute on
the other thirty-seven sites in `apps/explorer` and `gui/desktop`, not these.

**Triaged 2026-09-16: `apps/explorer/src/main.rs`, the eight sites there.**
Four are a real but bounded defect and four are fine.

The defect: `PathBar::new` and `set_path` take a `&str`, so the address bar is
handed `current_path.to_string_lossy()`. For a directory whose path is not
UTF-8 the bar therefore *shows* a path that does not exist, and pressing Enter
on what it shows navigates nowhere — the same shape as the completion defect
fixed the same day, one layer up. Not fixed here because the honest repair is
for the widget to hold bytes, which is a change to `guitk::pathbar`'s API and
to every caller, not a call-site swap. The user can still reach such a folder
by clicking; only the typed route is broken.

The four that are fine: a listing entry's `name` is display text while
`entry.path` carries identity — the same split that makes `make_id` safe — and
the extension sites feed sorting and type lookup, where a name with no text
form sorts oddly and classifies as unknown, which is what it is.

**Triaged 2026-09-16: `gui/desktop`, the ten sites outside tests.** Three are
identifier-shaped and are recorded here rather than fixed, because each needs a
type to change rather than a call:

* `icons.rs` builds `IconAction::OpenPath(String)` from the Documents and home
  paths. The variant holds a `String`, and `storage_key` writes it into the
  saved icon layout as `path:{path}` — so for a home directory that is not
  UTF-8 the desktop icon both opens the wrong place and is *saved* under a
  mangled key. The fix is `OpenPath(PathBuf)` and a byte-safe storage key, not
  a call-site change.
* `run_dialog.rs` compares a candidate path to typed text through
  `to_string_lossy().trim()` in two places, so a command whose path is not
  UTF-8 can fail to match itself, or match the wrong entry.

The rest are display: a program's file name for a label, and test helpers.

**A note on `icons.rs`' own reasoning, which is right and worth borrowing.**
`storage_key`'s doc explains that an icon stores the *path* and looks its label
up when drawn, "for the same reason the taskbar's pinned apps already follow --
storing the label too would be a second copy of it, stale the first time the
thing is renamed". That is the correct instinct about identity; the flaw is
only that the identity is held as text that cannot represent every path.

**The fix for the widget cases already exists in the same toolkit.**
`gui/toolkit/src/dialog.rs` keeps *both*: `fill_filename` sets
`filename_input` (the lossy string, for drawing) and `filename_exact`
(the `OsString`, for the operation), with a doc comment saying why the display
copy cannot be the one that acts. Every widget case recorded above -- the
address bar's `set_path`, `IconAction::OpenPath`, the run dialog's comparisons
-- is the same problem with the same answer: **draw the flattened text, act on
the kept bytes.** None of them needs a new idea, only the pattern from the file
dialog applied a second time.

That settles the scope for two of the three, and **not** for the pathbar --
corrected here after checking rather than asserting twice in a row:

* `IconAction::OpenPath` -- **FIXED 2026-09-16**, and the measurement is what made it safe to do: the variant holds a `PathBuf`, `storage_key` answers `Option<String>` and skips a path with no text form, and an existing layout is untouched because a representable path yields the key it always did. Was a real defect, measured first: Half the
  byte-safety is already there -- `icons.rs` reads `HOME` with `var_os` and
  builds a `PathBuf` -- and the `String`-typed variant throws the bytes away,
  after which activation does `PathBuf::from(path)` on the flattened text. So a
  home directory that is not UTF-8 gives a desktop icon that launches a path
  which does not exist.

  The fix is `OpenPath(PathBuf)` **plus a decision**, because `storage_key`
  writes `path:{…}` into the saved icon layout and that document's keys are
  text. A path with no text form cannot be a key there at all. The precedent is
  already in this tree: `columnprefs::set_for_folder` answers `false` for such a
  path and lets the caller say so, rather than inventing a key that would save
  the position against a different folder. Applying that here means an icon on
  an unrepresentable path keeps working and simply does not remember where it
  was put.
* **The run dialog is not a defect at all** -- corrected after reading around
  the line rather than at it. `RunDialog` already holds `command_exact:
  Option<PathBuf>`, which is the kept-bytes half of the pattern. The lossy
  comparison is not an attempt at identity; it asks *whether the user left the
  displayed text alone*, and only then does it act on the exact path. Its own
  comment says so: "exact glyphs on screen gets the file those glyphs came
  from, which is the only file they could have meant." Changing it to compare
  `OsStr` to `OsStr` would have broken a correct design -- the comparison must
  be against what is *shown*, because that is what the user either edited or
  did not.
* **`guitk::pathbar` is more than a field.** It holds `path: String` and
  `edit_text: String` and touches them in sixty-four places; breadcrumb mode
  *splits the path into clickable segments*, and edit mode is a text field the
  user types into. Carrying exact bytes means the segment split and the
  `Navigate(String)` event change with it. That is a contained refactor of one
  widget and its one consumer, not a redesign and not a one-liner.

  **Blast radius confirmed 2026-09-16:** `apps/explorer` is the *only* crate
  that constructs a `PathBar` or matches `PathBarEvent`. `apps/editor` and
  `apps/fileassoc` mention the word in prose and nothing else. So the change
  touches exactly two files, which is the fact that decides whether this is
  an evening's work or a week's.

  **Worked design, 2026-09-16 — with one part corrected before any code was
  written, and the correction is the important half.**

  My first plan was "`Path::components()` does the breadcrumb split, so no byte
  surgery is needed". **That is wrong for this widget.** It splits on `/`
  because SlateOS paths use forward slashes, and `Path::components()` is
  *host-dependent*: on the Windows machine the tests run on it also treats `\`
  as a separator and `C:\` as a prefix. Converting to it would make the widget
  behave differently on the host than on the target, and the tests only see the
  host — a green suite proving nothing about the system this ships on.

  **FIXED 2026-09-16.** And the "decision" I thought this needed turned out not
  to be one. `design.txt` already fixes the separator, so the widget is
  POSIX-shaped by specification rather than by preference; and
  `dialog::parent_path` had *already* written the host-versus-target argument
  down, months of reading ago, for exactly the same reason. I rediscovered a
  conclusion the codebase had reached without me. Checking for the precedent
  first would have been cheaper than deriving it twice.

  Two things came out of doing it that the design had not predicted:

  * **The obvious byte implementation is unsound in a way the ASCII argument
    hides.** Splitting `as_encoded_bytes()` at `/` is sanctioned — `/` is
    ASCII, the encoding is self-synchronising, so a cut can never land inside a
    character. But *joining* the pieces back into a `Vec<u8>` is not covered:
    on Windows a trailing unpaired high surrogate meeting a leading low
    surrogate has to be recomposed into a single four-byte sequence, and a raw
    concatenation would silently produce ill-formed WTF-8. So the rule is
    asymmetric, and only half of it is the half everyone quotes: **narrowing to
    a sub-slice is safe; splicing two runs together is not.** All joining here
    goes through `OsString::push`, the safe API that knows about recomposition.
  * **The crate already had this `unsafe`, once.** `dialog.rs` carried a
    private `os_str_from_bytes` with the same contract. Adding a second copy
    would have made `gui/toolkit` a crate where the same proof is stated twice
    and can be weakened in one place only — the shape of
    `TD-C-FOUR-PLACES-DECIDE-WHAT-KIND-OF-FILE-SOMETHING-IS`. Both now use
    `gui/toolkit/src/osbytes.rs`, which holds the single obligation and exposes
    a *safe* `split_on_slash` above it. `pathbar.rs` ends up with no `unsafe`
    at all, which is the better outcome: the count of unsafe blocks in the
    crate went **down** while the number of byte-correct call sites went up.

  * `path: PathBuf` instead of `String`; `segments` keeps display strings for
    drawing, and a breadcrumb click joins components up to the clicked one.
  * `Navigate(String)` becomes `Navigate(PathBuf)`. `apps/explorer` is the only
    matcher.
  * `edit_text` **stays a `String`** — it is a text field with a caret, and the
    cursor handling there is BiDi-aware byte offsets that should not be
    disturbed. Typing produces text, and text is what `PathBuf::from` takes.
  * The exactness is kept the way `RunDialog` already keeps it: on entering
    edit mode, store `edit_exact: Option<PathBuf>` beside the lossy
    `edit_text`; on confirm, if `edit_text` still equals the lossy rendering,
    navigate to `edit_exact`, otherwise to `PathBuf::from(edit_text)`. That is
    `command_exact`'s logic, which is already proven in this tree and was
    nearly "fixed" out of it this evening by someone who did not read around
    the line.

  What this buys: a directory whose name is not UTF-8 displays and navigates
  correctly unless the user edits the text, which is the same guarantee the
  file dialog and the run dialog give.

Saying "one extra field" for all three was the same error as the entries it was
correcting: a scope stated without being measured. The difference matters
because it decides whether someone starts.



**The real extent, counted 2026-09-16 and larger than this entry first said.**
The "45 sites" figure above covered `apps/explorer` and `gui/desktop` only.
Across lane C it is **131 sites in 42 files**. Twenty-two are triaged: two
defects fixed, one measured and recorded (`IconAction::OpenPath`), one
withdrawn after reading the design around it (the run dialog, which already
keeps exact bytes), and the rest cleared with reasons.

So this is not a sweep to finish in a sitting, and it should not be attempted
as one. The triaged files show why: the rate of genuine defects is roughly one
in ten, the other nine each have a reason on the spot, and **one of the
twenty-two was code I nearly broke by applying the pattern without reading
around it.** A batch pass over 131 sites by someone confident in the pattern is
the most likely way this tree acquires a real bug from this entry.

**The dangerous category is exhausted, searched 2026-09-16.** Taking that
order and running it across all of `apps/` and `gui/`: no production site
outside the two already handled turns flattened text into a **filename or a map
key**. The searches were for a lossy value reaching `join`, `insert`, a map
lookup, a `format!` that builds a name, or a `File::create`/`fs::write`. What
came back was test fixtures, the two extension formatters already cleared, and
`apps/indexer`, which compares an extension against `config.exclude_extensions`
— a list the user writes as *text*, so a textual comparison is the only kind
available and flattening is right there for the same reason it is right in the
run dialog.

So the remaining ~109 sites are display and text comparison, and the two that
mattered are fixed. That is the useful shape of this entry now: **not 131 things
to do, but a rule to apply when writing new code**, plus three recorded cases
(`IconAction::OpenPath`, `guitk::pathbar`, and the icon layout's key format)
where the type is wrong and the fix is scoped.

The order worth taking, cheapest signal first: anything whose flattened text is
used as a **map key or a filename** (that is where both real defects were),
then anything compared for equality, then everything else, which is almost
always display.

**Do not sweep this blindly.** Two `env::var` calls in this lane are correct
and must stay: `gui/compositor`'s `SLATE_DRM_CARD` parses to a `u32`, so a
non-UTF-8 value is invalid input and is already refused with a message, and
`gui/font`'s is a Windows-only test reading `WINDIR`. **The discriminator is
not the API, it is whether the value is a path or a number.** A sweep that
replaced every `var` with `var_os` would be churn in both places and would
teach the next reader that the rule is mechanical.
