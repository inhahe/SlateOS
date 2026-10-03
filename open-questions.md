<!-- docs-layout: signpost -->
# Open questions — moved

This file used to hold every entry in one document. Since 2026-10-02 each entry is
its own file:

* `open-questions/<ID>.md` -- the operator's decision queue, one question per file
* `open-questions-resolved/<lane>.md` -- the one-line records of answered questions
* `open-questions/README.md` -- how to write a question the operator can decide

Find an entry by id, by section number or by meaning:

    python scripts/docsearch.py A-Q14

Citations of the form `open-questions.md A-Q14` throughout the tree still identify the entry: the
id (or number) is the file name. The old single file is in git history at the
commit before the cutover (`git log --diff-filter=D -1 -- open-questions.md` finds it).
