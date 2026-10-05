## 1202. The finance ledger is a tab-separated text file, kept on every change, and one that cannot be read whole is left alone

**Date:** 2026-09-25
**Lane:** E
**Decided by:** Claude (autonomous) -- the known-issues entry that asked for a
store proposed YAML; this is a different call, and Claude's to revisit

**In short:** The finance app now keeps what is entered. It writes one plain
text file -- `finance/ledger.txt` in the user's settings folder -- with one line
per account, budget and transaction, the fields separated by tabs. It is
rewritten, whole and atomically (a new file swapped in, so a crash leaves the
old one), after every change: there is no Save. If the file is there but cannot
be read completely, the app reads none of it, writes nothing over it, and says
so on screen for as long as it is open.

### Text lines rather than YAML

`TD-C-FINANCE-IS-A-VIEWER-OVER-SAMPLE-DATA` proposed "a YAML store under the
user's config directory". The design's rule is *YAML for configuration files*,
and a ledger is not configuration: it is a record that grows by a line a day,
for years.

| | Tab-separated lines (chosen) | YAML (`yamldoc`) |
|---|---|---|
| Size of a year | a line per transaction, ~100 bytes | a map per transaction, several times that |
| Reading it elsewhere | `cut`, `grep`, a spreadsheet's import | a YAML library |
| Consistency with settings | different from every settings file | the same as them |
| Round trip of text | four escapes (`\\`, `\t`, `\n`, `\r`), exact | exact |
| Comments and formatting kept | no -- the app owns the file | yes, the point of `yamldoc` |

`yamldoc` earns its keep on a file a person edits and the program must not
disturb; nobody hand-formats a ledger, and preserving formatting across
thousands of records is cost with no reader. Money is whole cents and dates
are `YYYY-MM-DD`, so nothing is rounded on the way through. The first line
names the format (`format 1`), and a ledger naming another is refused rather
than guessed at.

### All or nothing on reading

A ledger with one line not understood is refused whole. The alternative --
skip the bad line, read the rest -- looks kinder and is the dangerous one: the
next change saves the ledger, and the save writes back only what was read, so
the line not understood is deleted. Refusing leaves the file byte for byte as
it was, and the window says which line and why ("line 3: a date is not one"),
so it can be mended by hand or moved aside. A transaction naming an account
the ledger does not have is refused the same way.

### Kept on every change

The app keeps state the way `apps/habits` and `apps/flashcards` do: every
change is written as it is made, so closing the window, or a crash, loses
nothing, and there is no unsaved state to ask about. A save that fails is
reported in the status bar until one succeeds.

**Where it lives:** `apps/finance/src/main.rs`: `ledger_path`, `ledger_text`,
`parse_ledger`, `FinanceApp::{from_settings, load_ledger, save_ledger}`.

**How to reverse:** the format is behind `ledger_text`/`parse_ledger`; a YAML
store replaces those two and keeps `load_ledger`'s all-or-nothing rule, which
does not depend on the format.
