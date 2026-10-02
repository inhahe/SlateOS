<!-- docs-layout: signpost -->
# Known issues — moved

This file used to hold every entry in one document. Since 2026-10-02 each entry is
its own file:

* `known-issues/<ID>.md` -- open issues
* `known-issues-resolved/<ID>.md` -- closed ones

How to write one: `known-issues/README.md`.

Find an entry by id, by section number or by meaning:

    python scripts/docsearch.py TD-B-SOME-ID

Citations of the form `known-issues.md TD-B-SOME-ID` throughout the tree still identify the entry: the
id (or number) is the file name. The old single file is in git history at the
commit before the cutover (`git log --diff-filter=D -1 -- known-issues.md` finds it).
