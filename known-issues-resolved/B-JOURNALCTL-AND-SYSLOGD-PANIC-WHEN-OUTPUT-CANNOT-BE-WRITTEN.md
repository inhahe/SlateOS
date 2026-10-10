## B-JOURNALCTL-AND-SYSLOGD-PANIC-WHEN-OUTPUT-CANNOT-BE-WRITTEN (lane B, 2026-10-07)

**Status:** FIXED 2026-10-08 (lane B); boot-tested on `main` the same day
(`b085a015f`, published as `cd4a13399`). Fixing it turned up more in the same
two programs, all fixed in the same series and listed under "Found on the
way".

**In short:** `journalctl` and `syslogd` printed through Rust's `println!`,
which ends the program with a panic -- an internal error message and status
101 -- when standard output cannot take the line: a full disk, or a pipe
whose reader has gone, as in `journalctl --list-fields | head -1` (Rust's
runtime ignores `SIGPIPE`, so the write fails instead of the process dying
quietly). Both also read their command line with `std::env::args()`, which
panics on an argument that is not UTF-8. A script saw a crash where it
should have seen a one-line error and a status.

**The fix:** every line either program prints goes through one buffered
writer whose failure is checked (`to_stdout`, in each): `PROGRAM: write
error: REASON` and status 1, and nothing said for a reader that has gone
(`EPIPE`), whose status is the command's own. Standard error is written
without panicking (`2>/dev/full`). Arguments are `OsString`s: a value
matched against a record's bytes -- `journalctl`'s `-u`, `-b` and `--grep`,
`syslogd query`'s `--service` and `--msg` -- may be any bytes, and a value
that must be text and is not is refused with the rest, quoted. `syslogd
tail N` and `clean N` refuse a count that does not parse, where they took
20 and 30 in its place.

**Found on the way, and fixed with it:**

* `syslogd`'s reading commands had a JSON parser of their own -- the one
  `journalctl`'s was fixed away from on 2026-09-26 -- which pushed each
  byte `as char` (`café` shown as `cafÃ©`), decoded no `\uXXXX`, and
  failed on a byte array, so every record its own daemon files with a
  field that is not text (design-decisions §1063) was never shown. The
  parser is now `journalrec::parse_object`, shared with `journalctl`.
* `syslogd tail`, `query` and `stats` read the log as one `String`: any
  failure but "not found" -- and one byte anywhere that was not UTF-8 --
  read as "No log file found." / "No log entries." / 0 entries. Now the
  file is bytes, a bad line costs only itself, and a read failure is said,
  status 1.
* `syslogd follow` sliced the re-read file at the old length, which panicked
  inside a multi-byte character, re-read the whole log twice a second, and
  lost a record caught half-appended. `journalctl -f` was keyed by file NAME,
  so at each rotation every name's offset was applied to the file that now
  had the name, and lines were shown twice or skipped. Both now follow
  through `journalio::Follow`, which knows each file by device and inode.
* `syslogd query` skipped an option it did not know (a misspelt `--servce`
  showed every record), an option missing its value, and a `--since` or
  `--limit` that did not parse; it now refuses each.
* `syslogd log` filed levels as typed (`error`, `warn`) where the daemon,
  `logger` and `systemd-cat` file `err` and `warning`, so `query --level
  error` missed the daemon's records and `stats` left them out of its
  breakdown. Levels are now filed and read by their canonical names
  (`journalrec::priority_name`).
* `syslogd tail N` took the last N lines, not records; `rotate` said "done"
  whatever happened (each rotation step's failure is now said, status 1, and
  the daemon reports one at most once a minute); `hours * 3600` and
  `days * 86400` could overflow.
* `journalctl`'s diagnostics printed Rust's `io::Error` wording ("(os error
  2)"); they use `strerror`'s now, as its write errors already did.
* Both programs turned a timestamp into a date a year at a time from 1970:
  one record with a `ts` near `u64::MAX` -- which anything able to write a
  line into the log can put there -- kept every listing busy for many
  minutes. `journalrec::Utc` does it in a fixed number of steps, both ways.
* `journalctl --since`/`--until` took a date before 1970 as the same day of
  1970, rolled February 31 and 25:99 over into the next month and hour,
  took a time field that did not parse as 0, counted a typed year one step
  at a time (without end for a large one), overflowed on a large relative
  count, and panicked on `-5é`. It refuses each now.
* `journalctl --vacuum-size` and `--vacuum-time` multiplied their number by
  the unit unchecked, which wraps in a release build: `--vacuum-size
  17179869184G` is 2^64 bytes, wrapped round to 0, and removed the whole
  journal. A limit too large to count is refused now, and a multi-byte unit
  no longer panics. Found by putting `journalctl` under the workspace lints,
  which both programs now are (off `scripts/workspace-lints-baseline.txt`).

**Reproduce (before the fix):** in WSL, `journalctl --list-fields >/dev/full`
after any record exists; `syslogd help >/dev/full`; `journalctl $'\xff'`;
`syslogd tail banana`.
