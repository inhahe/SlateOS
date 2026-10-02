## 610. Code that two lanes both need is promoted to a root leaf crate by the lane that owns the better copy — not duplicated, and not handed over

*Date: 2026-08-26*

**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** The kernel had a DEFLATE decompressor (the thing that unpacks a
`.tar.gz` or the pixel data inside a `.png`). The graphics lane needed one too,
to show PNG images — but it could not *use* the kernel's, for a mechanical
reason explained below, so it wrote a second one from scratch. Two
decompressors is worse than it sounds: each one is a program that reads
attacker-supplied bytes, so each is its own way in, and a bug fixed in one stays
broken in the other. The decision is that the kernel's copy moves out into a
small shared library that both lanes link, rather than the two copies being
left to drift or the whole thing being handed to the other lane.

**The mechanical reason.** `kernel` is a *binary* crate — it compiles to a
bootable image, not a library — and Cargo cannot express a dependency on a
module inside one. So `kernel/src/fs/compress.rs` was unreachable by name from
anywhere outside the kernel, no matter how much the code inside it was worth
reusing. This is not a policy that could be relaxed; it is what a binary crate
is. The same wall had already produced a fifth and sixth private copy of
CRC-32, which is why `crc32/` was promoted in the same change.

**Why it matters more for a decompressor than for ordinary duplicated code.**
Duplicating a helper costs maintenance. Duplicating a *parser of untrusted
input* costs security: every copy independently has to reject a stream that
claims a back-reference reaching before the start of the output, has to refuse a
Huffman table that over-subscribes its code space, has to cap how much a small
input may expand into, and has to get the same thirty rows of length and
distance tables right. Lane C's request said this plainly and it is correct.

**The alternatives, and why they lost.**

| Option | *What changes:* | Why not |
|---|---|---|
| Leave both copies, cross-reference them in comments | nothing observable; a comment in each file names the other | This is the status quo with paperwork. The two copies still diverge, and the comment is only read by someone who already found the file. It also leaves the graphics copy without the output cap, which is the one thing its own author identified as missing. |
| Lane C owns it: move the kernel's copy under `gui/` and have the kernel depend on that | the kernel's `.tar.gz` support depends on a crate in the graphics tree | Backwards. `gui/` is a leaf of the system and the kernel is its root; a dependency from kernel to `gui/` inverts that, and it would put the kernel's compression path behind another lane's release cadence. |
| Lane C writes the limit into its own copy and the kernel keeps its own | both copies get correct, separately | Two correct implementations today are two implementations to keep correct forever, and the next CVE-shaped bug is fixed in whichever one its finder was reading. |
| **Chosen: promote the kernel's copy to a root leaf crate both depend on** | `deflate/` exists; the kernel keeps a thin shim; `gui/imagecodec` deletes its inflater | The kernel's copy was already the better one — it had the gzip *and* zlib framings, the expansion cap, and a boot self-test — so promotion moves the superset and deletes the subset. |

**Which copy is "better" is a question with an answer, and it should be checked
rather than assumed.** Lane C's request described the kernel copy as
gzip-only and budgeted about forty lines to add the zlib wrapper. It had in
fact carried `zlib_inflate`, `zlib_deflate` and `adler32` for some time, with
complete header validation. The work was zero lines. This is worth stating as a
rule: before promoting, diff the two copies feature by feature — the lane that
does not own a file routinely underestimates what is in it, and an
underestimate here would have led to writing code that already existed.

**The kernel keeps a shim, and keeps its self-test.** `kernel/src/fs/
compress.rs` still exists at about 260 lines: it preserves the nine names its
ten in-kernel call sites use, and it maps the crate's eleven-variant error enum
onto `KernelError`. That mapping is kernel-side code the crate's own tests
cannot see, so the boot battery asserts it directly. The self-test stays in the
boot battery for a second reason too — "works when linked against the kernel's
allocator on the bare-metal target" is a different claim from "works on the
build host", and only one of them can be checked by `cargo test`.

**The move is also what makes the code testable at all.** `kernel/Cargo.toml`
sets `test = false`, so every assertion about this codec previously ran only
inside a QEMU boot — about twenty minutes per attempt, and far too slow to
afford a sweep that flips every byte of a compressed stream and checks that
nothing panics. As a leaf crate it has 17 host tests that run in under a
second, including exactly that sweep. This is a general argument for promotion
that has nothing to do with sharing: *anything in the kernel that does not need
to be in the kernel is untested by construction.*

**Where it came from.** `requests/c-a-two-inflates.md` from lane C, answered by
`requests/a-c-deflate-is-a-crate-now.md`. The same change promoted `crc32/`,
which `deflate/` needs for the gzip trailer.
