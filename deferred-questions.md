<!-- docs-layout: signpost -->
# Deferred questions — moved

This file used to hold every entry in one document. Since 2026-10-02 each entry is
its own file:

* `deferred-questions/<ID>.md` -- one deferred question per file
* `deferred-questions/README.md` -- the rules

Find an entry by id, by section number or by meaning:

    python scripts/docsearch.py DQ4

Citations of the form `deferred-questions.md DQ4` throughout the tree still identify the entry: the
id (or number) is the file name. The old single file is in git history at the
commit before the cutover (`git log --diff-filter=D -1 -- deferred-questions.md` finds it).
