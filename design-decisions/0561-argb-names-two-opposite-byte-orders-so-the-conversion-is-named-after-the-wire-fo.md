## 561. "ARGB" names two opposite byte orders, so the conversion is named after the wire format and lives in exactly one place

**Date:** 2026-08-26
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** a colour is four bytes — red, green, blue, and transparency — and
there are two ways round to store them. This tree uses both, and calls both of
them "ARGB". The drawing surface stores transparency first; the display server's
wire format expects it last. Passing one where the other is wanted compiles
cleanly, never crashes, and produces a picture with red and blue exchanged and
transparency read out of the blue channel. The file manager's first draft did
exactly that. The decision is about how to make that mistake hard, given both
orders must continue to exist.

**The two orders, and why neither can go.**

| | Bytes, low to high | Who requires it |
|---|---|---|
| `Canvas::from_argb`/`to_argb` | `A, R, G, B` | the file manager's on-disk thumbnail cache, whose saved files are in this order already |
| `BufferFormat::Argb8888` (`gui/remote/src/control.rs:186`) | `B, G, R, A` | every buffer handed to the display server; it is a little-endian `u32` of `0xAARRGGBB`, which is what the rasteriser's `blend_pixel` reads |

The wire order is fixed by the format's own definition and by the hardware
convention it follows. The other is fixed by files already written to users'
disks. Neither is free to change.

**The decision.** `Canvas` gained a second pair, `from_argb8888`/`to_argb8888`,
and `Thumbnail::to_wire_bytes` routes through it — decode with one order, encode
with the other. Three things about that:

1. **Named after the wire enum, not after its byte order.** `to_bgra` would have
   been more descriptive of the bytes and *worse*, because the two names would
   then be `to_argb` and `to_bgra` and a caller would pick between them by
   guessing which one the compositor wants. `to_argb8888` matches
   `BufferFormat::Argb8888` character for character at the call site, so the
   match is checkable by eye.
2. **In `Canvas`, not hand-rolled at the call site.** The obvious
   implementation is `chunks_exact_mut(4)` + `reverse()` in the thumbnailer. It
   would work. It would also be a *third* statement of the byte order, free to
   drift from the two definitions it sits between, and it would silently accept
   a buffer with a trailing partial pixel where `Canvas::from_argb` returns
   `None`. Routing through `Canvas` gets the length check for free.
3. **Asserted, not just documented.** `the_compositors_argb_is_the_byte_reverse_of_the_other_argb`
   states the relationship as an executable fact: `Color::rgba(1,2,3,4)` is
   `[3,2,1,4]` in wire order, that is `to_argb()` reversed, and reading it as a
   little-endian `u32` gives `0x0401_0203`. A change to either definition breaks
   it loudly.

*Alternative considered — one order everywhere, converting the disk cache on
read.* *For:* one order is unambiguously simpler than two. *Against:* the
conversion does not go away, it moves — and it moves to a path that runs on
every cache hit rather than only on upload, which is the opposite of where you
want it. It also does not remove the wire format's constraint, only hides it.

**What this does *not* fix, and is logged as debt.** Both orders are still
`Vec<u8>` with identical signatures, so nothing *prevents* the wrong one being
passed — only the naming, the docs and the test discourage it. The real fix is a
newtype the upload path requires and only `to_argb8888` can produce; it touches
every upload site in `gui/**` and `apps/**` at once, which is why it is not in
this change. See `TD-C-TWO-BYTE-ORDERS-ARE-BOTH-CALLED-ARGB`.

**Where it lives.** `gui/toolkit/src/canvas.rs` (`from_argb8888`, `to_argb8888`,
the cross-references on `from_argb`/`to_argb`, the module-doc warning, and two
tests), `apps/explorer/src/thumbs.rs` (`Thumbnail::to_wire_bytes`),
`apps/explorer/src/main.rs` (`take_images`, and
`an_upload_carries_wire_order_bytes_and_not_the_stored_ones`).
