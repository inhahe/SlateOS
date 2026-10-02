## A count field could allocate memory the archive never contained (lane A, 2026-08-23)

**Status: FIXED** -- `kernel/src/fs/sevenz.rs`, `kernel/src/fs/zstd.rs`.

### In short

Two archive parsers took a number out of a file and used it to reserve memory
before checking whether the file was big enough to contain that much. A 9-byte
7z header could claim four billion files; a 14-byte zstd header could claim it
decompressed to 256 MiB. In a normal program the allocation just fails and you
get an error. In a kernel it does not fail -- it halts the machine. Both
parsers are reachable from a shell command on a file the user supplies, so
"the user opened a bad archive" and "the machine stopped" were the same event.

### Why an unchecked count is worse than an unchecked loop

The usual unbounded-growth bug is a loop that appends without a ceiling; it
takes an attacker some traffic to exploit and it degrades before it dies. A
count field is worse in every dimension, because of *where the number lands*:

    Vec::with_capacity(n)      // n from the file
    vec![x; n]
    resize_with(n, ..)

None of these can report failure. There is no `Result` to propagate -- on
failure the allocator calls `handle_alloc_error`, and in a kernel that is a
halt. So the check cannot be "handle the allocation error"; it has to happen
*before* the allocation, or it does not happen at all. One malformed header,
one halt, no traffic required.

### The two instances

| Parser | The claim | What it cost |
|---|---|---|
| `sevenz.rs` | every count was a raw `read_vli()` -- up to `u64::MAX` -- with **no guard of any kind**; `MAX_OUTPUT` was the module's only constant | `num_files` fed `resize_with` directly, so a 9-byte header sized a `Vec<FileInfo>` at four billion |
| `zstd.rs` | `content_size` is an 8-byte frame-header field, clamped **only** to `MAX_OUTPUT_SIZE` | a 14-byte frame reserved 256 MiB before one byte of compressed data was read |

Reachability was checked, not assumed. `un7z`: `archive.rs:391`,
`archive.rs:478`, `kshell.rs:104454`. `unzstd`: `fcompress.rs:529`,
`kshell.rs:102859`, `kshell.rs:104108`, `kshell.rs:105962`. Several of those
are shell commands taking a path from the user.

An earlier pass had wrongly cleared zstd as unreachable, on a grep for
`zstd::decompress`. The public entry point is `unzstd`, and it has four
callers. A reachability check that greps for a guessed name and concludes
"no callers" is not a check; the export list is the thing to read.

### The fixes, and why they use different bounds

They look like the same bug and they are, but the right bound differs because
the two numbers mean different things.

**7z counts bound themselves against the input.** Every counted item costs at
least one byte somewhere in the archive, so a count larger than the bytes
remaining is a lie *whatever it is counting* -- no invented constant needed.
That is generous (real entries cost far more than a byte each) and it is exact
in the direction that matters: it can never reject an archive that could
actually have been parsed. `read_count()` applies it at the four count sites.
Two further places needed separate reasoning: the per-folder substream **sum**
(each part was bounded, but `sum()` on `usize` panics on overflow under the
kernel's overflow checks, and a non-overflowing total still sizes a
`vec![_; n]`), and `read_bool_vector` itself, whose all-defined branch
allocates while consuming no input at all -- the one place a count becomes
memory with nothing to check it against.

**A zstd content size cannot be bounded that way**, because decompression is
supposed to produce more output than input; bytes-remaining is not a valid
ceiling. But the reservation is only a *performance hint*, which is what makes
it cheap to cap: `MAX_INITIAL_RESERVE` (1 MiB) bounds what is reserved up
front, while `MAX_OUTPUT_SIZE` stays the real ceiling, enforced incrementally
as blocks actually append bytes. A frame that genuinely decodes to 256 MiB now
costs eight geometric regrowths -- negligible against the decode -- and memory
is committed in proportion to bytes *produced* rather than bytes *claimed*.

### Two things the fixes deliberately also test

- **That the bound does not reject valid input.** Rejecting the malformed case
  is only half the property; a bound that also refused parseable archives
  would be a worse bug than the one it fixed. Both self-tests pin both sides.
- **That the clamp did not replace the validation.** zstd still verifies the
  declared content size against the real output at the end of the frame. The
  test asserts an inflated claim is still `CorruptedData` and not merely
  survived -- otherwise a future reader could conclude the clamp made the
  check redundant and delete it.

`initial_output_capacity` was split out of `decompress_frame` purely so the
bound is reachable from a test. A clamp inside a function that also does the
decoding can only be exercised by allocating whatever it failed to clamp,
which is precisely the thing being prevented.

Both constants carry `const _: () = assert!(..)` invariants, so an edit that
reintroduces the hazard fails the build rather than the boot.

### Where this leaves the audit

`kernel/src/net` is closed (see the entry above), and with this pass
`kernel/src/fs` is closed for **this class** too. Every allocation in the
archive and decompression parsers whose size could come from the file was
traced to its source. Two were broken; the rest were already right:

| File | Verdict |
|---|---|
| `sevenz.rs` | **was broken** -- no count guard of any kind. Fixed. |
| `zstd.rs` | **was broken** -- `content_size` clamped only to `MAX_OUTPUT_SIZE`. Fixed. |
| `compress.rs` | correct -- `with_capacity(len.saturating_mul(2).min(MAX_OUTPUT))` |
| `bzip2.rs` | correct -- `n_selectors` <= 18002, `max_block` <= `MAX_BLOCK_SIZE`, `alpha_size` <= 258; `with_capacity(max_block.min(65536))` at line 715 is the pattern zstd should have had. The `block_len` at lines 313/326 is `mtf_decoded.len()` -- an already-materialised length, not a claim. |
| `xz.rs` | correct -- and the strongest case: it makes *no* speculative reservation, growing `output` only as bytes are produced. `lc`/`lp`/`pb` are validated (`lc<=8, lp<=4, pb<=4`) before `1usize << (lc+lp)`, which matters more than it looks: an unvalidated shift there is not a big allocation but a shift-overflow panic, i.e. a halt. |
| `zip.rs` | correct -- `with_capacity(total_entries.min(4096))` |
| `tar.rs` | correct -- capacities derive from slice lengths already in memory |
| `cpio.rs`, `ar.rs` | correct -- writer-side padding and fixed field widths only |
| `rar.rs` | no allocation of this shape at all |

The distinction the table keeps drawing is the one that matters: a size taken
from a buffer that **already exists** is a fact, and a size taken from a header
that **describes** a buffer is a claim. Both look like `usize` at the
allocation site.

The question that finds this class quickly is: **does a number read from the
file reach an allocation before anything has confirmed the file is big enough
to contain what it describes?** Grep is a poor tool for it -- none of these
produce a clippy warning, and there is no index, unwrap or arithmetic to match
on. Reading each `with_capacity` / `vec![_; n]` / `resize*` call and asking
where its argument came from is what worked, and there are few enough of them
per file to do exhaustively.

### A smaller finding, left alone deliberately

zstd's literals decoder allocates `regen_size` bytes from a 20-bit wire field
(`zstd.rs:1293`, `:1404`, `:1462`), so ~1 MiB from three bytes of input. That
is the same shape but two orders of magnitude smaller -- bounded by the field
width, survivable, and the same order as the reserve cap now applied at frame
level. Noted rather than changed, so a future reader who spots it knows it was
seen and judged, not missed.
