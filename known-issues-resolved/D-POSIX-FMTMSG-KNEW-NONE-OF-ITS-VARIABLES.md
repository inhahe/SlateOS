## D-POSIX-FMTMSG-KNEW-NONE-OF-ITS-VARIABLES — `fmtmsg` ignored `MSGVERB` and `SEV_LEVEL`, never wrote to the console, accepted any label and printed an unknown severity, and `addseverity` did not exist (lane D, 2026-09-30) — **Status: FIXED 2026-09-30 (`posix/src/fmtmsg.rs`)**

**In short:** `fmtmsg` prints a structured diagnostic -- `UX:cat: ERROR:
can't open` and a `TO FIX:` line -- to standard error, the system console,
or both. POSIX lets the user choose which parts appear (`MSGVERB`), and
glibc lets the user and the program define extra severity levels
(`SEV_LEVEL`, `addseverity`). Ours had none of that: it printed every part
always, wrote nothing to the console (the `MM_CONSOLE` flag went to standard
error instead), printed a severity it did not know rather than refusing it,
accepted a label of any shape, and laid the second line out differently.
`addseverity` was missing, so a program that calls it could not link.

| | Was | Is |
|---|---|---|
| `MSGVERB` | ignored | the parts it names, on standard error; unset, empty or malformed: all |
| `SEV_LEVEL`, `addseverity` | ignored; missing | levels above `MM_INFO`, defined, redefined, removed, as glibc's |
| `MM_CONSOLE` | written to standard error | `/dev/console`, every part |
| an unknown severity | printed with no name | `MM_NOTOK`, nothing printed |
| a label not `10-byte:14-byte` | printed | `MM_NOTOK` |
| the layout | a first line, then `TO FIX:` and the action and tag when either was given: `UX:cat`, `TO FIX: tag` for a label and a tag | glibc's: `UX:cat: tag`; a tag goes after `TO FIX: action` when there is an action, alone on the second line after a text, on the first line otherwise |
| standard error | descriptor 2, around the stream | the `stderr` stream, one write; `MM_NOMSG`, `MM_NOCON`, `MM_NOTOK` as POSIX has them |

**One answer is not glibc's** (design-decisions §1150): glibc reads
`MSGVERB` and `SEV_LEVEL` at a process's first `fmtmsg`, so an `addseverity`
made before it is undone by `SEV_LEVEL`; here they are read at the first
call of either function, so a program's `addseverity` always has the last
word. `posix/src/fmtmsg_deviations.txt` lists the three cases.

**Tests:** `posix/tools/oracle/fmtmsg_harness.py` records glibc 2.39's
answers for 804 cases, each in a process of its own (`fmtmsg_oracle.txt`):
labels, every combination of the parts, severities, `MSGVERB` and
`SEV_LEVEL` values, `addseverity` sequences, standard error closed. The
tests replay them, and check each console message against the same case's
standard error with every part selected. `fmtmsg_model.py`, the rules
written a second way, agrees with glibc on all 804 in glibc's order and
writes the deviations in this library's.

**Where:** `posix/src/fmtmsg.rs`; `posix/include/fmtmsg.h` (`addseverity`).
