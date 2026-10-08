## B-JOURNALCTL-AND-SYSLOGD-PANIC-WHEN-OUTPUT-CANNOT-BE-WRITTEN (lane B, 2026-10-07)

**Status:** OPEN (lane B). The record listings are fixed; the rest is below.

**In short:** `journalctl` and `syslogd` print through Rust's `println!`,
which ends the program with a panic -- an internal error message and status
101 -- when standard output cannot take the line: a full disk, or a pipe
whose reader has gone, as in `journalctl --list-fields | head -1` (Rust's
runtime ignores `SIGPIPE`, so the write fails instead of the process dying
quietly). Both also read their command line with `std::env::args()`, which
panics on an argument that is not UTF-8. A script sees a crash where it
should see a one-line error and a status.

**Where:**

* `userspace/journalctl/src/main.rs`: `print_usage`, `cmd_list_fields`,
  `cmd_disk_usage`, the vacuum reports, and `main`'s `std::env::args()`. The
  record listings themselves -- every output format, and `-f` -- write through
  `render_all` since 2026-10-07, which stops at a failed write: quietly for a
  reader that has gone, `journalctl: write error: R` and status 1 otherwise.
* `userspace/syslogd/src/main.rs`: `print_usage`, `cmd_tail`, `cmd_query`,
  `cmd_follow`, `cmd_stats`, `cmd_rotate`, `cmd_clean`, each `println!`; and
  `tail`'s and `clean`'s counts, read with `.parse().unwrap_or(20)` and
  `.unwrap_or(30)`, so `syslogd tail banana` shows twenty entries rather than
  refusing. (`main` now reads its arguments as `OsString`s: the daemon takes
  its paths as they are, and the other commands refuse an argument that is not
  text instead of panicking.)

**Reproduce:** in WSL, `journalctl --list-fields >/dev/full` after any record
exists; `syslogd help >/dev/full`; `journalctl $'\xff'`.

**The fix:** both are SlateOS's own programs, so there is no upstream wording
to match: every output path through a writer whose failure is checked, as
`render_all` does -- `PROGRAM: write error: R`, status 1, and nothing said for
`EPIPE` -- arguments read as `OsString`s, and a count that does not parse
refused with a message rather than replaced by a default.
