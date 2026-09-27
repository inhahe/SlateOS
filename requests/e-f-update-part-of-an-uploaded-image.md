# Lane E -> lane F: update part of an uploaded image

**Filed:** 2026-09-27 by lane E. **For:** lane F (`gui/window`:
`WindowHandle::upload_image`, `app::ImageChange`, `RequestBody::UploadImage`,
and the compositor's image store). **Status:** OPEN. Nothing is broken
meanwhile; it costs bandwidth and time per frame.

**In short:** the Remote Desktop (`apps/remotedesktop`) now shows a real VNC
session. The remote screen is one image, and VNC sends what changed as small
rectangles -- a blinking cursor is a few dozen pixels. But the only way to
change an uploaded image is to upload all of it again, so every one of those
small changes re-sends the whole screen: about 8 MB for a 1920x1080 desktop,
many times a second while anything moves. The ask is a way to replace a
rectangle of an image already uploaded.

## The ask

An `ImageChange` (and the request under it) that writes a rectangle of pixels
into the image already stored under `id`, leaving the rest as it was:

```rust
ImageChange::Patch {
    id: u64,          // an image already uploaded
    x: u32, y: u32,   // where the rectangle goes in it
    width: u32, height: u32,
    stride: u32,      // bytes per row of `bytes`
    bytes: guitk::canvas::WireBytes,
}
```

Refused, with nothing changed, when `id` is not stored or the rectangle does
not lie inside the stored image -- the same all-or-nothing rule
`upload_image` documents. The format is the stored image's, so it need not be
repeated.

## Why here and not in the app

The app already knows exactly which rectangles changed (each VNC update names
them); only the window API cannot say so. Splitting the screen into many
small images and drawing them as a mosaic would work around it, but every
application that shows a changing picture -- a video, a remote screen, a
canvas being painted -- would then carry its own tiling, and each would get
the seams wrong in its own way.

## For reference

`apps/remotedesktop/src/main.rs`, `pump`: a `dirty` flag per session and one
`ImageChange::Upload` of the whole screen per wake. With a patch it would keep
the list of rectangles `rfb::Update::Pixels` and `Copy` already carry, and
send those.
