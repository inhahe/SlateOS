## 1229. The archive codecs are ported, held to their references' own bytes, and take their input whole

**Date:** 2026-10-03
**Lane:** E
**Decided by:** Claude (autonomous) -- within lane A's answer to
`requests/e-a-bzip2-xz-and-7z-are-trapped-in-the-kernel-binary.md` (lane E
writes the crates, lane A switches the kernel to them)

**In short:** the `bzip2` and `xz` crates are line-by-line ports of
libbzip2 1.0.8 and liblzma 5.2.5. Their tests do not check that what they
write decodes, which any encoder passes; they check that it is *the same
bytes* the reference writes. The decoders give the same verdict as the
reference on every damaged file tried. Both take their input in one piece
and return their output in one piece, with no streaming interface.

| Call | Chosen | The other way, and why not |
|---|---|---|
| Where the code comes from | a port of the reference implementation, function by function | written from the format's description, as the kernel's copies were: those wrote streams no decoder reads (`requests/e-a-the-xz-crate-is-ready-and-xz-compress-loses-files.md`), and nothing short of the reference's own output says where a hand-written codec is wrong |
| What the tests hold the encoder to | the reference's bytes: 136 settings and inputs for xz, every preset of both, the reference's own files re-encoded | that the output decodes: true of almost any encoder, including a bad one -- and an encoder that matches byte for byte has every one of the reference's choices right, its price tables, chunk limits and match finder included |
| The interface | the whole input in, the whole output back (`xz::compress(&[u8]) -> Vec<u8>`) | streaming (`Read`/`Write` adapters), as liblzma has: needed only by a caller who cannot hold the data, and neither caller is one -- the archive manager holds an archive in memory already (under its 512 MiB budget), and `fcompress` compresses one file at a time. Whole-input makes the port simpler and its equivalence argument short: liblzma only codes a position when more input lies past it than it ever looks at, so where its buffers end changes nothing |
| Memory past liblzma's | none extra; the encoder's son array is cut to the input when the input is shorter than the dictionary | liblzma's full-size arrays: committed memory -- 512 MiB at `-9` for a one-kilobyte file -- that no position would ever read |
| Speed in tests | `xz` built at `opt-level = 3` in dev builds (`Cargo.toml`, as rav1d) | unoptimised: the corpus took minutes; overflow checks stay on either way |

A streaming interface can be added over the same code if a caller ever needs
one: the state machines were flattened, not lost, and liblzma's
`read_ahead`/`read_limit` logic is what such an adapter would restore.
