## 615. An archive entry's declared size is a promise it must keep, checked before the allocation and again after

*Date: 2026-08-26*

**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** A ZIP file states, in its own header, how big each item inside it
is before you unpack it. The old code ignored that number while unpacking and
only noticed a problem afterwards, using a checksum — which means a booby-trapped
archive claiming to hold 4 KB but actually expanding to 4 GB got its 4 GB
allocated first and was rejected second. That is backwards: the check that
would have prevented the damage ran after the damage. The decision is to treat
the declared size as binding — refuse to produce more than it, and afterwards
refuse anything that is not *exactly* it. The cost is that archives whose
headers disagree with their contents are now rejected rather than leniently
unpacked.

This came out of promoting `kernel/src/fs/zip.rs` into the root `ziparchive`
crate (§610's rule applied a third time, at lane C's request in
`requests/c-a-zip-is-trapped-in-the-kernel-binary.md`). Lane C had asked only
for an output-limit *parameter*; the parameter alone would not have fixed this,
because every existing caller would have kept passing the default.

**Glossary, once.** *Decompression bomb* (or "zip bomb"): a small archive
crafted to expand enormously, so that merely opening it exhausts memory.
*CRC-32*: a checksum stored alongside each entry, which detects damaged bytes
but only once you already hold all of them.

### What the old code did

```rust
// before
let decompressed = inflate(raw)?;          // no bound of any kind
if crc32(&decompressed) != entry.crc32 { return Err(CorruptedData); }
```

Two separate faults, and the second is the one that is easy to miss:

1. The inflater was **unbounded**. Nothing limited the output.
2. The result was **never compared to `entry.uncompressed_size`** at all. The
   header's own statement of the size was read into the struct, displayed by
   `unzip -l`, and then never used as a check.

The CRC did eventually reject a bomb — a bomb's payload does not match its
declared checksum — so the code was not *wrong* so much as ordered wrongly. The
defence worked only after the attack had already succeeded at the thing it was
trying to do, which was to make us allocate.

### What it does now

```rust
let declared = usize::try_from(entry.uncompressed_size).unwrap_or(usize::MAX);
let cap = declared.min(limit);
let decompressed = inflate_limited(raw, cap)?;      // refuses past the cap
if decompressed.len() != declared { return Err(CorruptedData); }
if crc32(&decompressed) != entry.crc32 { return Err(CorruptedData); }
```

The ordering is the point. The cap stops a bomb *during* inflation. The equality
check catches a header that lies in the other direction. The CRC then does what
a CRC is for — detecting corruption in bytes we have already agreed to hold.

**Why check size before CRC, when the CRC would catch it anyway?** Because the
two failures deserve different reports. A size that disagrees with the payload
is a structural fault in the archive; a checksum that disagrees is damage to the
data. Running the CRC first would report every size lie as "checksum failed",
which sends whoever is debugging it looking for bit rot in a file whose problem
is that its index is wrong.

### The tradeoff, stated honestly

**Against.** This is strictly less permissive than before. A ZIP whose central
directory disagrees with its actual payload used to extract fine if the CRC
happened to match, and now does not. Such archives exist: buggy writers, files
truncated and repaired, streaming writers that patched the local header but not
the central one. We will reject some archives that other tools open. If that
turns out to bite real files, the right response is a `--force`-style opt-in on
the *caller*, not a relaxation here.

**For.** The permissiveness bought nothing. An entry whose size we cannot trust
is an entry whose contents we cannot trust either — we would be extracting bytes
into a file while knowing the archive is internally inconsistent. And the strict
version is what makes the bomb case cheap: the refusal costs one comparison and
happens before the allocation, rather than costing the allocation and happening
after.

**Rejected alternative: a limit parameter only** (what was asked for). It puts
the burden on every caller to know a good bound, and callers do not — the
archive is the only party that knows how big its contents are. A parameter is
still offered (`extract_entry_limited`) for callers who genuinely have a tighter
bound, but it narrows the cap rather than being the cap.

**Rejected alternative: cap at a fixed `MAX_ENTRY_SIZE` only.** 64 MiB still
remains as a backstop for an entry that declares something absurd, but as the
*primary* defence it is far too loose: a bomb that declares 1 KB and delivers
64 MiB would sail through, and it is the declared size that makes that
detectable.

### The related call: entry names stay bytes

`ZipEntry.name` is a `Vec<u8>`, not the kernel's `PathBuf`. `no_std` forced the
question, but the answer is one I would want anyway: a member name out of an
untrusted archive **is not yet a path**. It becomes one only after being
confined under a destination directory — `../../etc/passwd` is a legal ZIP
member name, and Zip Slip is the bug that comes from forgetting it. Keeping the
type distinct makes the confinement step something a caller has to perform
rather than something they can inherit by accident. Callers who genuinely want a
path write `Path::new(&entry.name)`, which is free: the kernel's `Path` is a
`[u8]` newtype.

**Cost:** three adaptation sites in `kernel/src/fs/archive.rs` and five in
`kernel/src/kshell.rs`, all one-liners.

**Where it bites.** `ziparchive/src/lib.rs` (`extract_entry`,
`extract_entry_limited`); `kernel/src/fs/zip.rs` (now a shim, keeping the boot
self-test per `compress.rs`'s precedent); `kernel/src/fs/archive.rs`;
`kernel/src/kshell.rs` (`zip`/`unzip`). Verified by 13 host tests in the crate
and the coarse boot battery.
