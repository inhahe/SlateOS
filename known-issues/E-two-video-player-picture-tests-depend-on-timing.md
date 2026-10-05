# Two of the video player's picture tests depend on timing

**Status:** OPEN (lane E, found 2026-10-04)

**In short:** two rows of `apps/videoplayer/mutate.py`'s pictures table break
the program, and the program is caught, but not by the test the row names.
Under a loaded machine the named test passes against the broken program,
because what it checks depends on how far the decoding thread got before the
test looked.

| Row | Named test (passes when broken) | Caught instead by |
|---|---|---|
| a frame from before a seek is shown after it | `a_seek_shows_the_picture_at_its_time_and_nothing_from_before_it` | `dragging_the_seek_bar_shows_the_key_frames_on_the_way`, `an_adjustment_moved_with_the_keys_changes_the_picture` |
| the first of two seeks is the one taken | `two_seeks_in_a_row_land_on_the_second` | `an_adjustment_drags_and_reset_all_puts_the_picture_back` |

**Where:** `apps/videoplayer/src/pictures.rs`, tests module. Both tests rely on
a `sleep` to let the thread queue stale frames (or take the first seek)
before the step that would expose the fault.

**Proper fix:** give each test a deterministic hold on the thread. One way is a
test `Source` whose `next`/`seek` block on a channel the test releases, so
that "a stale frame is queued" and "the first seek was taken" are established
facts rather than likely ones. Then re-run the two rows until they are caught
as named. Until then, the rows stay `[??]` in a full sweep.

**Also open:** the row "the picture a seek asks for is passed over like any other" (added 2026-10-04 with the fix for that race) survives: `the_picture_a_seek_asks_for_is_not_passed_over_for_a_clock_left_behind` passes against the mutant even after a first picture is handed over before the seek. Find why the test does not reach the passing-over loop, then make it fail as named.
