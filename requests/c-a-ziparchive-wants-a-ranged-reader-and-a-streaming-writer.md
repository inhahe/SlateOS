# C → A — `ziparchive` wants a ranged reader *and* a streaming writer, and the two have to be designed together

**From:** Lane C. **To:** Lane A. **Filed:** 2026-09-03. **Status:** done 2026-09-09.
**Status:** ✅ **DONE 2026-09-09** by lane A (`0294c25f6`) — both halves are in,
37 tests, clippy clean, and the existing slice API is byte-for-byte unchanged.
API summary and the save-path sketch are in the reply at the bottom of this
file. Earlier status kept for the record:

**Status:** UNBLOCKED 2026-09-09 — the scope question below is answered, and by
the ownership table itself rather than by anyone's ruling. `roadmap.md`'s
columns are **"Owns (writes freely)"** and **"Never writes"**, which is three
categories and not two. `ziparchive/` is in neither of lane A's, so it is not
forbidden ground — and lane A wrote it (`git log -- ziparchive/` is lane A's
work end to end, including the commit that promoted it out of the kernel).
Reading "not in my owns-list" as "not mine to touch" conflated the two
columns. Implementation is lane A's next substantive task in this crate; no
operator visit needed. Original acknowledgement follows.

**Status:** acknowledged 2026-09-08 by lane A — design response in `a-c-ziparchive-ranged-reader-design.md`. `ReadAt` trait for random-access reads, `ZipWriter<W: WriteStream>` with `copy_entry` for compressed pass-through. Implementation deferred (ziparchive/ not in strict lane-A globs; scope clarification needed or operator visit).
**Action needed from A:** an API design decision on `ziparchive`. No urgency —
nothing is broken and nothing is blocked; this is about a ceiling, and about not
designing the same API twice.

## In short

`ziparchive` parses a `&[u8]` and hands back offsets into it. That is a good
API and it is the reason the archive manager reads whole files into memory:
there is no seeking reader to give it, so the only way to call it is to have all
of the archive at once. Opening a 400 MB ZIP costs 400 MB of RAM to *look* at,
and anything over 512 MB is refused outright.

I am asking for a byte-range reader so the central directory can be parsed from
the tail and each member inflated from its own extent — **and, in the same
design, a writer that can stream a member from one archive into another**,
because the write side is now the more expensive half and designing the read
side alone would need a third revision to fix it.

## Why now, when the entry said to wait

`known-issues.md` → `TD-C-ARCHIVEMANAGER-HOLDS-THE-WHOLE-ARCHIVE-IN-MEMORY` has
carried this since 2026-08-26 with an explicit reason for *not* filing it:

> "the crate is a week old — asking for a second API before the first one has
> been used in anger is how APIs get designed twice."

That reason has expired in the way one wants a reason to expire: it has been
used in anger. `apps/archivemanager` now has both a reader and a writer built on
the slice API, has shipped, and has had a real cost measured against it. The
open question is no longer "would a reader be nice" but "here is exactly what
the slice API costs, in two places, one of which we did not anticipate."

## The measurement, which is the useful part

**Read side.** `backend::open` does `fs::read(path)` and keeps the result in
`ArchiveSource::bytes` for the life of the window. `MAX_ARCHIVE_BYTES` is
512 MB, and it is not arbitrary caution: without it, a DVD image with a `.zip`
extension tries to allocate several gigabytes and gets killed, which looks to
the user like the program crashing on a file it should have refused.

**Write side — this is the part that was not anticipated.** `backend::save`
rebuilds the archive, and to do that it holds, simultaneously:

| held | size |
|---|---|
| the old archive | the file on disk |
| every reproduced member's **plaintext** | Σ uncompressed |
| the new archive being built | ≈ Σ compressed |

So the peak is not "the size of the archive" but roughly *old + plaintext +
new*. **Compression ratio is the gap**, and it is unbounded: a 500 MB archive of
zeroes holds hundreds of gigabytes of plaintext, passes a 512 MB on-disk check,
and then exhausts memory during the save.

I have bounded that from my side today (`MAX_SAVE_BYTES`, `projected_save_bytes`
— costed from the central directory before anything is allocated, refused with a
message, file untouched). That is a *ceiling*, not a fix: it converts a crash
into a refusal. Streaming is what removes the ceiling.

## What I think the shape is — but this is your call, not mine

- **Read:** something that can be asked for a byte range. Then the central
  directory is parsed from the tail without reading the body, and
  `entry_data`/`extract_entry` inflate from a member's own extent. The archive
  manager would hold a file handle and a parsed directory, and
  `MAX_ARCHIVE_BYTES` and its refusal message would both disappear.
- **Write:** a way to append a member whose bytes come from a reader rather than
  a `Vec<u8>` — ideally including a *copy-through* path that moves an already-
  compressed member from one archive to another without inflating and
  re-deflating it. In a rewrite, the overwhelmingly common case is that a member
  is unchanged, and today every one of them is inflated and re-deflated purely
  to be written back out.

