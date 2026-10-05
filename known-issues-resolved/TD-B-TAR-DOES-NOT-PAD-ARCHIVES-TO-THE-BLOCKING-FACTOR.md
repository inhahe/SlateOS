## TD-B-TAR-DOES-NOT-PAD-ARCHIVES-TO-THE-BLOCKING-FACTOR — **Status: FIXED** 2026-08-30

**Fixed** the same day it was filed. `do_create` now writes through a
`RecordWriter` that buffers whole records and pads the last one, and both of
GNU's knobs for the record size exist: `-b` / `--blocking-factor=BLOCKS` in
512-byte units and `--record-size=NUMBER` in bytes, which are one setting with
two spellings — last one on the line wins — complete with GNU's tape-suffix
table (`b` is 512 while `B` is 1024, `P` is a suffix and `p` is not, there is no
`E`/`Z`/`Y`) and `strtoul`'s grammar rather than a trim.

Two things beyond the padding came out of the measurement and are pinned by
tests rather than left to be rediscovered. The **buffering rule** is observable:
a record reaches the stream when the *next* write needs the room, not when the
byte that fills it arrives — which is why `tar -b 1 -cf o.tar -C tree a.txt -C
nosuchdir` leaves 512 of the 1024 bytes it was handed, and why
`std::io::BufWriter` (which bypasses its buffer for a write at least as large as
its capacity) would have been wrong. And **zero is refused twice, differently**:
`-b 0` is the parser's refusal and beats even `--help`, while `--record-size=0`
gets through the parser and is refused by the run — after the no-mode check and
after the refusal to archive nothing, but before `-f` is opened.

`scripts/tar-diff.sh` no longer normalises GNU with `--blocking-factor=1`, so
all ~30 create cases now compare the archive's length as well as its contents,
and § 1 gained twenty-odd cases for the knob itself; `archive_delta` grew a
sentence for a length difference, which it previously could not describe.
`design-decisions.md` § 712 records the three choices. Thirteen unit tests in
`tar.rs` cover the parsers and the writer; each was mutation-checked.

The entry as filed follows.

**Where:** `userspace/coreutils/src/bin/tar.rs` — `do_create`, and the two
end-of-archive zero blocks it writes. (Also true of `userspace/tar`, which is
where this entry originally pointed; that crate is dead, and the gap being real
in the shipped one too is why the entry survived the correction rather than
being withdrawn like the one above.)

**Independently confirmed by the harness**, which is the strongest evidence
here because it is not this entry's own measurement: `scripts/tar-diff.sh`
normalises GNU with `--blocking-factor=1` precisely so the padding stops
swamping every create case, and its header says why. Take that flag away and
every `create_case` fails on trailing zeros alone.

**In short:** GNU writes archives in units of 20 × 512 = 10240 bytes, padding
the last unit with zeros. We stop after the two zero blocks. The same
one-file archive is 10240 bytes from GNU and 3072 from us. Both are valid tar
and both read back correctly — in either implementation — so this is a
fidelity gap, not a bug.

**How it surfaced.** Not by looking for it. A failed `-C` mid-create leaves
an archive behind in both implementations (proving `-f` resolved before the
chdir), but GNU's is 0 bytes and ours is 1024 — because GNU had the first
member sitting in its 10240-byte buffer, unflushed, when it exited. So the
buffering is observable on the error path as well as in the file size.

**Where it would actually bite:** writing to a tape device or a pipe expecting
fixed-size records, and any test that byte-compares our archive against a
GNU-produced one.

**The fix.** Buffer output in 10240-byte records and pad the final one, then
add `-b, --blocking-factor=N` (GNU's knob for it, in 512-byte units) so the
constant is not baked in. Reading is unaffected: we already scan block by
block and stop at two consecutive zero blocks, which is why a GNU archive's
trailing padding has never bothered us.
