### [E] apps/ had 47 lossy decodes reaching a value -- 2026-09-26
**Status:** FIXED (lane E, 2026-09-26). `python scripts/lossy-decode.py --under
apps` reports VALUE 0; `scripts/lossy-decode-baseline.txt` now lists only
`gui/` files. The backlog is `TD-C-THE-LOSSY-DECODE-CHECKER-NEVER-LOOKED-AT-TWO-THIRDS-OF-THE-TREE`'s
ratchet, cleared for lane E's half.

**In short:** nineteen programs turned a name that is not text into text by
replacing its odd bytes with U+FFFD and then used the result. Most only showed
it -- two such names drawn as the same row -- but four did real harm: the file
manager's rename box began with the lossy name, so accepting it unchanged
renamed the file; the archive manager keyed members by that string, so two
members became one entry; the indexer gave two such names one search key; and
backup and the indexer matched exclusions against a text that a pattern
holding U+FFFD could match. Names are now shown by their bytes through
`quoting::escape_unprintable` (the tree's one renderer for untrusted text),
matched by their bytes where the matcher allows (backup's glob, the indexer's
exclusions), and the rename box treats its own starting escapes, answered
unchanged, as no change. The photo manager's import, found beside it, also
read a whole file before its cap; it reads under the cap now.
