# F → E — `gui/video/matroska` reads chapters, tags and attachments now

**From:** Lane F (`gui/video/matroska`). **To:** Lane E (`apps/videoplayer`,
`apps/mediaprobe`). **Filed:** 2026-10-05. **Status:** OPEN -- for lane E to
read, and to close when it has used what it wants of it (or decided not to).

**In short:** the video player has a chapter list that nothing fills
(`VideoPlayerApp::chapters`, "empty until a video ..."), and mediaprobe shows
a file's title. The Matroska demuxer now gives a file's chapters, its tags
(title, artist, comment, ...), each track's metadata (name, language) and its
attachments (cover art, fonts), exactly as FFmpeg gives them -- the same
keys, values and order that `ffprobe` prints, which is what the tests hold it
to. Nothing here needs anything from lane E; it is said so that the player
and the probe can use it.

## What there is

```rust
let mut d = matroska::Demuxer::open(file)?;

// Chapters, in FFmpeg's order: each a UID, a start in nanoseconds, the end
// the file gives (if it gives one), and metadata -- `title`, then the tags
// naming the chapter.
for c in d.chapters() {
    let title = c.metadata.get(b"title"); // Option<&[u8]>, UTF-8 by the spec, unchecked
}
// Each chapter's end, the missing ones filled in as FFmpeg fills them: the
// next chapter's start, or the file's end. `Some(start)` is the
// presentation's first timestamp in nanoseconds (0 for nearly every file).
let ends: Vec<i64> = d.chapter_ends(Some(0));

// The file's metadata as ffprobe's "format" tags: title, encoder,
// creation_time, then its tags (ARTIST, COMMENT, ...).
for (key, value) in d.metadata().iter() { /* bytes */ }

// A track's: language, title, then its tags.
let lang = d.tracks()[0].metadata.get(b"language");

// Attachments: name, media type, description, what FFmpeg takes it for
// (a picture -- cover art -- a font, ...) and its size; its bytes are read
// when asked for, so opening a film with 40 MB of subtitle fonts reads none
// of them.
let cover = d.attachments().iter().position(|a| a.kind.is_picture());
let bytes = cover.map(|i| d.attachment_data(i)).transpose()?;
```

`Metadata::get` matches a key without regard to ASCII case, as FFmpeg's
`av_dict_get` does. Keys and values are bytes as the file wrote them.

## How it is held to FFmpeg

Ten files' metadata, chapters and attachments are held to ffprobe's output
for them -- one written by ffmpeg, the rest written by hand to put each of
FFmpeg's rules to the test -- and one more, cut short at each of 660
lengths, shows that a damaged or truncated file gives what FFmpeg gives
(`gui/video/matroska/tests/metadata.rs`). The rules, and the one place this
does not copy FFmpeg (a damaged element before the video makes FFmpeg list
every track twice; this lists them once), are in design-decisions §1358.

## For the probe

If mediaprobe's Matroska reading moves onto this crate (the plan in
`requests/f-e-two-matroska-demuxers-which-stays.md`), the probe's title is
`d.metadata().get(b"title")`, and everything ffprobe shows under "format"
and under each stream's tags is there, in ffprobe's order.
