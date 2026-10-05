## A crafted 16-byte MP4 panics the image viewer (lane C)

**Status: FIXED 2026-08-16** (lane C). Verified by reverting the fix and
watching the test panic at the exact line named.

`parse_mp4_boxes` in `apps/imageviewer/src/video.rs` walks the top-level box
list. An MP4 box header is a 4-byte size followed by a 4-byte type, and a size
of **1** means "the real size is the 64-bit number that follows". That 64-bit
number is read straight out of the file and is otherwise unconstrained.

The walk advanced with `pos.checked_add(box_size as usize)`, accepting the
result if it was greater than `pos`. With `box_size == u64::MAX` and `pos == 0`,
that is `Some(usize::MAX)` — greater than `pos`, so accepted. The loop condition
was then `while pos + 8 <= data.len()`, which is `usize::MAX + 8`:

```
thread '...' panicked at apps\imageviewer\src\video.rs:294:11:
attempt to add with overflow
```

In a release build the add wraps to `7`, the condition passes, and the very next
statement is `data[pos]` at `pos == usize::MAX` — so it panics either way, just
with a less informative message.

Sixteen bytes of file are enough: `00 00 00 01 "ftyp" FF*8`. Nothing about it is
implausible for a hostile or merely corrupt file, and the viewer parses before
it displays, so opening the file is the whole exploit path.

The fix is not a bigger bound check. `checked_add` was already there and was not
the problem — the problem was that the *next* thing to do with the accepted
position was arithmetic on it. The walk now uses `byteread::Reader::seek`, which
refuses a position past the end of the buffer, so an absurd size ends the walk
instead of parking `pos` at a value that cannot be added to. The `usize::try_from`
on `box_size` makes the 64-bit-to-pointer-width narrowing explicit rather than a
silent `as`.

Regression tests: `a_box_that_claims_the_whole_address_space_ends_the_walk` and
`a_box_whose_size_runs_past_the_end_stops_the_walk_rather_than_reading_on` in
`apps/imageviewer/src/video.rs`. The first fails on the old code with the panic
above; the second passes on both and is there to pin the ordinary
size-past-the-end case, which was already correct.
