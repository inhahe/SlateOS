### [E] "Open with" opened nothing in six of the file manager's eight programs -- 2026-09-26
**Status:** FIXED for all six (lane E, 2026-09-26), and since 2026-09-27 for a file whose name is not UTF-8 too: lane F's `oswindow::app::ArgsOs` (§1330) hands the programs their arguments as bytes, and all six -- with the editor, the image viewer, match3 and pinball -- read them through it (`b0471617a`; `requests/e-f-a-file-named-on-the-command-line-may-be-any-bytes.md`, closed).

**In short:** double-clicking a file in the file manager runs the program the
associations name with the file's path after it. Six of the eight programs the
associations can name started through `oswindow::app::launch`, which refuses
every argument but `--display` -- it printed "unexpected argument" to a stderr
nobody sees and exited 2 before the window opened. So opening a song, a PDF, a
binary file, a disk image or an archive did nothing at all, and "show in
folder" from anywhere in the desktop opened no folder. The archive manager and
the explorer even read the path themselves first (`args_os().nth(1)`), then
called `launch`, which refused the very argument they had just used.

**Fixed.** Each parses its command line with `oswindow::app::Args` and starts
through `launch_with`, as the editor and the image viewer already did:

- **hexeditor** -- each file named opens in a tab of its own; every message is
  kept, so a file that could not be read is named beside the ones that opened.
  Its open also read the whole file (`std::fs::read`) before cutting it to the
  16 MiB cap, so the four-gigabyte file the cap is for was read in full first;
  it reads only as far as the cap now (`safeio::read_capped`).
- **pdfviewer** -- each PDF named opens in a tab; a file that fails leaves no
  empty tab behind, and every failure is shown.
- **archivemanager** -- the archive named opens; a second one named is said
  not to have been opened (a window holds one).
- **explorer** -- a folder named opens on itself; a file named opens on its
  folder with the file selected; a second path is said not to have been opened.
- **musicplayer** -- every song and every playlist named is listed and the
  first becomes the one shown. Ctrl+O read every pick as an M3U playlist, so a
  song chosen there was "stream did not contain valid UTF-8"; a song joins the
  list now and a playlist replaces it, which the window's own help line says.

- **videoplayer** -- it had no picker either, and its window said it "has no
  filesystem access". A file opens now from the command line, Ctrl+O or a
  playlist entry, and is read for what it holds by `apps/mediaprobe` (a new
  crate: MP4/MOV, Matroska/WebM and AVI headers -- length, and each track's
  codec, size, frame rate, sound and language). Nothing decodes a frame, so
  Play says so rather than running a clock over a black picture.

**Still open:**
- **Names that are not UTF-8** -- `Args::from_env` reads `std::env::args()`,
  which panics on such an argument, and `Args::rest` is `Vec<String>`. Lane F's
  (`gui/window`); the request says what would do it. Lane E's seven callers
  change one signature each when it lands.

**Where.** `main` and a testable `open_arguments` (`explorer_for` in the
explorer) in `apps/{hexeditor,pdfviewer,archivemanager,explorer,musicplayer,videoplayer}/src/main.rs`;
the music player's `add_song`, `add_playlist`, `open_picked` and
`PlayerState::add_m3u_from`; the video player's `MediaFile::open`,
`open_path`, `add_path` and `load_playlist_entry`; `apps/mediaprobe`.
