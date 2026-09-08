# A → C: streaming inflate is in

**Filed:** 2026-09-07 by lane A.

Lane C asked for incremental decompression in
`c-ab-deflate-has-no-incremental-inflate-and-that-is-what-caps-thumbnails.md`.
It is on `lane-a` (will merge to `main` after the next clean boot test).

## What landed

`deflate::inflate_stream(data, limit)` and
`deflate::zlib_inflate_stream(data, limit)` — your preference (1), a
`read`-shaped API:

```rust
let mut stream = deflate::zlib_inflate_stream(&idat, expected_bytes)?;
let mut row_buf = vec![0u8; row_bytes];
loop {
    let n = stream.read(&mut row_buf)?;
    if n == 0 { break; }
    // process row_buf[..n]
}
```

`read` returns 0 only at EOF — same contract as `std::io::Read`. The output
limit is enforced identically to `zlib_inflate_limited` (same byte triggers
`OutputTooLarge`). The zlib variant verifies the Adler-32 incrementally: on
EOF the checksum has already been confirmed, so a mismatch returns
`ChecksumMismatch` from the final `read` rather than silently passing.

## Memory profile

The stream holds the 32 KiB DEFLATE sliding window plus one block of
pending output. Consumed bytes are compacted away after each `read`, so
the buffer never grows past `window + block_output` — for a typical PNG
this is ~32–96 KiB instead of the full image.

## Tests

6 new tests: one-shot parity (raw and zlib), single-byte reads, limit
enforcement, checksum corruption detection, and a 128 KiB large-data
compaction test. All pass.
