### [F] The scene stream forwards no pictures, so a remote viewer would show blanks where windows draw images -- 2026-09-28

**Status:** FIXED on `lane-f` 2026-09-28, as sketched below: scene version 2
(`SceneImage` whole / patch / drop per window), `ImageAsset` revisions and a
32-entry patch log stamped by the compositor, `SceneSession` sending each
viewer only what it lacks, and `SceneViewer` applying it all or nothing.
Tests: `guiremote`'s scene unit tests and `tests/scene_session.rs` (400 steps
of random uploads, patches and drops, a viewer joining late),
`compositor::tests::test_stream_forwards_pictures_to_the_viewer`. Moves to
`known-issues-resolved.md` once on `main` through a boot test. Found while
scoping the video-encoded capture fallback; nothing was broken for anyone,
because nothing yet serves the scene stream to a viewer.

**In short:** native remote desktop streams each window's draw commands (a
`SceneFrame`, `gui/remote/src/scene.rs`) for the viewer to replay. A picture
is drawn by a command that names an image *id*; the pixels reach the
compositor separately (`UploadImage`, `PatchImage`, `DropImage`) and are
never put in the stream. So a viewer replaying the commands would draw
nothing wherever a window shows a picture -- every thumbnail, photo and icon
that is an uploaded image. The roadmap marks native streaming done; this is
the half of it that was never there.

**Where.** `Compositor::capture_stream_frame` (`gui/compositor/src/lib.rs`)
walks the windows' render trees only; `SceneSession` tracks command
fingerprints, not images; `SceneFrame` has no field for pixels.

**The proper fix.** A scene-protocol version 2 carrying image changes beside
the windows: per window, `Upload` / `Patch` / `Drop` exactly as a client sends
them. `SceneSession` records, per (window, image id), which version of the
image its viewer holds; the compositor's image store bumps a version on
every upload and patch and keeps a bounded log of recent patches, so a
viewer one or a few patches behind receives the patches (a remote desktop's
cursor blink is a few dozen pixels, not a screen) and one further behind the
whole image again. A window leaving the stream takes its images with it.
Tests: capture, encode, decode and `apply_scene_frame` end to end, with an
upload, a patch, a drop and a late-joining viewer.
