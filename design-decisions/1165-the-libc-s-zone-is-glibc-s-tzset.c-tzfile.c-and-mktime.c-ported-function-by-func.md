## 1165. The libc's zone is glibc's: `tzset.c`, `tzfile.c` and `mktime.c`, ported function by function, quirks and all

**Date:** 2026-10-01
**Decided by:** Claude (autonomous)
**Lane:** D

**In short:** a C program on SlateOS now reads `TZ` exactly as it would on
Linux with glibc 2.39. It picks the same zone, prints the same local times,
holds the same names in `tzname`, and gets the same instant back from
`mktime` for a wall-clock time a clock change skipped or repeated. Before,
the libc read `TZ` its own way through `tzrules`, and for some values a C
program and `date` on the same machine printed different hours: `TZ=EST5EDT`
an hour off for any year before 2007, `TZ=Foo/Bar` UTC named `UTC` where
glibc says `Foo`. Every one of the 1096 answers glibc gives in the new
oracle is now this library's too.

**What it is.** `posix/src/tz.rs` is glibc's three files, function for
function. glibc's static variables are one state behind one lock, as
glibc's are behind `tzset_lock`, so the parts of glibc's behaviour that
depend on what a process did before are reproduced, not tidied away:
- a rule with a DST name and no dates borrows `posixrules`' history,
  re-anchored by an offset that the last such rule left behind;
- `compute_change`'s cache starts at zero for half a rule that did not
  parse;
- `mktime` starts its search from the offset the previous call found, so
  which of an autumn hour's two instants it returns depends on what came
  before;
- `localtime_r` in a zone read from a file moves `tzname` to the names in
  force around the instant;
- an instant past the file's last transition moves all three globals to its
  footer rule's;
- glibc's check meant to restore a `posixrules` zone's own names at that
  point compares with an address its allocator never returns, so it never
  does, and here neither.

**Held to it by** `posix/tools/oracle/tz_harness.py`. It runs 57 scenarios
under glibc 2.39, each a process of its own. Every row of lane B's table is
a scenario, and so are the files glibc reads (recorded byte for byte, and
offered to the replay as its file system): the `tzname` globals after every
call, `localtime_r` around the changes of fourteen zones, `mktime` in their
gaps and overlaps, and `%Z %z`. `posix/src/tz/tests.rs` replays all of them
through the C entry points. A mutation that drops `daylight`'s value from
`localtime_r` turns 338 of its lines red.

**Where not**, each on purpose:
- *A `TZ` naming a file through `..` is never read*; glibc refuses one only
  in a set-user-ID program. `TZ` is inherited from whoever started the
  program, and a file read is an oracle for "does this path exist and
  parse". `userspace/localtime` refuses it too, so the two readers of one
  `TZ` agree. The value then reads as a rule, which the oracle holds to
  glibc's answer for a value that names no file and is no rule.
- *Leap seconds are not applied* (a `right/` zone's records are skipped);
  glibc would shift the clock.
- *A footer with a DST name and no dates* gets the US rules; glibc would
  run `__tzfile_default` from inside a lookup and replace the zone mid-call.
  `zic` never writes one.
- *Names live in an 8 KiB arena*, not `malloc`ed blocks. A name past it is
  refused, which is what a failed `malloc` is in glibc.
- *A `TZ` over 1024 bytes is not remembered*, so `tzset` reads it again
  each time.
- *A zoneinfo file over 16 KiB is refused*, read into a fixed buffer;
  glibc reads any size. tzdata's largest is under 4 KiB.

**The alternatives.**
- *Keep `tzrules`' engine and write down why it differs*, lane B's option 2.
  It is less code, but then C programs and `date` disagree on purpose, on
  values a user can type. The request exists because they already did.
- *Move one port into `tzrules`, for the libc, `userspace/localtime` and the
  desktop to share.* That is one copy, which is the right end state. But
  lane B's port is `std` (it keeps tables in `Vec`s and has no use for
  `tzname`), the libc needs the globals and a static file buffer, and the
  crate is lane C's. This port is `no_std` and could become that shared one;
  lane C is told so in the reply to `requests/c-bd-read-tz-through-tzrules-tz-source.md`.

**D-Q6:** written from glibc 2.39's own source. `time/tzset.c`,
`time/tzfile.c`, `time/mktime.c` and `time/mktime-internal.h` were read
(the GitHub mirror of the 2.39 tag), as recorded in `open-questions.md`
D-Q6.

**Where:** `posix/src/tz.rs`, `posix/src/tz/tests.rs`,
`posix/src/tz_oracle.txt`; `posix/src/time.rs` (`tzset`, `localtime`,
`localtime_r`, `gmtime`, `gmtime_r`, `mktime`, the three globals);
`posix/tools/oracle/tz_harness.py`.
