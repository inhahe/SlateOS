# Known issues

Bugs and technical debt that are **still wrong**, one file per issue:
`known-issues/<ID>.md`. Closed ones live in `known-issues-resolved/`. Until
2026-10-02 both were single files (`known-issues.md`, `known-issues-resolved.md`);
citations of the form `known-issues.md TD-B-FOO` still name the entry, because the
id is the file name.

## Writing an issue

* **Name the file by the entry's id**, and start it with the heading: `## TD-B-FOO-BAR
  (lane B, 2026-10-02) — OPEN` or `### [E] A prose title -- 2026-10-02`. Put your lane
  letter in the heading. An id is upper-case and hyphen-joined (`TD-<lane>-...`);
  a prose title is fine too, filed as `<lane>-<slug>.md`.
* **Put a `**Status:**` line immediately under the heading**: `OPEN`, `FIXED <date>`,
  `RESOLVED <date>`, `CLOSED <date>`, and keep it current. **Write the stamp in
  capitals**: on 2026-09-13 five entries carried a lowercase `fixed`, a triage grep
  for `FIXED|RESOLVED|CLOSED` counted them open, and one was picked up as the next
  task three weeks after it was finished. `scripts/check-docs.py` requires the line
  in every new entry.
* **Any lane may update any entry's status line** without a request: an issue you
  fixed but cannot mark stays open forever in the one place whose job is knowing what
  is open. Everything else about another lane's entry still needs a request
  (`roadmap.md` rule 3).
* **When it is fixed** and the fix has survived a boot test on `main`, move the file
  to `known-issues-resolved/` (`git mv`). Nothing is ever deleted.

## Finding one

    python scripts/docsearch.py "the thing you are looking for" --kind issue --status open
    ls known-issues/TD-B-*            # by id prefix
    grep -rl "some phrase" known-issues known-issues-resolved

The old fence-aware tooling (`ki_split.py`, `ki_archive.py`, `ki_dupes.py`) existed
because one 180,000-line file could not be split, archived or de-duplicated safely by
line; a file per issue needs none of it, and a move is a `git mv`.
