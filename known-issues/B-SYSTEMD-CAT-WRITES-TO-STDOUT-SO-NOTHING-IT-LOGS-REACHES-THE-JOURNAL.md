## B-SYSTEMD-CAT-WRITES-TO-STDOUT-SO-NOTHING-IT-LOGS-REACHES-THE-JOURNAL (lane B, 2026-09-12)

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

**Not started.** The work is: append one record per input line to a file
under `/var/log/journal/`, with those fields; keep the `[journal] ` stdout
echo only if something depends on it, which nothing appears to. This is a
functional gap rather than a fabrication -- the prefix is a placeholder
transport, not an invented measurement -- so it is filed rather than
urgent.
