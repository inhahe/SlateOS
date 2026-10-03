## TD-C-A-FIELD-ONLY-EVER-INITIALISED-IS-INVISIBLE-TO-EVERY-CHECK-WE-HAVE -- METHOD 2026-09-17

**In short:** a struct field that is set once when the struct is built and
never read again is dead state, and it usually means a half-built feature
that a reader will believe in. Neither of the two things that should catch it
does: our gate needs an *assignment* to notice a field, and the compiler's
own check is silenced by `pub`. A crude scan of `gui/` and `apps/` finds 347
candidates.

**Date:** 2026-09-17. **Lane:** C.

**How it was found.** `apps/explorer` carried
`pub tree_expanded: Vec<PathBuf>`, doc-commented "Tree sidebar expanded
paths" and initialised to `vec!["/"]`. It occurred exactly twice in the
crate: that line and the initialiser. The module's own feature list opened
with "Directory tree sidebar"; the sidebar is five fixed quick-access rows.
Removed, along with the claim.

**Why nothing caught it.**

| check | why it is silent |
|---|---|
| `scripts/check-fields-written-never-read.py` | it looks for a field that is *assigned* and never read. A field only ever initialised in a struct literal has no assignment. |
| `dead_code` | the field is `pub`. On a binary crate `pub` buys nothing and costs this. |

**The measurement.** `build/never_read_probe.py` asks the cruder question --
is `.name` ever written anywhere in the crate -- over `gui/` and `apps/`:
**347** fields. The regex is rough and some of those will be read through
destructuring or a derive, so that is an upper bound. Two picked at random
were both real:

* `apps/dbviewer` `pub expanded: bool` — set in three struct literals, read
  nowhere.
* `apps/camera` `pub view_mode: GalleryViewMode` — initialised to `Grid`,
  read nowhere, so the gallery cannot switch view. There *is* a test,
  `test_gallery_view_modes`, and it asserts `GalleryViewMode::Grid.label()`
  and that `all()` has three entries: it proves the enum exists and says
  nothing about whether anything uses it.

**Why the gate was not simply extended.** It runs over every lane's crates,
and a gate that turns red on a false positive blocks lane A's and lane B's
pushes as well as ours. At an upper bound of 347 it would not be a gate, it
would be a wall. The order has to be triage first, then a baseline of what
survives, then the check — which is how
`fields-written-never-read-baseline.txt` itself describes its own 46: "a
triage queue, not an amnesty".

**Most of the count is one known cause, and that makes the triage far
cheaper than 347 suggests.** Per crate: `gui/desktop` 84, `gui/compositor`
23, `apps/torrent` 23, `apps/email` 21, then a long tail. The 84 are very
largely the fields of panels nothing can display -- `ink_level`, `stapling`,
`collation` and `submitted_at` all belong to `print_manager.rs`, which is
1961 lines and 44 tests whose only mention anywhere in the tree is `pub mod
print_manager;`. That is not a new finding; it is
`TD-C-THE-SHELL-DRAWS-FOUR-OF-ITS-FIFTY-SEVEN-MODULES`, which counted about
fifty such modules and named this one. **Do not triage those field by field.**
They are dead because their module is unreachable, and they stop being dead
the moment it is reached, so they follow that entry's decision and not this
one's.

**A second known cause takes most of the `apps/` tail.**
`TD-C-SEVERAL-APPS-DISPLAY-DATA-THAT-NOTHING-PRODUCES` already names that
pattern -- a complete, correct-looking screen over a source that does not
exist -- and says it had been found sixteen times by 2026-09-04. `apps/email`
is 21 of the 347 and is that entry's sharpest case by its own account.
`apps/torrent` is 23 and is the same shape though the entry does not name it:
`download_limit` and `upload_limit` are never read, and a bandwidth throttle
*does* exist beside them (`set_limit`, `is_unlimited`, tested), so the setting
and the mechanism are both built and nothing joins them. Wiring them would be
theatre while the app has no socket at all -- its "Tracker announce/scrape
(HTTP)" is a `TrackerProtocol::Http` enum variant and a `Display` impl.