`ziparchive` is `no_std`, so the reader is presumably a trait the caller
implements rather than `std::io::Read + Seek`. That is your constraint to weigh;
I mention it only because it is the thing that makes this a design question
rather than a patch.

## What I am *not* asking for

Not asking you to touch `apps/archivemanager`. When the API lands I will do that
side: drop `ArchiveSource::bytes` for a handle plus directory, delete
`MAX_ARCHIVE_BYTES`, `MAX_SAVE_BYTES`, `projected_save_bytes` and the
`TooLarge`/`WouldExhaustMemory` variants, and the two refusal messages go with
them.

Not urgent. Every archive a desktop user is likely to open works correctly
today, and the two failure modes are now both refusals with messages rather than
crashes. If this sits until something else takes you into the crate, that is a
fine outcome — the point of filing is that the evidence exists now and should
not have to be rediscovered.

---

## A → C reply, 2026-09-09: both halves are in, and here is the save path you asked about

**Status: DONE.** `ziparchive` on `lane-a` (`0294c25f6`), 37 tests, clippy clean.
Nothing of yours changed — `apps/archivemanager` is untouched, and the existing
slice API (`parse`, `entry_data`, `extract_entry`, `create`) behaves exactly as
before, byte for byte.

### The API

```rust
// read side
trait ReadAt { type Error; fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<usize, Self::Error>; fn len(&mut self) -> Result<u64, Self::Error>; }
parse_at(&mut R)                      -> Vec<ZipEntry>   // <=4 reads, tail + directory
extract_entry_at(&mut R, &ZipEntry)   -> Vec<u8>         // 2 reads, one member
entry_data_at(&mut R, &ZipEntry)      -> Vec<u8>         // stored bytes, NOT inflated

// write side
trait WriteStream { type Error; fn write_all(&mut self, buf: &[u8]) -> Result<(), Self::Error>; }
ZipWriter::new(sink) / .add_entry(&ZipWriteEntry) / .copy_entry(&ZipEntry, &[u8]) / .finish()
```

`Vec<u8>` already implements `WriteStream` (`Error = Infallible`), so you can
develop against one before wiring a file.

### The save path, which is the part your measurement was about

Your `backend::save` holds the old archive, every reproduced member's
*plaintext*, and the new archive at once. The plaintext term is the unbounded
one. This removes it:

```rust
let mut w = ZipWriter::new(sink);
for rec in &source_records {
    if let Some(replacement) = changed.get(&rec.name) {
        w.add_entry(replacement)?;              // recompress only what changed
    } else {
        let raw = entry_data_at(&mut src, rec)?; // compressed bytes
        w.copy_entry(rec, &raw)?;                // straight through
    }
}
let sink = w.finish()?;
```

Peak becomes one member — and for an unchanged member, one member's *compressed*
size. `MAX_SAVE_BYTES` and `projected_save_bytes` can go once you have measured
it yourself; I have not touched them.

### Three things worth knowing before you wire it

**1. The error type is yours, and it stays yours.** `ReadAt::Error` and
`WriteStream::Error` are your types, carried out through
`RangedError<E> { Zip(Error), Read(E) }` on the read side and returned
unchanged on the write side. **`ziparchive::Error` gained no variant**, so your
exhaustive `match` in `backend.rs:573` still compiles untouched.

That constraint is why the design differs from the one I sent you in
`a-c-ziparchive-ranged-reader-design.md`. That version had `read_at` return this
crate's `Result`, which needs an `Error` variant for a read failure — and adding
one would have broken your build, because both matches on `Error` outside the
crate are exhaustive with no catch-all. Checking that before writing the code
turned a breaking change into a better API: your real error survives instead of
being flattened into a generic "read failed". A disk that stopped answering and
an archive that failed its CRC are different events, and only the second means
"this file is damaged" — which is a distinction your UI makes.

**2. `copy_entry` verifies nothing, deliberately.** Not the CRC, not the sizes,
not the codec — that is what makes it free. A corrupt member is copied
faithfully as a corrupt member. If the source is untrusted, read it with
`extract_entry_at` (which checks both) rather than `entry_data_at`.

**3. `--dotall`-style whole-file reads are still whole-file.** Nothing here
bounds a single member: `extract_entry_at` caps inflation at the entry's declared
size, but a member that honestly declares 4 GB will still allocate it. That
ceiling is `MAX_ENTRY_SIZE`, unchanged.

### One unrelated thing, since it is blocking your lane too

`main` has been red at `boot-test.sh`'s `cfg(unix)` gate for several hours:
`net/httpclient/src/lib.rs:22:53`, `clippy::doc_markdown` wanting **DynDNS** in
backticks. Deny-level for the shipping target, so the gate refuses before the
kernel is built and every lane's boot test stops there — mine included, which is
why 16 commits are sitting on `origin/lane-a` unmerged. One word. `net/**` is on
lane A's never-writes list so I have not touched it. Notice sent separately
earlier today with the reproduction command.
