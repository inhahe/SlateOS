# Known issues — resolved

Closed issues, one file per issue, named by the issue's id. An issue moves here
from `known-issues/` (`git mv known-issues/<ID>.md known-issues-resolved/`) once
its `**Status:**` line says FIXED, RESOLVED or CLOSED and the fix has survived a
full boot test on `main`. Nothing is ever deleted: each file keeps the entry's
full text, follow-ups and commit hashes. A reopened issue moves back.

How to write an issue, and the status rules: `known-issues/README.md`.
`scripts/check-docs.py` refuses a file here that says OPEN.
