## `TD-C-THE-FILE-MANAGER-CLIPBOARD-STOPS-AT-ITS-OWN-WINDOW` (lane C, 2026-09-07)

**In short:** copying a file in the file manager and pasting it in the file
manager works. Copying a file there and pasting it *anywhere else* does not,
because the file manager keeps its copied-files list in a variable of its own
rather than handing it to the system. Nothing tells the user that; Ctrl+C looks
identical either way, and the paste simply does not happen in the other window.

**Where it lives.** `apps/explorer/src/main.rs` -> the `clipboard` field, and
`copy_selected` / `cut_selected` / `paste` around it. The field is an
`Option<ClipboardOp>` holding `Copy(Vec<PathBuf>)` or `Cut(Vec<PathBuf>)`, and
it is private to the one running `ExplorerState`.

**Why this entry exists.** It replaces a wrong claim.
`TD-C-EXPLORER-HAS-NO-EDITING-KEYS` said the file manager had *no* clipboard
and that Ctrl+C could not be wired until one was built. It has had one all
along -- what it lacks is a *shared* one. Wiring the keys was therefore
correct, and the residue is this narrower thing.

### Update 2026-09-07: the clipboard this entry asks for exists, and cannot carry our paths

**The system clipboard is already built, in the kernel.** `fs::clipboard`
(`roadmap.md` line 2470, marked done) has exactly the shape this entry asks
for: a `Format::FilePaths` variant and a `FileOp` enum (`Copy`/`Cut`) beside
it, explicitly "for file manager copy/cut", with `/proc/clipboard` and a
kshell command. The open question below -- toolkit, compositor or service --
was answered before it was asked. There are two *more* clipboards in the tree
that are not it: `guitk`'s text one, and `gui/clipboard`, a written service
binary that nothing links to or talks to.

**I went to wire the explorer to it and stopped.** `set_files` joins paths
with `
` and takes `&[&str]`; `get_files` splits them with `.lines()` after
a `from_utf8`. This filesystem allows every byte but `/` and NUL, so `
` and
`
` are both legal in a filename and UTF-8 is not required at all. A file
named `a
b` pastes as two files; a file whose name is not valid UTF-8 makes
the whole clipboard read back *empty*, with no error, because the `.ok()?`
turns a decode failure into "nothing here".

Reported as `requests/c-a-the-system-clipboards-file-list-cannot-carry-our-own-paths.md`
with a proposed fix (NUL as the separator -- the one byte a path cannot
contain -- or length-prefixing). `kernel/**` is lane A's, and this is a wire
format two other lanes will build on, so it should be their shape.

**This entry stays open**, and its blocker is now specific: not "there is no
system clipboard" but "the system clipboard's file list cannot represent a
legal path". Wiring the explorer to it as it stands would mangle filenames
silently, on the user's own files, which is worse than copy stopping at the
window.

**What the proper fix is.** A clipboard owned outside the app, holding a
*typed* payload rather than text: a file reference is a list of paths plus
whether it was a copy or a cut, and flattening it to a newline-joined string
loses the cut/copy distinction and breaks on any path containing a newline
(our paths allow every byte except `/` and NUL). `gui/toolkit` has a clipboard
for text only. Whether the system clipboard belongs in the toolkit, the
compositor or a service is the open part -- text and files want the same
ownership rules and different payloads, and a text-only clipboard grown a
files case by accident is how the two end up disagreeing about which one is
authoritative.

**How you would notice.** Copy a file in the file manager, switch to any other
window, press Ctrl+V. Nothing arrives, and nothing says why.
