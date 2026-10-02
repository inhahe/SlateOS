### [E] The explorer's file-type columns showed the same invented values for every file -- 2026-09-25
**Status:** FIXED for pictures, source files and zip archives (lane E, 2026-09-25), and for audio files, video files, TAR and gzip archives (lane E, 2026-09-26); OPEN for a picture's colour depth (lane F's to read: `requests/e-f-a-pictures-colour-depth-from-its-header.md`), the count of files in a `.tar.gz`, and 7z and rar archives -- lane E's.

**In short:** the file explorer's detail view can show extra columns for
pictures (size, colour depth, shape), songs (length, bitrate, artist...),
source files (lines) and archives (files inside, compressed size). Every one of
those cells was made up: every picture was "1920 x 1080, 24-bit, 16:9", every
song "3:42, 320 kbps, Unknown Artist", every source file 0 lines, every archive
0 files. A user who turned a column on saw the same numbers down the whole
list. The code said so -- "Stub: in a real implementation, read image headers"
-- and a test pinned the invented 3:42.

**Fixed.** Pictures are measured by `imagecodec::dimensions`, which reads the
header only (64 KiB first; a TIFF whose directory is further in, up to 64 MiB),
and turns the size the way the picture is shown; the ratio is worked out from
it (`16:9`, or `1.78:1` when the lowest terms are not small). WebP, ICO and TIFF
joined the column's formats. Source files are counted (line feeds, plus an
unfinished last line; files over 16 MiB are left blank). A zip archive's own
directory is read through `ziparchive::parse_at` from the end of the file:
files inside, their compressed size, and the share that saves. Each provider
keeps what it read per file while the file's size and time are unchanged
(`FactCache`), because a provider is asked for every visible row every frame.

**Fixed, 2026-09-26: audio.** The readers were inside `apps/musicplayer`'s
binary, where nothing else could reach them; they are the crate
`apps/audiotags` now, which both use, and the move finished them. They had
decoded an ISO-8859-1 ID3 frame as UTF-8 (a title with an "e acute" in a
Latin-1 tag was dropped whole) and kept the NUL a text frame may end in;
computed a FLAC's bit depth as `hi | (lo + 1)` (a 32-bit FLAC read as 16); and
read no MP3's length or bitrate, no Ogg file, no FLAC's own tags, no WAV's
`LIST`/`INFO` and no ID3v1 tag. `audiotags` reads MP3 (the first frame header
confirmed by the next; a Xing/Info or VBRI header for a VBR file; ID3v2.2 to
2.4, unsynchronisation, every text encoding; ID3v1 behind it filling what v2
does not say), FLAC (STREAMINFO and its Vorbis comments, and a FLAC behind an
ID3 tag), Ogg Vorbis and Opus (the first packets and the last page's granule,
Opus at 48 kHz less its pre-skip) and WAV (`fmt `, `data`, `LIST`/`INFO`),
bounded throughout. The six audio columns read from it through the same
`FactCache`; the bitrate and sample rate are a `ColumnValue::Measure`, which
sorts by the count ("96 kbps" before "320 kbps", which as text it was not).

The move turned up that the **player never called its own readers**: every
track it listed showed its file name, "Unknown Artist" and 0:00, whatever the
file said. A playlist's tracks read their files now, and a relative M3U entry
is taken from the playlist's folder, as M3U means it (it was taken from the
player's working directory).

**Fixed, 2026-09-26: video, TAR and gzip.** Video files had no columns at
all; a new provider reads them through `apps/mediaprobe` into the Duration,
Bitrate and Dimensions columns music and pictures already have (shared by id:
one Duration column for everything that plays) and a new Frame Rate. A TAR's
headers are read through `apps/tararchive` -- its files, their size, and a
compression ratio of 0%, which a TAR honestly has. A gzip file's trailer gives
the size it inflates to, so its ratio is read without inflating it; a plain
`.gz` holds one file.

**Still open, and blank rather than guessed:**
- **Colour depth** -- `imagecodec` does not report a picture's bit depth; a
  header-only `pixel_format` beside `dimensions` would be lane F's, and is
  asked for in `requests/e-f-a-pictures-colour-depth-from-its-header.md`
  (2026-09-26).
- **How many files a `.tar.gz` holds** -- counting them means inflating the
  whole archive, synchronously, while the window draws the row. The proper
  fix is a background reader for the columns (the thumbnails have one); until
  then the cell is blank, not a guess.
- **7z, rar** -- nothing in the tree reads them.

**Where.** `apps/explorer/src/columns.rs`: `ImageColumns`, `AudioColumns`,
`CodeColumns`, `ArchiveColumns`, `FactCache`, `image_size`, `line_count`,
`zip_facts`, `audio_facts`. `apps/audiotags`. `apps/musicplayer/src/main.rs`:
`Track::read_facts`, `PlayerState::load_m3u_from`.
