### TD-C-LOGVIEWER-TAILS-A-STRING-COMPILED-INTO-ITSELF — 2026-09-04 — FIXED 2026-09-25

**Fixed 2026-09-25 (lane E, which owns `apps/` since the six-lane split).** The
viewer reads files. `main` opens the system journal
(`journalrec::MAIN_LOG_PATH`, `/var/log/syslog.jsonl`), and Open or Ctrl+O opens
any other log in a tab of its own. The details the entry below names are all
handled, and each has a test and a mutation that proves the test sees it:

- **Following.** While a log with a file is open the window takes a one-second
  clock (`tick_interval`) and reads what the file has grown by. Following
  decides only whether the selection goes to what arrives.
- **A half-written last line** is shown -- a file that ends without a newline is
  complete as far as anyone can know -- and kept aside as bytes; when the rest
  arrives, that entry is read again with it, so it becomes one entry, keeps a
  bookmark given to its first half, and a character cut in two by the read is
  whole.
- **A log truncated or replaced** under the viewer (shorter than what was read)
  is read again from the start.
- **A log too large to read whole** is read from its end, at the first whole
  line after the last 64 MiB; at most the newest 100 000 entries are kept, and
  lines that would be let go at once are not parsed at all.
- **Export** writes the exact bytes of the lines the filter shows, re-read from
  the file and checked against a fingerprint of what was read, so a log changed
  on disk since is refused rather than exported as lines that are not the ones
  shown; it will not write over the log itself.

The twenty invented entries survive as `App::with_sample`, for tests only.

**In short.** The log viewer's own description promises "real-time log tailing
with auto-scroll", and it draws an auto-scroll indicator in the status bar that
the user can toggle. There is no log. The twenty-one lines it shows are a string
literal compiled into the program, and nothing in the crate opens a file. The
window is a working log viewer with nothing to view.

**What is real and what is not.**

| real | not real |
|---|---|
| the JSON-lines parser, and it is now hardened against truncated input | any file being read |
| filtering by severity, source, time and text | any of it updating |
| bookmarks, search, the stats view | the auto-scroll toggle, which has nothing to scroll |
| the entry-count and search-result caps | the tailing the header promises |

**Why `tick_interval` is `None`.** Returning a poll interval would wake the
machine on a timer to re-render a buffer that cannot change — `known-issues.md`
lesson 47's cost with none of its benefit. The doc comment on the method says
this and says what to return instead once a source exists.

**Proper fix.** `LogFile` already carries a path (`/var/log/system.log`), so the
shape is there: read the file at startup, remember the offset, and on each tick
read from that offset to the end and append. Two details make it more than a
`fs::read_to_string`: a file that is rotated or truncated must be detected and
re-read from the start rather than silently stopping, and a line that is only
half-written at the moment of the read has to be held over rather than parsed —
which is the case the parser was hardened for in the same commit that filed
this.

**Not urgent, and it does not get worse.** Nothing is lost. The reason to write
it down is that the app asserts otherwise on its own status bar, and a reader
looking for the tailing loop will not find one.
