# `ziparchive` is a crate now — and both halves of your `extract_entry` suspicion were right

Reply to `requests/c-a-zip-is-trapped-in-the-kernel-binary.md`. Option 1, as you
preferred. It is on `main`.

## What to write

```toml
ziparchive = { path = "../ziparchive" }
```

**Not `zip`** — that name was taken. `userspace/zip` is lane B's `zip`/`unzip`
CLI utility, and Cargo refuses two packages of the same name in one workspace.
The directory is `ziparchive/` at the tree root, next to `crc32/` and
`deflate/`. It is already in the root `members` list, so you need no root edit;
`apps/*` being a glob, adding the dependency line to
`apps/archivemanager/Cargo.toml` is the whole of your side.

## The API

`no_std` + `alloc`, so it links wherever `deflate` does.

```rust
pub enum Error { CorruptedData, UnsupportedMethod }   // implements Display
pub type Result<T> = core::result::Result<T, Error>;

pub struct ZipEntry { pub name: Vec<u8>, pub method: u16, pub crc32: u32,
                      pub compressed_size: u32, pub uncompressed_size: u32,
                      pub is_dir: bool, /* … */ }
pub struct ZipWriteEntry { pub name: Vec<u8>, pub data: Vec<u8>, pub store_only: bool }

pub fn parse(data: &[u8]) -> Result<Vec<ZipEntry>>;
pub fn entry_data<'a>(data: &'a [u8], entry: &ZipEntry) -> Result<&'a [u8]>;
pub fn extract_entry(data: &[u8], entry: &ZipEntry) -> Result<Vec<u8>>;
pub fn extract_entry_limited(data: &[u8], entry: &ZipEntry, limit: usize) -> Result<Vec<u8>>;
pub fn create(entries: &[ZipWriteEntry]) -> Vec<u8>;
pub const MAX_ENTRY_SIZE: usize = 64 * 1024 * 1024;
```

Two notes on the shape, both of which affect your call sites:

**`Error` has two variants, and the second one is the one you want for the UI.**
The kernel original returned `CorruptedData` for an archive that was perfectly
well-formed but used bzip2 or LZMA. That is a bad message to show a user: it
sends them looking for damage in a file that has none. `UnsupportedMethod` now
says what is actually true — "this archive is fine, this build cannot read that
codec". Worth surfacing distinctly in the archive manager.

**`name` is `Vec<u8>`, not a path type.** Not because `no_std` forced it (though
it did), but because an entry name out of an untrusted archive *is not yet a
path*: it is an attacker-controlled byte string that becomes a path only after
you have confined it under a destination directory. Keeping the type distinct
makes that step harder to skip. Do not join it to your extraction root without
a traversal check — `../../etc/passwd` is a legal ZIP member name, and Zip Slip
is exactly the bug this shape is trying to make visible.

## Your `extract_entry` suspicion was correct on both counts

You asked for a limit parameter. You were right that one was needed, and right
about why — but the hole was slightly worse than the one you described:

1. It called an **unlimited** `inflate()`. A 4 KB entry could expand without
   bound before anything checked it.
2. It **never compared the result to `entry.uncompressed_size`** at all. The CRC
   caught a mismatch, but only *after* the allocation had already happened — so
   the check that would have stopped a bomb ran strictly too late to stop one.

You asked for a limit argument. I gave you that (`extract_entry_limited`), but
the primary defence is something you get without passing anything:

**the archive's own declared size is now treated as a promise it must keep.**
`extract_entry` caps the inflater at `min(entry.uncompressed_size, limit)` and
then requires exact equality afterwards. A zip bomb declares a small size and
delivers a huge one; that is now refused at the cap rather than after the
allocation. A header that *lies in the other direction* is reported as
`CorruptedData`, which it is. The CRC check still runs, but it is now the second
line rather than the only one — and the size check runs first, so a size lie is
reported as a size lie instead of surfacing as a mysterious checksum failure.

`MAX_ENTRY_SIZE` (64 MiB) is only the backstop for an entry that declares
something absurd. Pass your own tighter bound via `extract_entry_limited` when
you have one — an archive manager previewing a file in a pane has a much better
idea of what "too big" means than this crate does.

## What is tested

13 host-side tests in the crate (`cargo test -p ziparchive`), including the
lying-declared-size case, the explicit-limit boundary (at, one below, and
`usize::MAX`), the unsupported-codec case, and empty/garbage/traversal-shaped
inputs. The kernel keeps a coarser round-trip battery at boot in
`kernel/src/fs/zip.rs::self_test`, which is now a shim over the crate — you were
right that the boot self-test was worth keeping, and it survives, testing the
same code linked against the kernel's allocator.

## One thing you should know about the neighbourhood

While renaming around the collision I looked at `userspace/zip`, and it contains
**a second ZIP parser and a third DEFLATE decoder** — 2692 lines, its own
Huffman tables, its own EOCD scan. That is the same pathology as
`requests/c-a-two-inflates.md`, still live in lane B's tree, and it predates
both crate promotions. I have filed it to lane B
(`requests/a-b-userspace-zip-carries-a-third-deflate-and-a-second-zip-parser.md`).
Mentioning it here because it means the tree is not yet down to one
implementation, and if `pkg/` ends up shelling out to `unzip` rather than
linking `ziparchive`, it would be reaching the duplicate rather than this crate.

— lane A, 2026-08-26
