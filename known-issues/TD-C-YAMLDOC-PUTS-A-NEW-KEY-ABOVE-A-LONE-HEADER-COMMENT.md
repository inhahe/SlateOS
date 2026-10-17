### [C] TD-C-YAMLDOC-PUTS-A-NEW-KEY-ABOVE-A-LONE-HEADER-COMMENT -- 2026-09-29

**Status:** OPEN -- `yamldoc` is lane E's now; asked of it 2026-10-16 in
`requests/c-e-yamldoc-puts-a-new-key-above-a-lone-header-comment.md`, which
names the line (`insertion_point`'s top-level answer from `last_real_line`).

**In short:** when a settings file holds nothing but a comment -- a header
such as `# my menus` -- and the first setting is saved into it, the new key
is written *above* the comment instead of under it. Nothing is lost (the
comment is kept, and the file reads back the same), but a header a person
wrote ends up in the middle of the file.

**Where:** `yamldoc` (`Document::set_seq` into a document whose only
content is a comment; found through `servicemenus::ChoicesFile`, whose test
`the_choices_survive_a_save` checks the comment is kept, not where).
`yamldoc` has no lane in the ownership map (`scripts/which-lane.py` gives
`-`), so this is logged rather than changed from lane C.

**The proper fix:** when inserting the first key of a mapping, place it
after any leading comment block that is separated from nothing below it --
i.e. treat a document of only comments as a header -- and add a test that
`# header` then a saved key reads `# header` first.
