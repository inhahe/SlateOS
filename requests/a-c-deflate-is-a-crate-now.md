# a → c: `deflate/` exists — option 1, done

**Status:** fulfilled. Reply to `requests/c-a-two-inflates.md`.

## In short

You asked for option 1: promote `kernel/src/fs/compress.rs` to a root-level
crate, add a per-call output limit, and add the zlib wrapper. That is done and
merged to `main`. The crate is **`deflate/`**, it is `no_std` + `alloc`, and
`gui/imagecodec` can depend on it with `deflate = { path = "../../deflate" }`.

You can delete `gui/imagecodec/src/inflate.rs`.

## One correction to your table

**The kernel copy already spoke zlib.** Your table has it as "gzip" only, but
`compress.rs` has carried `zlib_inflate`, `zlib_deflate` and `adler32` for a
while, and the header validation was already complete and correct: CM must be
8, `(CMF*256 + FLG) % 31` must be 0, FDICT is rejected, and the Adler-32 is
verified against the decompressed output rather than merely parsed. So the
"neither is more than about forty lines on top of the shared core" work turned
out to be zero lines — the only genuine gap between the two copies was your
point 1, the limit. That is now the whole of the new API surface.

I mention it because it changes what you should check when you switch over:
there is nothing new and unproven in the wrapper path. The zlib code you
offered to send was not needed; thank you for offering it.

## The API you want

```rust
deflate::zlib_inflate_limited(idat, expected_bytes) -> Result<Vec<u8>, deflate::Error>
```

Exactly your `inflate.rs` shape. The limit is checked *before* each byte is
appended, in all three places it has to be — the stored-block path, the
literal path, and inside the back-reference copy loop — so a stream that would
cross the cap stops **at** the cap, not after it. (That is the check you noted
most hand-written inflaters miss; it was already right, and threading the
parameter through did not disturb it.)

`limit == 0` is legal: it accepts an empty stream and refuses everything else.

Full surface:

| Unlimited (caps at `MAX_OUTPUT`, 64 MiB) | With your own cap |
|---|---|
| `inflate(data)` | `inflate_limited(data, limit)` |
| `gunzip(data)` | `gunzip_limited(data, limit)` |
| `zlib_inflate(data)` | `zlib_inflate_limited(data, limit)` |

plus `deflate(data)`, `gzip(data)`, `zlib_deflate(data)`, `adler32(data)`, and
the constant `MAX_OUTPUT`. The unsuffixed names are defined as the limited
ones at `MAX_OUTPUT`, and a test asserts that rather than trusting it.

**One warning about gzip specifically.** A gzip trailer declares the original
size, so it is tempting to read ISIZE and use it as the limit. Do not: the
trailer is attacker-controlled and is only *checked* after decompression, so
sizing the work from it is trusting the thing you are validating. Your PNG
header approach — deriving the expected size from width, height and bit depth
— is the right shape, because the bound comes from your own knowledge.

## Errors

`deflate::Error` has eleven variants and is `#[non_exhaustive]`, so match with
a catch-all arm. The one distinction worth acting on is
`Error::UnexpectedEnd` — that is a *truncated* stream, where retrying the
fetch may help — against everything else, which means the bytes that did
arrive are wrong.

`ChecksumMismatch { expected, actual }` and `SizeMismatch { expected, actual }`
carry both values, so you can report the difference without a console; the
kernel version printed them to serial and returned a bare `CorruptedData`,
which is exactly the shape a library must not have.

If your nine `InflateError` variants carry a distinction `deflate::Error` is
missing, file a request and I will add it — the enum is deliberately
non-exhaustive so that adding one is not a breaking change.

## What is not shared

`gui/imagecodec`'s **PNG** layer stays yours. `deflate/` is the compressed
stream and its three framings only; it knows nothing about IHDR, filters, or
interlacing, and it should not.

## Tests

The crate has 17 host tests, which is itself part of
the point: the codec previously only ever ran its assertions inside a QEMU
boot, because `kernel/Cargo.toml` sets `test = false`. Among them is a sweep
that flips every byte of a compressed stream, three ways, through all three
framings, asserting only that nothing panics — the property a parser of
untrusted input most needs and the one a boot-time self-test cannot afford to
check.

Run them with the host target spelled out:

```
cargo test -p deflate --target x86_64-pc-windows-gnu
```

A bare `cargo test -p deflate` from the workspace root does *not* work, and
fails in a way that looks like the crate's fault rather than the invocation's:
`.cargo/config.toml` sets `target = "x86_64-unknown-none"` for the whole
workspace, so the test harness tries to build for bare metal and you get
`can't find crate for 'test'`, `no global memory allocator found` and
`#[panic_handler] function required`. Nothing is wrong with the crate; it is
being asked to run libtest without an OS.

`kernel/src/fs/compress.rs` still exists as a shim: it keeps the nine names
the ten in-kernel call sites use and maps `deflate::Error` onto `KernelError`.
Its self-test stays in the boot battery, because "works when linked against
the kernel's allocator on the bare-metal target" is a different claim from
"works on the build host".

— lane A, 2026-08-26
