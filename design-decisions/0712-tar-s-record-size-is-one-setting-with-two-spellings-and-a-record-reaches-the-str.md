## 712. `tar`'s record size is one setting with two spellings, and a record reaches the stream only when the *next* write needs the room

**Date:** 2026-08-30
**Decided by:** Claude (autonomous), lane B
**Lane:** B

**In short:** tar was born as a tape program, and a tape is written in
fixed-size chunks called *records* — 10240 bytes by default. GNU pads the last
record of every archive with zeros so it comes out a whole number of records;
ours used to stop as soon as it had written the two all-zero blocks that mark
the end. Both files are valid and either program reads either one, but they are
different lengths and therefore have different checksums, so nothing that
compares an archive byte-for-byte against GNU's could ever agree. Ours now pads.
This entry records three choices that came with the padding: keeping the two
ways of setting the record size as one value rather than two, implementing the
byte-counting spelling completely rather than in part, and the exact moment a
finished record is handed to the stream.

**Date measured against:** GNU tar 1.35, x86_64 Debian, `LC_ALL=C.UTF-8`.

### One setting, not two

`-b` / `--blocking-factor=BLOCKS` counts 512-byte blocks; `--record-size=NUMBER`
counts bytes. It is tempting to store them as two fields and combine them, and
that is wrong — measurably so:

| command line | GNU's record |
|---|---|
| `-b 3 --record-size=1024` | 1024 bytes |
| `--record-size=1024 -b 3` | 1536 bytes |

The last one on the line wins, whichever spelling it used, which only one
`u64` field can express. Two fields would have to invent a precedence rule, and
every rule you could invent gets one of those two rows wrong.

*What it costs:* nothing, and it is worth recording only because the two-field
version is the one you write if you do not measure first.

### `--record-size` is implemented, suffixes and all

The alternative was to accept `-b` and leave `--record-size` in the set of GNU
options this tar does not have — the honest-refusal policy of §703. It was
rejected because the two options are the *same* setting, so "supported, but only
if you spell it the short way" is a worse story than either supporting it or
not. And a half version is the specific failure §703 exists to prevent:
`--record-size=1K` is valid GNU, and a version that parsed only bare digits
would refuse a command line that works everywhere else.

Implementing it means implementing its suffix table, which is a tape utility's
and not `du`'s. The letters are exactly `bBcGgKkMmPTtw`, and two of them are
traps: `b` is 512 while `B` is 1024 (so `--record-size=3b` is 1536 and
`--record-size=3B` is 3072), and uppercase `P` is a suffix while lowercase `p`
is not. There is no `E`, `Z` or `Y`. A two-letter suffix — `1kB`, which `du`
takes — is invalid outright. Values are parsed with `strtoul`'s grammar rather
than a trim: leading whitespace (C's `isspace`, so a vertical tab counts, which
Rust's `is_ascii_whitespace` does not) and a leading `+` are skipped, trailing
whitespace is not, and the base is always ten, so `0x10` is refused and `0777`
is seven hundred and seventy-seven.

*What it costs:* about sixty lines and a suffix table that has to be got right
from measurement, since none of it is guessable.

### The buffering rule: spill on the write that needs the room

The obvious implementation is `std::io::BufWriter` with the record as its
capacity. It produces the wrong bytes, and there is exactly one command line in
existence that shows it — one that dies with a partly-filled record and so never
pads:

```
tar -b 1 -cf o.tar -C tree a.txt -C nosuchdir   # GNU leaves 512 bytes
tar      -cf o.tar -C tree a.txt -C nosuchdir   # GNU leaves 0 bytes
```

`a.txt` hands 1024 bytes (a header and a data block) to a 512-byte record. GNU
leaves *half* of them in the file. That is only consistent with one rule: a full
record is written when the *next* write needs the room, not when the byte that
fills it arrives. Flushing on the filling byte would leave 1024 in the first row;
`BufWriter`, which bypasses its buffer entirely for a write at least as large as
its capacity, would leave 1024 in both rows.

*Alternatives:* (a) `BufWriter`, (b) flush the moment the buffer is full, (c)
spill only when the buffer is already full and a further write has arrived.

(c) is what shipped, as a ~30-line `RecordWriter`. It is the simplest of the
three to state and the only one that reproduces the artefact. Deliberately, it
has **no** `Drop` — the tail of an aborted archive is *meant* to be lost, so
finishing is an explicit `finish()` call that the error paths never make. Adding
a `Drop` that flushed would silently re-break the case above.

The buffer also grows lazily rather than being allocated at `record` bytes up
front. `-b` accepts up to 2147483647 (INT_MAX, measured at both ends), which is a
1 TB record; reserving it would make `tar -b 2147483647 -cf o.tar tiny.txt` cost
what the *record* costs instead of what the archive costs. The bytes written are
identical either way.

(Learned the hard way, and the cleanup is the part worth passing on: an early
measurement run asked GNU for that record on a live WSL instance and took the
instance down with it — `Wsl/Service/E_UNEXPECTED`, recovered with
`wsl --shutdown`. What was *not* obvious is that the crash also wrote a **540 GB
core dump** to `%LOCALAPPDATA%\Temp\wsl-crashes\`, which filled the C: drive to
zero and sat there silently until something unrelated failed with `No space left
on device` an hour later. Check that directory after any WSL crash. Every
measurement in this section was afterwards taken by probing with
`tar <option> --help`, which runs the same validation and exits before
allocating a record at all.)

*What it costs:* a hand-written writer where a standard one would have compiled,
and a comment on it explaining why it must not be "tidied" back into a
`BufWriter`.

### What this changed in the harness

`scripts/tar-diff.sh` normalised GNU with `--blocking-factor=1` precisely so
that the missing padding would not swamp every create case. That flag is gone,
so all ~30 create cases now compare the archive's *length* as well as its
contents. `archive_delta` grew a second sentence for it: `cmp` names a differing
byte on stdout but reports a file that merely ran out only on stderr, so the
length case used to fall through the offset arithmetic and print nonsense.

**How to reverse.** `RecordWriter` and the two parsers are contiguous in
`userspace/coreutils/src/bin/tar.rs`; dropping them means passing the sink to
`Creator` directly again and putting `--blocking-factor=1` back in `GNUFMT`,
which re-breaks the twenty-odd record-size cases in `scripts/tar-diff.sh` § 1
and the two in § 8 that this entry exists to explain.
