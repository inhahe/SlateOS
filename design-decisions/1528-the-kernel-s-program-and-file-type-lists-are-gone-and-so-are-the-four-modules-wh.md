## 1528. The kernel's program and file-type lists are gone, and so are the four modules whose only job was typing files with them

**Date:** 2026-10-02 · **Decided by:** Operator for the ten lists (§1425, C-Q20: one list of programs, in userspace, "the kernel's registry goes" once everything in it is carried over); Claude (autonomous) for the four modules removed with them and for what the dependents keep · **Lane:** A (lane C asked: `requests/c-a-the-kernels-app-registry-and-the-first-screen-hint.md`, item 1)

**In short:** the kernel kept ten lists of programs and file types: which
programs exist, what opens what, which files are pictures or songs. The
operator decided on one list, in userspace. Lane C built that list and
checked every item of the kernel's ten against it (`gui/programs/INVENTORY.md`).
The ten are now deleted, with their kernel-shell commands and `/proc` files.
Four more modules went with them, because sorting files by type was their
whole job and they could not do it without the lists: the file-details
reader, the thumbnail maker, the column chooser and the search index built on
the details reader. Nothing a program uses is lost. No program read any of
the fourteen, and the file manager and the toolkit each have their own
version.

**What went, and where its job is done now:**

| Module | What it was | Where that lives |
|---|---|---|
| `appregistry`, `defaultapps`, `associations`, `pinnedapps`, `startmenu`, `applaunch`, `openwith`, `appstore` | programs, roles, defaults, pins, launcher entries (the last two empty) | `gui/programs` and the shell's first start, item by item in `INVENTORY.md` |
| `mime`, `filetype` | extension and content-signature tables, type descriptions and icons | `guitk::filetypes` (`INVENTORY.md`) |
| `fileinfo` | read ID3, EXIF, PNG text, PDF, WAV and ELF headers into fields for the explorer's detail columns; chose its parser by `mime::detect` | `apps/explorer/src/columns.rs` (dimensions, duration, bitrate, artist, ...) |
| `preview` | thumbnails, chosen by `mime::detect` | `gui/thumbs` |
| `columnview` | which detail columns a folder shows, from the types `mime::detect` found in it | `apps/explorer` (`columns.rs`, `columnprefs.rs`) |
| `findex` | an in-memory index of `fileinfo`'s fields (a picture's size, a song's artist), with a query language | `userspace/indexer`, the background file indexer, which indexes names, sizes and dates but not yet what is inside a file. No program ever read the kernel's index |

Also gone: the kernel-shell commands `mime`, `assoc`, `openwith`, `filetype`,
`appreg`, `startmenu`, `defaultapps`, `appstore`, `pinnedapps`, `applaunch`,
`fileinfo`, `findex`, `preview` and `columnview` (with their aliases), the
thirteen `/proc` files of the same names (`mime` had none), and the modules'
boot self-tests. About 13,000 lines.

**The rule for the four, and why they and not others.** A module went if
sorting files by type was its core, so that without the lists it would do
nothing. That covers `fileinfo`, `preview`, `columnview` and `findex`
(through `fileinfo`). A module that only *used* the lists for a field or two
kept everything else:

| Kept | What it lost | Why that is right |
|---|---|---|
| `properties` | the type, MIME type, "opens with" and the details read by `fileinfo` | the kernel no longer knows any of them; size, dates, owner, permissions, checksums and folder contents remain |
| `contextmenu` | "Edit" for text and "Set as wallpaper" for pictures | a kind-specific item is what an extension adds (the design spec's lines 722-731); the program that knows the kind registers it |
| `systray` | the program's own "start in the tray" and "has a tray icon" | the registry said no for every program it held (`INVENTORY.md`: the same for every program), so with no user override the answer is still no |
| `sysdiag` | its check that some program was registered | the kernel neither reads nor grades the list of installed programs |

| Option | For | Against |
|---|---|---|
| **The ten, plus the four that cannot work without them (chosen)** | no list survives in the kernel in any form; what is deleted is either carried (the inventory) or done better in userspace | the four were not named in §1425; if they are wanted back in the kernel, it is a revert |
| The ten only; the four keep a small type sniffer of their own | the four keep working | a second file-type list in the kernel, the very thing §1425 exists to end; and still no program reads them |
| The ten only; the four lose type detection and stay | nothing beyond the request is deleted | modules whose every answer is "unknown type", kept with no reader |

**Not done:** the wider "most of `fs/` has no consumer but `/proc`" backlog
(roadmap) is untouched: this rule does not say a module without a reader
goes, only that one which cannot work without the lists goes with them.
`thumbcache` stays, though `preview` was its only producer; it still answers
`storageclean`.
