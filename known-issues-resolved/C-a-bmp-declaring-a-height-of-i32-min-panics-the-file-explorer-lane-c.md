## A BMP declaring a height of `i32::MIN` panics the file explorer (lane C)

**Status: FIXED 2026-08-16** (lane C). Verified by reverting the fix and
watching the test panic.

`parse_bmp_dimensions` in `apps/explorer/src/thumbs.rs` read the BMP height as
a `u32`, cast it to `i32` to interpret the sign — a negative height means a
top-down bitmap — and then took its magnitude with `.abs()`:

```rust
let height = (read_le_u32(data, 22)? as i32).abs();
```

`i32::MIN.abs()` panics: `+2147483648` is not an `i32`. Four bytes of
`00 00 00 80` at offset 22 of any file the explorer decides is a BMP is enough.

```
panicked at library\core\src\num\mod.rs:394:5:
attempt to negate with overflow
```

The check that would have rejected the value — `height == 0`, and the caller's
own size limits — runs *after* the `abs()`, so it never gets the chance. This is
the same shape as the sweep's other findings: the guard exists, it is just
downstream of the operation it was meant to guard.

`unsigned_abs()` returns the magnitude as the `u32` it fits in, and the
existing dimension limits then decline the thumbnail. Found while replacing
explorer's five hand-written binary readers with `byteread` — reading the field
as `i32_le_at` rather than `u32_le_at` + a cast is what made the `.abs()`
visible as a decision rather than a formality.

Regression test:
`the_one_height_that_has_no_positive_counterpart_is_still_just_declined` in
`apps/explorer/src/thumbs.rs`.
