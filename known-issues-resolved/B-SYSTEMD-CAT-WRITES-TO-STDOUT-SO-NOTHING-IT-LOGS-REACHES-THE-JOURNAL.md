## B-SYSTEMD-CAT-WRITES-TO-STDOUT-SO-NOTHING-IT-LOGS-REACHES-THE-JOURNAL (lane B, 2026-09-12)

**Status:** FIXED 2026-09-12 (`fdb538fda`, the same day; this entry was not
updated until 2026-10-09). `systemd-cat` appends a JSON-lines record per line to
the journal `journalctl` reads, under the journal's lock, with `-p` as `level`
and `-t` as `service` (`--pid` was never `systemd-cat`'s: it belonged to
`systemd-notify`, which shared the file). On `main`, boot-tested with
`0c2886e3c`; moved to `known-issues-resolved/`. Re-reading it on 2026-10-09
found what the fix left out -- a COMMAND to run, `--stderr-priority`,
`--level-prefix`, a timestamp per line, lines that are not UTF-8, and
unknown options -- which are fixed in the change after this one.

Found while chasing the three options `check-help-vs-parser` flagged on
`systemd-cat` (`-p/--priority`, `-t/--identifier`, `--pid`, all advertised
and none parsed). Deciding whether to implement or de-advertise them meant
asking what they would *do*, and the answer is the actual defect.

`run_cat_journal` reads stdin and writes each line to **stdout** prefixed
with `[journal] `. There is no journal in it. Meanwhile
`userspace/journalctl` reads real JSON-lines records out of
`/var/log/journal/` (`JOURNAL_DIR`), so a line piped through `systemd-cat`
is never readable by `journalctl` -- the two tools that exist to be each
other's ends do not meet.

That makes the three options implementable and worth implementing, rather
than removable: they are exactly the fields the record format carries.
`journalctl` parses `ts`, `msg`, `pid`, `level` (falling back to
`PRIORITY`) and `service` (falling back to `unit`), so:

| option | field |
|---|---|
| `-t`, `--identifier=ID` | `service` |
| `-p`, `--priority=PRIO` | `level` |
| `--pid=PID` | `pid` |

One wrinkle worth writing down before someone trusts the module doc: it
gives the example record as `{"ts":…,"level":…,"service":…}` but the
parser accepts `unit` as an alternative and the doc does not say so. Write
`service`, since that is what the documented example uses and what the
parser tries first.

**Not started** (when written). The work was: append one record per input line to a file
under `/var/log/journal/`, with those fields; keep the `[journal] ` stdout
echo only if something depends on it, which nothing appears to. This is a
functional gap rather than a fabrication -- the prefix is a placeholder
transport, not an invented measurement -- so it is filed rather than
urgent.
