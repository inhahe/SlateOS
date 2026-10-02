## `TD-C-TWO-BYTE-ORDERS-ARE-BOTH-CALLED-ARGB` (lane C, 2026-08-26) -- **CLOSED 2026-09-07**

**Closed 2026-09-07 (lane C).** The proper fix this entry specified is done:
the byte order is part of the *type*. `guitk::canvas::WireBytes` has a private
field and exactly one constructor -- `Canvas::to_argb8888` -- and the upload
path takes nothing else. `ImageChange::Upload::bytes` and
`oswindow::Window::upload_image` both require it, so
`upload_image(.., canvas.to_argb())` is a compile error rather than a picture
with red and blue exchanged.

**One type covers both wire formats**, and that is not a shortcut:
`BufferFormat::Xrgb8888` has the same byte order as `Argb8888` and differs only
in whether the alpha byte is honoured, so the layout question this type answers
has one answer for both.

**Extraction is allowed and construction is not**, which is the whole design.
`into_vec` hands the bytes to the encoder that puts them on the wire, at the
one point past which no other order could be mistaken for them. Making that
cost a copy of a whole picture per upload would have been a real price for no
safety -- what needs guarding is what goes *in*.

**Two test fixtures were building wire bytes by hand** and had to stop, which
is the change working as intended. One of them asserted an upload's contents
against `vec![1, 2, 3, 4, 5, 6, 7, 8]` -- a literal encoding the test's own
guess at the byte order, in the one area of this tree where nobody should be
guessing. It now derives the expectation from the same canvas the fixture
uploads.

The naming, the cross-referencing tables and
`the_compositors_argb_is_the_byte_reverse_of_the_other_argb` all stay. They
were not wrong, they were just not *load-bearing*: nothing stopped the next
caller. See design-decisions.md §561.

**Still open, and named in the entry below:** the disk-cache order has not had
the same treatment. `Canvas::from_argb`/`to_argb` remain plain `Vec<u8>`, so
explorer's on-disk thumbnail cache can still be read with the wrong pair. That
is a smaller blast radius -- one app, one file format, and a wrong read there
produces visibly wrong thumbnails rather than a wrong picture on someone
else's screen -- but it is the same shape of bug.

Original entry follows.

---


**In short:** "ARGB" names two *opposite* arrangements of the same four bytes in
this tree, and both are spelled the same way in code. `Canvas::to_argb` writes
alpha first (`A, R, G, B`); the compositor's wire format
`BufferFormat::Argb8888` expects alpha last (`B, G, R, A`). Handing one to
something expecting the other is not a compile error and does not panic — the
picture simply appears with red and blue swapped and its transparency read out
of the blue channel. The file manager was written with exactly that bug and it
was caught by reading the two definitions side by side, not by any test.

**Where it lives.** `gui/toolkit/src/canvas.rs` (`from_argb`/`to_argb` versus
`from_argb8888`/`to_argb8888`) and `gui/remote/src/control.rs:186` (the wire
enum's definition). Every caller that hands pixels to the compositor —
`Thumbnail::to_wire_bytes`, the wallpaper upload, the image viewer — must use
the `8888` pair; every caller reading or writing explorer's on-disk thumbnail
cache must use the other.

**What has been done about it.** The `8888` pair is named after the *wire enum*
rather than after its byte order, so the two cannot be chosen between by reading
the name and guessing; each of the four functions carries a table saying which
is which and cross-references the other; the module doc opens with a "beware"
paragraph; and a test (`the_compositors_argb_is_the_byte_reverse_of_the_other_argb`)
asserts the reversal explicitly, so a change to either definition breaks loudly.
See design-decisions.md §561.

**Why that is not enough.** It is still two `Vec<u8>` types with identical
signatures. Nothing stops a future caller from writing
`canvas.to_argb()` into an `ImageChange::Upload`, which is precisely the mistake
that was made once already.

**What the proper fix is.** Make the byte order part of the *type*, not the
function name: a `WireBytes(Vec<u8>)` newtype that only `to_argb8888` can
produce and that `ImageChange::Upload`/`upload_image` require, so the wrong
buffer cannot be passed at all. The same treatment would suit the disk-cache
order. Cheap to do; it was not done in the same change because it touches every
upload site in `gui/**` and `apps/**` at once.

**How you would notice.** A thumbnail, wallpaper or photograph draws with red
and blue exchanged — a blue sky above orange grass — and semi-transparent
pixels come out wrong. Nothing reports an error.
