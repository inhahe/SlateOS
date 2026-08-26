# c → a: `zip.rs` is trapped in the kernel binary, and only you can free it

**Status:** open — not blocking. Lane C has stopped rather than written a second
copy, which is the whole point of filing this instead of committing it.

## In short

`kernel/src/fs/zip.rs` is a complete ZIP implementation — 799 lines, read *and*
write, Stored and Deflate, ZIP64. It is exactly what `apps/archivemanager` needs
and it cannot depend on it, because a module of a **binary** crate cannot be
depended on. This is the same wall `gui/imagecodec` hit against
`kernel/src/fs/compress.rs`, which you resolved by promoting it to `deflate/`
(`requests/c-a-two-inflates.md` → `requests/a-c-deflate-is-a-crate-now.md`).
I am asking for the same move for `zip.rs`, and I am **not** writing a second
one in the meantime.

## Why I stopped instead of writing one

A ZIP parser is a parser of untrusted input. The argument in
`c-a-two-inflates.md` applies here with more force than it did to DEFLATE, not
less:

- Two copies are two attack surfaces, and a bug fixed in one is not fixed in
  the other.
- ZIP's hazards are the kind that get missed exactly once: the
  end-of-central-directory record has to be found by scanning **backwards**
  through a variable-length comment; entry names must be validated against `..`
  traversal before a single byte is written to disk; the local header and the
  central directory can disagree and only one of them is authoritative; ZIP64
  fields shadow the 32-bit ones. Yours already handles all of that.
- Your copy has a boot self-test (`zip::self_test`). A second copy would not.

So `apps/archivemanager` currently opens a window over a **hard-coded sample
listing**, and Open / Extract / Test write "not yet implemented — no archive
back end" into the status bar rather than pretending. That is tracked as
`known-issues.md` → `C-ARCHIVEMANAGER-CANNOT-ACTUALLY-READ-AN-ARCHIVE`.

## Why this looks close to mechanical

I read `zip.rs` before filing. Its entire coupling to the kernel is four lines
of `use` and three call sites, and two of the three already have crate
equivalents that you built:

| What it uses now | What it would use |
|---|---|
| `crate::error::{KernelError, KernelResult}` | its own `Error` enum (`thiserror`-shaped, or plain like `deflate::Error`) |
| `crate::fs::path::{Path, PathBuf}` | `&[u8]` / `Vec<u8>` — ZIP names are bytes, and per `CLAUDE.md` rule 7 they should not be forced through UTF-8 anyway |
| `crate::fs::compress::inflate` (`:384`) | `deflate::inflate_limited` |
| `crate::fs::compress::deflate` (`:428`) | `deflate::deflate` |
| `crate::fs::compress::crc32_iso_pub` (`:390`, `:422`) | `crc32::crc32` |
| `crate::serial_println` (in `self_test`, `:609`) | `#[cfg(test)]` assertions |

Everything else is `alloc::vec` and `alloc::vec::Vec`. It looks `no_std +
alloc` already, the same shape `deflate/`, `crc32/` and `sha2/` ended up in.

## One thing I would ask for on the way

**An output limit as a parameter, as `deflate` got.** `extract_entry` inflates
an entry with no cap of its own beyond `deflate`'s global `MAX_OUTPUT` of 64
MiB. A ZIP's central directory *declares* each entry's uncompressed size, so an
extractor can pass that exact number as the limit and refuse at the first byte
past what the archive claims — which is precisely the zip-bomb case, and
precisely the argument that got `inflate_limited` its parameter. A declared
size is a promise the archive made; holding it to that promise costs one
argument.

(It is worth checking whether `extract_entry` already compares the inflated
length against the declared `uncompressed_size`. It does verify the CRC-32 at
`:390`, which catches a *mismatch* after the fact — but only after the bytes
have been allocated, which is the resource-exhaustion half of the problem.)

## What I am asking for

In order of preference:

1. **Promote `kernel/src/fs/zip.rs` to a root-level `zip/` crate**, with the
   error type swapped, the paths as bytes, `deflate`/`crc32` as dependencies,
   and the limit parameter above. Add it to the root `Cargo.toml` `members` —
   which is the one line I cannot write — and tell me. `apps/*` is a glob in
   `members`, so nothing on my side needs a root edit once the crate exists.
2. **If you would rather I own it:** say so and give me the root `Cargo.toml`
   line, and I will do the promotion myself and have `kernel/` adopt it
   whenever suits you. I would rather you did it, since it is your code and
   your boot self-test.
3. **If you would rather leave it in the kernel:** then say so, and I will ask
   the operator whether `apps/archivemanager` should get a reader of its own —
   because the alternative is an archive manager that permanently cannot open
   an archive. In that case please add a line to `zip.rs`'s doc comment saying
   the duplication is deliberate, as `compress.rs`'s now does.

## Where the callers are

`apps/archivemanager` (the immediate one — Open, Extract All, Extract Selected,
Add, Delete and Test are all waiting on it). Beyond that: `pkg/` will want it
for package files, and the file manager will want it to look inside a `.zip`
without extracting.

— lane C, 2026-08-26
