## 857. A default-application category is a view over the file associations, or it is not offered

**Date:** 2026-09-16
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** the desktop has a half-built screen for picking your default web
browser, music player, and so on. Nothing it offers has any effect: it saves
nothing and nothing asks it what you chose. Meanwhile the File Associations
program keeps a real list of "files ending in `.mp3` open with this program",
and the file manager genuinely obeys that list. So the categories become a
friendlier way of writing that same list -- picking a music player sets the
music file types -- rather than a second list nobody reads. And "web browser"
is dropped, because this operating system does not have a web browser yet, and
a row offering to choose one would be promising something that cannot happen.

**The two vocabularies.** `apps/fileassoc` stores exactly one thing:
`["associations", <extension>] -> an executable path`. `apps/explorer` reads it
on every open, so an extension is a claim with a consumer behind it. The
shell's `default_apps.rs` speaks in categories instead -- browser, email,
music, video, image, documents -- and stores nothing at all: no
`settingsfile::` call, no write, and no reference anywhere but the `pub mod`
line that declares it. Two vocabularies for one idea, one of which is obeyed.

**The decision.** A category is defined as the set of extensions it covers, and
choosing a program for a category writes that program to each of those
extensions. The category is then real in the only sense 856 accepts: the thing
that obeys it is `apps/explorer`, which was already obeying it under a
different name.

**Where that write may happen, which is narrower than it looks.**
`apps/fileassoc` is the sole writer of this file *by design*. Its `write_into`
removes every entry its own registry no longer holds -- correct, because a
cleared association must otherwise reappear at the next start -- and the
consequence is that anything else writing an association directly into the
document has it deleted at the next save, with no error raised anywhere. So a
category-wide write lives inside that program, expressed over its registry.

This was very nearly got wrong here. The shared crate was written with a
`set_category(&mut Document, ...)`, which compiled, had a passing test against
a document, and would have lost the user's choice the first time the File
Associations window saved. It is deleted, and the reason is recorded on
`CATEGORIES` where the next person will look for the missing function. What
the crate offers a writer instead is `Category::extensions()`; the writing is
the owner's business. The general rule: **a second writer of a file with a
pruning owner is not a feature with a bug in it, it is a feature that cannot
work.**

**Why not the other way round** -- store categories and teach explorer to
resolve an extension through them? It adds a lookup hop and a second file to
keep in step, and it makes the association list, which is the thing that
actually works today, the derived copy. The rule that decided it: when one of
two models has a consumer and the other does not, the one with the consumer is
the real one.

**Why "web browser" and "email" are omitted rather than shown greyed out.**
Both are *protocol* categories -- a browser answers `http:`, a mail client
answers `mailto:` -- and this rule is defined over extensions, so neither can
be expressed by it. That is not the reason to drop them, though; it is the
symptom. The reason is that neither has a consumer: there is no browser in
`apps/` at all, and the only two places in the tree that mention `mailto` are
the dead module itself and `apps/qrcode`, which *encodes* the string into a QR
image rather than dispatching it to anything. A row that cannot be satisfied is
the fabrication 856 is about, and a disabled row is the same fabrication
relocated to a label. When a browser exists, or when something resolves a
protocol the way `apps/explorer` resolves an extension, the category costs one
table entry and a second resolver.

**So the shipped set is three:** Music, Video and Images.

"Documents" is dropped for a different reason than the other two, and it is the
sharper one. Its programs exist and its subjects have extensions, so it passes
both tests above -- but its members are not one kind. A `.txt` opens with the
editor and a `.pdf` with `pdfviewer`, and both are documents. A category is
defined here as a set of extensions that share one program, so a Documents row
would not express a preference, it would *impose* a wrong association on half
its members the moment it was used. The other two omissions drop a row that can
do nothing; this one drops a row that can do harm.

**What this settles.** `gui/desktop/src/default_apps.rs` -- 2,325 lines --
goes, under 815's "screens you open move to the Settings app and the shell's
copies go", on 853's precedent that a shell panel with no consumer is deleted
rather than finished. Its category *knowledge* survives as the extension table
this decision defines, which is the part of it that was worth keeping.
