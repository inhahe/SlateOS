## `apps/musicplayer`: ID3 tags could forge M3U playlist entries (FIXED)

`export_m3u` interpolated `track.artist` and `track.title` straight into the
`#EXTINF:` line. Those two fields are not the user's: `Track::update_from_data`
sets them verbatim from the file's own ID3v2 tags, so for any downloaded file
they are chosen by whoever produced it. `load_m3u` reads every non-`#` line as
a **file path**, so a title containing a newline injected arbitrary entries
into the user's playlist.

M3U is where this audit's usual answer runs out: the format is bare
line-oriented text with no quoting and no escape syntax, so a line break
cannot be escaped — only removed or refused. The fix splits on which of those
is honest for each field:

- `#EXTINF` metadata is advisory display text, so CR/LF become a space
  (`m3u_field`). Losing a newline out of a song title costs nothing.
- A **path** containing CR/LF is legal on this OS (all bytes but `/` and NUL)
  and has no M3U representation at all. Writing it anyway would silently point
  the entry at a different file, so the track is omitted — and *reported*:
  `export_m3u` now returns `M3uExport { text, skipped }` instead of a bare
  `String`, so a caller can tell the user rather than handing them a playlist
  quietly shorter than the one they exported.

The general point, third variant of it now: when a format cannot represent a
value, the choice is reject or sanitise, and it must never be "write it
anyway." GRUB got reject (control characters), M3U metadata gets sanitise, M3U
paths get reject-and-report.