So the count decomposes, roughly: 84 unreachable shell modules, ~60 or more
across the apps that display what nothing produces, and a genuinely-new
remainder in the long tail. **Triage the remainder; leave the other two to the
entries that own them**, or the same fields get argued about twice.

`apps/camera` was the first of the remainder and is done (2026-09-17): three
fields for a thumbnail grid that was never built, deleted with the enum that
existed only to fill a chooser nothing has. The probe now reports zero for
that crate.

**The probe reads documentation as well as it reads code, which was not what
it was for.** Twice now a dead field has been the thread that led to a
feature list claiming something nothing backs:

* `apps/explorer` opened its feature list with "Directory tree sidebar" and
  carried `tree_expanded`. The sidebar is five fixed rows.
* `apps/archivemanager` claimed "Create new archives from file lists",
  "Compression level selection", "Split archive support" and
  "Password/encryption", and carried a dead field behind each. Its backend
  takes a path and no options, and recognises encrypted members without being
  able to encrypt or decrypt. All four corrected 2026-09-17.

A field nobody reads is often a sentence in a feature list nobody can
honour, and the sentence is the more expensive of the two: state misleads
whoever edits the file, a feature list misleads everybody.

**Proper fix.** Work the list down in batches, per crate, deciding for each
field whether it is a feature to finish or state to delete. Read the module's
own doc comment alongside, and correct it in the same pass.

**"Delete it" is not always the answer, and the difference is legible.**
`apps/camera`'s three were a bare `usize`, a bare enum value and a 64-byte
zero buffer: nothing was lost by deleting them, because nothing had ever been
decided. `apps/archivemanager`'s `CreateArchiveSettings` is the other kind --
nine fields, a `Default`, and a `validate()` that returns "Output path", "No
source", and refusals for encryption and splitting on formats that do not
support them, with four tests over it. It is constructed only by those tests,
so it is as dead as the rest of this list, but it is *reasoned* dead code: a
model somebody thought through for a create-archive dialog that was never
built. Deleting that is a decision about whether the dialog is coming, which
is the question `open-questions.md` **C-Q17** already puts to the operator
about five larger cases. Left in place and named here rather than removed on
a triage pass's own authority. When it is small
enough to enumerate, add the "declared and never read" arm to the gate with
the survivors baselined. Deleting is usually right: a field nobody reads has
never worked, so nothing can depend on it.

### TRIAGE 2026-09-17 — most of these are one absent subsystem, not many mistakes

**In short:** "deleting is usually right" is wrong for the bulk of this list,
and the triage should know that before it starts. Grouping `apps/`'s 210
candidates by crate and then asking what each crate can actually *do* shows
the fields are mostly the shape of an I/O the app never performs. They are
unwired, not unimplementable — so deleting them discards a design rather
than dead weight.

**The top of the list, measured.**

| crate | candidates | what it cannot do |
|---|---|---|
| `torrent` | 23 | reach a network. Reads `.torrent` files through `safeio`; no socket at all |
| `email` | 21 | reach a network |
| `videoplayer` | 14 | open a file |
| `undelete` | 12 | open a file, or a disk |
| `ircclient` | 12 | reach a network |
| `photomanager` | 10 | open a file |
| `pdfviewer` | 10 | open a file — a PDF viewer that cannot open a PDF |
| `imageviewer` | 10 | *nothing — it reads files* (see below) |
| `mediaconvert` | 8 | open a file |

`videoplayer`, `undelete`, `ircclient`, `photomanager`, `mediaconvert` and
`email` each depend on exactly `appearance`, `guitk` and `oswindow`, and
contain zero references to `std::fs` and zero to any socket. `pdfviewer` adds
`printjob` and still reads nothing.

**And the fields are exactly what that I/O would have filled.** `email`'s are
an IMAP account (`protocol`, `auth_method`, `security`, `imap_path`,
`sync_interval_minutes`), IMAP message flags (`uid`, `answered`, `deleted`)
and a threading model. `torrent`'s are announce bookkeeping (`last_announce`,
`next_announce`, `announce_count`), per-torrent rate limits, and peer stats
(`connection_time`, `country`). None of it can be populated by a program that
never connects.

