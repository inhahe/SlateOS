## 1231. The 7z reader decodes Deflate with a decoder of its own that keeps 7-Zip's rules

**Date:** 2026-10-03
**Lane:** E
**Decided by:** Claude (autonomous)

**In short:** a 7z archive can hold files compressed with Deflate (the
method of ZIP and gzip) or its larger-window cousin Deflate64. The
workspace's `deflate` crate follows zlib, and zlib and 7-Zip disagree about
damaged and unusual streams, so files 7-Zip would extract were being
refused, and the reverse. 7-Zip's own Deflate decoder cannot be ported
the way its LZMA decoder was, because its licence is the LGPL, not the
public domain. So `sevenz` has a small Deflate decoder of its own, written
from the format's specification, that follows 7-Zip's rules where they
matter. The rules were found by reading 7-Zip's source; none of its code
is used. BZip2 gets the same treatment, as a second set of rules inside
the `bzip2` crate.

### What was decided

- `sevenz/src/inflate.rs`: Deflate and Deflate64, from RFC 1951 and the
  Deflate64 extensions (64 KiB window, 16 extra bits on length code 285,
  distance codes 30 and 31). It keeps 7-Zip 26.00's rules for a Deflate
  coder in a 7z folder:
  - input past the end reads as 1-bits, and decoding goes on until one of
    7-Zip's over-read checks sees it -- at block boundaries, header parts,
    megabyte boundaries and the end, and before each symbol only for more
    than four bytes drawn ahead of the end;
  - a code table is refused only if over-full (Kraft sum above 1);
    incomplete ones are kept and an unassigned bit pattern fails when met;
  - literal/length codes 286 and 287 are matches of length 3;
  - once the output is full the block must end (a match running past is
    cut) and later blocks must be empty up to the final one; the output may
    come up short without an error of the coder's own;
  - the coder reports how much input it took, for 7-Zip's "data after the
    end" check.
- `bzip2::decompress_as_7zip`: libbzip2's decoder with 7-Zip's three
  differences (an over-full coding table refused; a block ending in four
  equal bytes with no count accepted; one stream read).
- Both coders refuse properties, as 7-Zip 23+ refuses a coder given some it
  cannot take.

### The alternatives

| | For | Against |
|---|---|---|
| The `deflate` crate, as before | One Deflate decoder in the workspace; long-tested | 44 of 16,581 damaged archives got a verdict other than 7-Zip's; refuses incomplete codes 7-Zip reads; no Deflate64; the crate is not lane E's, so a 7-Zip mode in it is another lane's change |
| Port 7-Zip's `DeflateDecoder.cpp` | Exact by construction | LGPL code in a crate that is otherwise a public-domain port; the image's licence notices and the crate's terms change with it |
| **A decoder of its own, keeping 7-Zip's rules** (chosen) | Verdict parity: 0 of 18,198 mutants differ (Deflate64 among them), and 12 crafted archives (data after the end, two BZip2 streams, an incomplete code, codes 286/287, a coder given properties) get 7-Zip's verdicts; licence-clean; Deflate64 for free | A second Deflate decoder in the workspace -- a second parser of untrusted input to keep sound; its fidelity rests on the rules being complete, which only the corpus guards |

The second decoder's cost is real but bounded: it is about 500 lines,
written to the same no-index, no-panic rules as the rest of the crate, and
used only for 7z folders.

### What would reopen it

A 7z archive in the wild whose verdict here differs from 7-Zip's: the rule
it exposes goes into `inflate.rs` and the generator, as a crafted case if no
mutant reaches it.
