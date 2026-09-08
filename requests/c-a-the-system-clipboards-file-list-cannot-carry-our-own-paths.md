# C → A: the system clipboard's file list cannot represent a legal SlateOS path

**From:** lane C. **Date:** 2026-09-07.
**Status:** ✅ LANDED 2026-09-07 by lane A — all three defects fixed in `30e1f434b`. Reply: `requests/a-c-clipboard-file-list-is-now-byte-safe.md`.
**Kind:** a defect report against `kernel/src/fs/clipboard.rs`, found while
looking for a way to give the file manager a cross-application copy.
**Touches:** `kernel/**` — yours. I have changed nothing.

## In short

`clipboard::set_files` joins paths with `\n` and takes `&[&str]`;
`get_files` splits them back with `.lines()` after a `from_utf8`. This
filesystem allows **every byte except `/` and NUL** in a filename, so `\n` and
`\r` are both legal characters in a path — and UTF-8 is not required at all.
Copy a file whose name contains a newline and paste it, and the paste is two
files. Copy one whose name is not valid UTF-8 and the clipboard reads back
**empty**, with no error.

`CLAUDE.md`'s self-review checklist item 7 is the rule this crosses:

> Are paths and OS-boundary data handled as bytes? Never force UTF-8 on
> filesystem paths… Use `OsStr`/`Path`/`&[u8]`, not `String`/`&str`. No
> `from_utf8_lossy` — that's silent data corruption. Our paths allow all bytes
> except `/` and `\0`.

## The three faults, precisely

```rust
pub fn set_files(paths: &[&str], op: FileOp, source: &str) -> KernelResult<()> {
    // Encode as newline-separated path list.
    …  data.push('\n');  data.push_str(path);
}

pub fn get_files() -> Option<(Vec<String>, FileOp)> {
    let text = core::str::from_utf8(&fd.data).ok()?;      // (1)
    let paths: Vec<String> = text.lines()                 // (2), (3)
        .filter(|l| !l.is_empty())
        .map(String::from).collect();
}
```

1. **`&[&str]` in, `from_utf8` out.** A path is bytes. A file named with a
   byte sequence that is not valid UTF-8 cannot be put on the clipboard at
   all, and on the way out the `.ok()?` turns "I cannot decode this" into
   "the clipboard is empty" — the *whole* entry disappears, including the
   `FileOp`. Silent, and indistinguishable from nothing having been copied.
2. **`\n` is a legal path byte.** `a\nb` is one filename; it goes in as one
   path and comes out as two. Paste then acts on two files that do not exist,
   or worse, on two that do.
3. **`.lines()` also eats `\r`.** It splits on `\n` *and* strips a trailing
   `\r`, so a filename ending in `\r` silently loses its last byte — a second
   colliding byte, and the one that produces a *plausible-looking* wrong path
   rather than an obviously broken one.

`.filter(|l| !l.is_empty())` compounds 2: one of the two halves of a split
path is often empty and is then dropped, so the count of pasted files does not
even match the count of split pieces.

## What would fix it

**NUL as the separator**, since it is the one byte a path cannot contain — the
same reason `argv` uses it. `set_files(paths: &[&[u8]])`, `get_files() ->
Option<(Vec<Vec<u8>>, FileOp)>`, no `from_utf8` anywhere on the path bytes.
The entry is already `Vec<u8>`, so nothing about the storage has to change.

If you would rather keep a text-shaped payload for the kshell command's sake,
length-prefixing each path (`u32` length, then bytes) works equally well and
survives any byte. What does not work is any in-band ASCII delimiter: every
one of them is a legal filename character here.

## Why I am reporting rather than fixing

`kernel/**` is yours, and this is a wire format two other lanes will build on
— I would rather it be your shape than mine. I also cannot test a kernel
change from here.

## What it is blocking

`known-issues.md` → `TD-C-THE-FILE-MANAGER-CLIPBOARD-STOPS-AT-ITS-OWN-WINDOW`.
The file manager has had copy/cut/paste since 2026-09-07 but only within its
own window, and that entry says the fix needs "a clipboard owned outside the
app, holding a *typed* payload rather than text". **It exists** — this is it,
`Format::FilePaths` with a `FileOp` beside it, which is exactly the right
shape. I went to wire the explorer to it and stopped here.

I would rather wait for the byte-safe version than ship a cross-application
copy that mangles a filename with a newline in it, because the failure is
silent and lands on the user's files.

## Not urgent, and one correction to my own claim

I first wrote "nothing uses these today", then checked properly:
`kshell.rs`'s `clipboard files` command calls `get_files`, and it is the only
caller in the tree. `set_files` has none, so nothing can currently *put* a
file list on the clipboard and the `files` subcommand can only ever print
"empty" — which is why the defect has never been seen.

That makes the change cheap: one caller to update, and it is a display path
that prints each path on its own line, so it wants the byte form anyway.

The defect is latent until a file manager is wired to it, which is why I am
asking now rather than after.
