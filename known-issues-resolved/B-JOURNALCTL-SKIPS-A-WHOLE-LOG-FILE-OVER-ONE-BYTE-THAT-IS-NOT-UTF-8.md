## B-JOURNALCTL-SKIPS-A-WHOLE-LOG-FILE-OVER-ONE-BYTE-THAT-IS-NOT-UTF-8 (lane B, 2026-09-26) — FIXED 2026-09-26

**In short:** `journalctl` reads each log file with `fs::read_to_string` and,
if that fails, moves on to the next file without a word
(`read_all_entries` in `userspace/journalctl/src/main.rs`). A single byte
that is not valid UTF-8 anywhere in `/var/log/syslog.jsonl` therefore makes
`journalctl` show *nothing* from that file -- every record in it, silently,
with exit status 0. The same failure hides a file `journalctl` cannot open
at all, where the user is told nothing either.

**Reproduce:** append `printf '\xff\n'` to the log, then `journalctl`: the
earlier records are gone from its output.

**Where it comes from:** a writer that puts a message's raw bytes into the
file (anything outside `journalrec::escape`, which takes `&str`), a torn
write, or disk corruption -- the three things a log reader exists to survive.

**The same read, twice more.** `--vacuum-time` (`read_to_string`, then
`continue` on failure) leaves such a file alone without saying so -- the safe
direction, still silent. And `-f` re-reads the WHOLE file every 500 ms, then
slices the `String` at the previous length, `&content[prev_size as usize..]`,
which panics when that offset falls inside a multi-byte character -- as it
does when a writer's append was torn mid-character and completed later.

**The proper fix:** read the file as bytes and split on `\n`, so one bad
record costs that record and no other; in `-f`, read only the bytes past the
previous offset and carry an unterminated last line to the next round; report a record that is not UTF-8,
or not a record, as such (journald's own `journalctl` shows such data as
`[N bytes blob data]` rather than dropping it); and report an unreadable
file as an error naming it, with a non-zero status, instead of skipping it.
Found while correcting TD-B-NOTHING-RECEIVES-SYSLOG-MESSAGES, which had
misdescribed how `journalctl` chooses its files.

**Fixed 2026-09-26.** Every read is bytes, line by line: `record_of` parses
one line, so a bad byte costs its line. Lines that are not records are
counted per file and reported on stderr ("N lines are not a journal record
and not shown"), which is what `/var/log/syslog`'s text lines now produce
instead of vanishing. An unreadable file or directory -- including a log
file whose NAME is not UTF-8, which discovery used to skip -- is reported,
with exit status 1. `-f` takes its offsets from the same read as its
listing (records appended between the two were lost), reads only the bytes
past each offset, carries an unterminated last line to the next round (a
record written in two pieces was lost), and re-reads a file that got
shorter from its start (a truncated or rotated file lost what was written
before the next poll). `--vacuum-time` reports a failed rewrite as
`--vacuum-size` does, and exits 1. Eight tests; the torn-append one is what
found B-JOURNALCTL-SHOWS-NON-ASCII-TEXT-AS-MOJIBAKE.