**The capabilities exist in-tree, which is what makes this a wiring gap.**
`apps/safeio` reads files and `torrent` already uses it for Ctrl+O;
`imageviewer` reads through `byteread`, `imagecodec` and `scratchdir`. A
netstack service exists as `services/netstack`, and the `net*` crates are
consumed by the kernel and by `userspace/wpa` — but by no app in the tree.

**`imageviewer` is the counter-example and is why this is a taxonomy, not a
theory.** It performs real file I/O (19 `std::fs` references) and still has
10 candidates, so absent I/O does not explain everything. Whatever its ten
are, they are a different cause and want looking at on their own.

**CORRECTION 2026-09-17: `photomanager` was measured wrong and does read
files.** Its row above says it cannot open one. It can: `import_from_disk`
reads through `std::fs::read`, parses the EXIF, and adds the photo to the
library, reached from a file picker by `open_import_dialog`. The crate's own
comment records the repair that put it there — "the whole of this
application used to be a window over a library that was never read from
anywhere".

**How the measurement went wrong, which is the transferable part.** Two errors
compounded. The survey inferred file access from a crate's *dependencies*, and
`std::fs` needs none, so a crate reading files with the standard library looked
inert. And the command that should have caught that was

    grep -rcE "std::fs|..." apps/$c/src/*.rs | awk -F: '{s+=$2} END {print s+0}'

which reports a bare count with no filename when the glob matches exactly one
file — so `awk -F:` reads an empty second field and sums zero. Every
single-file crate came back as zero regardless of what it contained. **I had
identified that exact `grep -c` behaviour hours earlier, in this same session,
and then reused the shape without thinking.** The corrected form is
`grep -rHoE ... | wc -l`.

Re-measured, production code only: `videoplayer`, `mediaconvert`, `email` and
`ircclient` do open no files, so those rows stand. `undelete`'s row stands too,
though by luck — its one apparent match is inside a doc comment.
`photomanager`'s does not.

**What this changes about the triage.** Work it crate by crate, and for each
crate establish what it can do *before* judging its fields. Where the field
is the state of an I/O the app never performs, the entry belongs in the
backlog as unfinished wiring, not in a deletion batch — and the same
judgement that settled `archivemanager` applies: bare state deletes, a
reasoned model gets asked about. Deleting `email`'s IMAP settings would
delete the specification of the mail client.

**FOLLOW-UP 2026-09-17: the real gap was decoding, and it is now half
closed.** The row's underlying complaint was right about the symptom and
wrong about the cause. `photomanager` reads files perfectly well; what it
could not do was *decode* one. It had no `imagecodec` dependency at all, so
every view drew a rounded card with the file's name printed in the middle of
it over a photograph the application had genuinely loaded and parsed the EXIF
out of.

The single-photo view now decodes the selected photograph and draws it, at
the picture's own proportions rather than the 4:3 it used to assume, and
**the grid draws thumbnails** -- generated a few per frame through the
`thumbs` crate, which was extracted from `apps/explorer` for the purpose
rather than reimplemented. Both halves are done.

The id lifecycle I expected to have to build turned out not to exist as a
problem: `thumbs::image_id` derives an id by hashing the file's path,
modification time and size, so there is no pool and no allocator to get
wrong. What the cache does own is *eviction*, and the rule worth having
inherited is that drops are announced before uploads -- the compositor
checks its budget against `held - freed + incoming`, so a batch evicting as
many thumbnails as it generates is refused precisely when the cache is
working as designed.

Two further claims in that crate's feature list failed for the same root
reason -- nothing in the application had ever held a pixel. The adjustments
(brightness, contrast, saturation, exposure, temperature) are stored per
photograph and listed in the info panel, and no pixel has ever been changed
by one. Zoom and pan were claimed in two separate entries; the only `Zoom` in
the file is the name of a slideshow transition. Both now appear under a "What
it does not do yet" heading in the module doc instead of in the feature
list.
