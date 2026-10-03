<!-- docs-layout: signpost -->
# Design decisions — moved

This file used to hold every entry in one document. Since 2026-10-02 each entry is
its own file:

* `design-decisions/NNNN-<slug>.md` -- one decision per file, by section number
* `design-decisions/README.md` -- the format, and the numbering-band table

Find an entry by id, by section number or by meaning:

    python scripts/docsearch.py 538 --kind decision

Citations of the form `design-decisions.md §538` throughout the tree still identify the entry: the
id (or number) is the file name. The old single file is in git history at the
commit before the cutover (`git log --diff-filter=D -1 -- design-decisions.md` finds it).
