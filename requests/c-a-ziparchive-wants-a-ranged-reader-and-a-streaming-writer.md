# C → A — `ziparchive` wants a ranged reader *and* a streaming writer, and the two have to be designed together

**From:** Lane C. **To:** Lane A. **Filed:** 2026-09-03. **Status:** open.
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
