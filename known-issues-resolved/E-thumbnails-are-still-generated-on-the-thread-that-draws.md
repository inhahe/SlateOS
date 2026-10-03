### [E] Thumbnails are still generated on the thread that draws -- 2026-09-26
**Status:** FIXED (lane E, 2026-09-26) -- both grids make their thumbnails on
`offloop::Queue` (new the same day): the request is every card the view
shows that has no thumbnail, replacing whatever of the last set is not yet
started; each thumbnail is filed as it arrives, in `App::on_wake`, which asks
for a frame. The window's generator, with its disk cache, moves to the
worker when the waker arrives; without one (tests, a worker that will not
start) the old per-frame budget still does the work. Tests
`thumbnails_are_made_off_the_window` (explorer) and
`the_grids_thumbnails_are_made_off_the_window` (photomanager); both mutation
tables, `apps/explorer/mutate.py` (new) and `apps/photomanager/mutate.py`,
catch every row. Was: `apps/explorer/src/main.rs` (`pump_thumbnails` ->
`self.thumb_gen.process_batch(batch)`), `apps/photomanager/src/main.rs`
(`sync_thumbnails` -> `process_batch(Self::THUMB_BATCH)`).

**In short:** opening a folder of photographs in the file manager, or the
photo manager's grid, makes each thumbnail on the thread that draws the
window, a few per frame. `imagecodec::decode_scaled` makes one cheap for a
small picture, but a camera's JPEG still costs about a third of a second at
128 pixels (lane F's figure, 4000x5333, release), so every frame that makes
two or three of them is a frame the window cannot answer in. The selected
photograph's own decode moved off that thread on 2026-09-26 (`apps/offloop`,
`TD-C-DECODING-A-PHOTOGRAPH-BLOCKS-THE-FRAME-THAT-ASKED-FOR-IT`); the grids'
thumbnails did not.

**The proper fix** is the same worker with a different rule. A viewer wants
only the newest request (`offloop::Latest`); a grid wants *every* card it can
see, and each result as soon as it exists. So: a second `offloop` type whose
request is the whole set of visible cards -- replacing the set not yet
started, since cards scrolled away are no longer wanted -- and whose results
are handed back one by one as they are made, each waking the loop. The
thumbnail cache (`gui/thumbs`, lane C's) stays where it is; only the
`process_batch` call moves to the worker, with the generator's input and
output crossing by channel.
