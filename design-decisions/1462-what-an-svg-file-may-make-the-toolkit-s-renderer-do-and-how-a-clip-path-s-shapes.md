## 1462. What an SVG file may make the toolkit's renderer do, and how a clip path's shapes join

**Date:** 2026-10-01 &middot; **Decided by:** Claude (autonomous) &middot;
**Lane:** C

**In short:** SVG files reach the toolkit's renderer from anywhere: the file
manager makes thumbnails of whatever is in a folder, icon themes are
downloaded, the image viewer opens what it is given. When the renderer
learned gradients, `<use>`, `<symbol>` and clip paths, it also had to decide
what a broken or hostile file may make it do. Before this, a file of
nothing but nested groups could crash whatever was drawing it, and a few
dozen `<use>`s could ask for a billion shapes. These are the limits chosen,
and one drawing choice about how a clip path's shapes are joined.

| Question | Chosen | The alternative | Why |
|---|---|---|---|
| How deep elements may nest | 128 levels; a deeper file is refused as malformed | libxml2's 256, which librsvg inherits | measured in a debug build (the boot test runs one), building took about 2.7 KiB of stack a level, so 256 levels needed some 770 KiB: three quarters of the 1 MiB a Windows program's main thread has. 128 needs about 350 KiB. No real drawing nests past a few dozen levels |
| How a `<use>`'s content is built | once for each element named, before the tree, from the top of the stack | built where each `<use>` stands | a part used a thousand times is kept once, and `<use>`s naming `<use>`s cannot make the build recurse deeper |
| How much `<use>` may draw | at most 256 containers deep, and 100 000 nodes through `<use>`s in one drawing; what lies past either bound is left out | no bound | ten `<use>`s of ten `<use>`s, nine deep, is a billion shapes from a hundred elements. A real drawing repeats far less than either bound |
| A `<use>` inside what it names | draws nothing | draws one copy | SVG calls it an error and browsers draw nothing; the file looks here as it does everywhere else |
| A loop through several `<use>`s (a shows b, b shows a) | the content is drawn once round, cut where the loop closes | nothing drawn for any `<use>` in the loop, as browsers do | the cut falls out of drawing, without searching the whole graph of references while building; the two differ only by one copy of content that no correct file has |
| Clip shapes that meet or overlap | each pixel's coverage added up, never past all of it | the larger of the two, or `a + b - ab` | two shapes meeting along an edge (two halves, say) add up to the whole of each pixel on it; with either alternative that edge shows as a faint seam. The cost: where two shapes overlap exactly at an edge, that edge comes out slightly harder. A seam is the more visible flaw |
| How gradient stops mix | channel by channel, alpha with the rest, not premultiplied | premultiplied | not a free choice: SVG 2 says so ("SVG does not calculate gradients in pre-multiplied space"), and icons are drawn to look right that way. Recorded because the first version was written the other way |
