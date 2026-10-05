## Two reachable panics in `apps/paint`, found by the lint sweep (lane C)

**Status: FIXED 2026-08-16** (lane C). Both are the same defect in two
functions, and both tests were verified by reverting the fix and watching them
panic at the exact line named.

`PaintApp::layers` and `PaintApp::active_layer` are both public fields, and
nothing in the type ties them together. "The index is in range" is a convention
maintained by ten separate assignment sites — and `delete_layer` reached
`Vec::remove(self.active_layer)` and `merge_layer_down` reached
`self.layers[self.active_layer]` with whatever the field happened to say. The
only guards were `layers.len() <= 1` and `active_layer == 0`, neither of which
is a bound on the index. Setting the field directly, or restoring a
`HistorySnapshot` of one's own construction — equally public — takes the editor
down. No malformed input needed.

Both now check the bound where they use it and return `false` otherwise, which
is the answer both functions already had for "cannot do this".

`merge_layer_down` was additionally reordered. It used to remove the upper layer
and *then* look up the lower one, so any failure between the two would have lost
a layer without merging it into anything. It now borrows both at once via
`split_at_mut_checked` and does the blend first, removing only after the work
succeeded — the failure mode is structurally absent rather than merely
improbable. Pinned by
`merging_down_leaves_the_document_whole_when_it_declines`.

A third finding was investigated and is **not** a live bug, recorded so the next
reader does not re-derive it: `decode_bmp`'s `offset + w * 4 * h` cannot in fact
wrap on this target, because three 32-bit header fields cannot exceed a 64-bit
`usize`. That is an accident of the word size rather than something the
expression states, and it stops holding on a 32-bit target, where a wrapped
`needed` is small and passes the length check that exists to reject it. It is
written with `checked_*` now for that reason, not because a crafted BMP could
reach it here.
