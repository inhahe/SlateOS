### TD-C-SEVERAL-APPS-DISPLAY-DATA-THAT-NOTHING-PRODUCES — 2026-09-04 — OPEN

**In short.** A run of lane-C applications draw a complete, correct-looking
screenful of information that is a constant compiled into the program. The
drawing is real, the filtering and sorting and searching are real, the parsing
is real — and there is no source. This entry names the pattern, because it has
now been found sixteen times and writing it up sixteen times separately would train
the reader to skim it.

**`email` is the sharpest case and the least fixable today.** The client is
complete -- MIME parsing, RFC 5322 addresses, threading, filter rules, IMAP and
SMTP command builders with tests over each -- and none of the protocol half can
run, because `net/` has no client this lane can call. Twenty of its thirty-four
uncalled functions are that layer. What was wired instead is everything that
does not need a socket: reading, flagging, replying, forwarding, searching,
composing and building the message. Sending stops at the point where a socket
would be, and says so on the status line.

**`spreadsheet` is the mirror image and belongs here for the same reason.**
It is not short of a source — the user types the data — it is short of a
*sink*. `Sheet::export_csv` returns a `String` and `Sheet::import_csv` takes
one, both implemented and tested, and there is nowhere for that string to come
from or go: no file dialog, no command line, and no system clipboard (the app's
`clipboard` field is its own internal cell buffer). The shape is the same — a
capability that is complete except for the one end that touches the world — and
the fix is the same shape too: `gui/toolkit` has a file chooser
(`guitk::dialog`), so this one is nearer than the rest.

**The apps, and what each one is missing.** Every one of these was wired to the
compositor between 2026-09-03 and 2026-09-04; the wiring is what made the gap
visible, because a program that only ever printed to stdout has no obvious
promise to break.

| app | what it shows | what it lacks | its `tick_interval` |
|---|---|---|---|
| `weather` | a full forecast for a city | any forecast source; the settings offer a 5–120 min update interval with nothing behind it | `None`, documented |
| `logviewer` | 21 log lines, filterable | any file; its header promises "real-time log tailing" and it draws an auto-scroll toggle | `None`, documented |
| `sysinfo` | CPU, memory, disks, uptime | any read at all — uptime is the string `"4h 23m 17s"` | `None`, documented |
| `sysmonitor` | processes, live graphs, alerts | a real process source, but the *clock* is now real | the refresh interval |
| `finance` | accounts, budgets, transactions | ~~both a source and a way to enter anything; see its own entry~~ -- both, 2026-09-25: forms, and a ledger kept as it changes | until midnight, for today's date |
| `email` | an inbox, folders, threads, filters | a network. Every IMAP and SMTP command it can build -- `login`, `select`, `fetch`, `ehlo`, `mail_from`, twenty in all -- returns a protocol string with no socket to write it to | `None`, documented |
| `rssreader` | feeds, folders, articles, search | an HTTP client. Its RSS/Atom parser is real and now runs at startup on one sample feed, but nothing can fetch a second one, so `global_auto_refresh_seconds` has nothing to refresh | `None`, documented |
| `ebook` | library, pagination, bookmarks, contents, search | a window that can be resized. The reading position is a byte offset precisely so it survives repagination, and nothing ever repaginated because nothing ever changed size | `None` -- nothing in a book advances on its own |
| `torrent` | transfers, peers, pieces, trackers | a network. `TrackerRequest::build_url` builds an announce URL nothing can fetch, so no peer list ever comes back -- the swarm a download picks pieces from is invented at the first tick | `PIECE_STEP` while downloading -- since 2026-09-25 only while a download has a peer, so never; see `[E] The torrent client transfers nothing` |
| `spreadsheet` | a sheet a user can actually fill in | nothing to *show* — the gap is the other way round: `import_csv`/`export_csv` work on a `String` and there is no file dialog, command line or system clipboard to carry one | `None`, documented |

**The rule that came out of it, and it is not "wire a tick".** In each case the
tempting fix is to return a poll interval from `tick_interval` so the app "feels
live". That is wrong wherever there is no source: it wakes the machine on a
timer to redraw a constant, which is `known-issues.md` lesson 47's cost paid for
none of its benefit. `sysmonitor` is the one that *should* tick, and it is
exactly the one whose data ages under it. **The question is not "is this app the
sort of thing that updates" but "is there something a tick would read".**

**`sysinfo` is the interesting one, because its source already exists.**
`userspace/sysinfo` — lane B's command-line system-information tool — reads
`/proc/cpuinfo`, `/proc/meminfo`, `/proc/loadavg` and walks `/proc` for
processes. The graphical one twelve directories away reads nothing. Filed as
`requests/c-b-the-proc-readers-in-userspace-sysinfo-should-be-a-crate-both-sysinfos-can-use.md`,
asking whether the readers can be a shared crate rather than a second parser for
the kernel's interface.

**Checked before generalising, because the obvious version of this is wrong.**
The four `apps/` ↔ `userspace/` pairs are not duplicates in general:
`apps/backup` does 23 file operations to `userspace/backup`'s 13, and
`apps/indexer` 23 to `userspace/indexer`'s 8. Those graphical apps are not
hollow. `sysinfo` is the only pair where one side has what the other lacks.

**Proper fix, per app.** Each needs its own source and they are not the same
job: an HTTP client for `weather` (lane C has `net/` but no app makes an
outbound request yet), a tailing file reader for `logviewer` (with rotation and
half-written-line handling), the shared `/proc` readers for `sysinfo`, a process
source for `sysmonitor`, and for `finance` a creation UI and a store — see
`TD-C-FINANCE-IS-A-VIEWER-OVER-SAMPLE-DATA`.

**Why none of it is urgent.** Nothing here loses or corrupts anything, because
there is nothing yet to lose. The cost is that each of these programs asserts on
its own screen that it is doing something it is not, and a reader looking for
the loop that does it will not find one. That is what these entries are for.
