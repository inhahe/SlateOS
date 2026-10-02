## `TD-C-THE-ARCHIVE-CREATION-OPTIONS-ARE-A-STRUCT-NOBODY-CALLS` (lane C, 2026-09-18) -- **CLOSED 2026-09-22, by deletion**

> **Closed by removing the options, not by building the dialog.**
> `CreateArchiveSettings`, `EncryptionSettings`, `SplitSettings`,
> `CompressionLevel` and the whole `ArchiveOperation` enum are gone -- about
> 180 lines of type, `Default`, validation and tests that no code path could
> reach.
>
> **The prescription below is wrong in one clause and it is worth saying
> which.** It asks for "a new-archive dialog offering only what the backend
> can actually write -- today that is ZIP at the levels `CompressionLevel`
> names". There are no such levels: `ziparchive::create` takes
> `store_only: bool` and nothing else, so `Fast`, `Normal` and `Best` were
> three names for one behaviour. The module doc in that same crate had said so
> the day before -- "the backend has no compression-level parameter at all" --
> and this entry, written afterwards, contradicted it. **Two documents about
> one crate disagreed, and the older one was right.**
>
> What the backend can write is an empty ZIP at a path, which is exactly what
> the create flow already does, so there was nothing for a dialog to offer.
>
> **And the absence was bigger than the entry found.** `ArchiveOperation` --
> the enum holding the `Create { format, level }` variant -- appears exactly
> once in the crate: its own declaration. The entry's own closing note says to
> ask what *holds* a field before asking what writes it; following that one
> step further than it did turned four dead types into five.

**In short:** `apps/archivemanager` can make a new archive, and it always
makes the same kind: an empty ZIP at normal compression, no password, not
split, no comment. The program contains a full set of options for this --
format, compression level, encryption, splitting, whether to keep empty
folders, whether to store full paths -- written out as a type with its own
validation and its own tests. Nothing in the program ever builds one. The
menu item that creates an archive calls a different function that takes a
path and nothing else.

**Where it lives.** `CreateArchiveSettings` in
`apps/archivemanager/src/main.rs:1170`, with `Default`, a `validate()` that
checks for an empty password when encryption is on, and tests at ~4822. Live
code mentions the type exactly three times: its declaration, its `Default`,
and its own `impl`. The creation path is `create_archive(&mut self, path:
&Path)` at ~3064, which calls `backend::create_empty(path)`.

**Why the frozen-flag survey pointed here.** It reported
`CreateArchiveSettings::format` as read-and-never-written, which is true and
understates it: the whole struct is unreached, so every field in it is. A
survey that reports one field of a dead struct is telling you about the
smallest visible part of a larger absence. Worth remembering when a row looks
oddly specific -- ask what holds the field before asking what writes it.

**Why this is not being fixed in passing.** Making the options reach the
creation path is not wiring, it is building the feature: a new-archive dialog
with a format list, a compression control, a password field, and a split
size -- and `backend::create_empty` writes a ZIP, so honouring `format` means
writers for the other formats that do not exist. Shipping a dialog whose
format list has one working entry would be the defect this lane keeps
finding, one level up.

**What is safe about it today.** Creating an archive works and says what it
did. Nothing claims the options exist: they are not drawn anywhere, so the
program is silent about them rather than lying about them. That is the right
order to leave it in, and it is why this is a gap rather than a bug.

**What the repair wants, in order.** A new-archive dialog offering only what
the backend can actually write -- today that is ZIP at the levels
`CompressionLevel` names -- and the rest of the fields removed from
`CreateArchiveSettings` until there is a writer behind them. An option struct
should not outrun its backend; that is how a list with one working entry gets
shipped.
