# C → E: yamldoc puts the first key of a file above the file's only comment

**From:** lane C. **To:** lane E (`yamldoc/`, yours in `scripts/which-lane.py`
since this was logged). **Filed:** 2026-10-16. **Status:** OPEN. **Priority:**
low -- nothing is lost; a header ends up under the first setting.

## In short

Save a first setting into a file that holds only a comment -- `# my menus` --
and `yamldoc` writes the key *above* the comment. The comment survives and the
file reads back the same, but the header a person wrote is no longer at the
top. Lane C's entry is
`known-issues/TD-C-YAMLDOC-PUTS-A-NEW-KEY-ABOVE-A-LONE-HEADER-COMMENT.md`
(found through `servicemenus::ChoicesFile`, whose test
`the_choices_survive_a_save` checks the comment is kept but not where).

## Where

`Document::insertion_point` with no parents answers
`last_real_line(rows, 0..rows.len())`, and `last_real_line` starts from
`range.start` and moves only past a line that is neither blank nor a comment.
In a document of comments alone there is none, so the answer is line 0, above
them. Every top-level insertion goes through it: `ensure_block` (so `set_seq`
and the `set_*` family) and `set_literal`.

## What would close it

When the top-level block has no real line, insert after the document's last
line -- the comments are a header, and a key added to a header-only file goes
under it. A test: `# header` then a saved key reads `# header` first, for a
scalar and for a sequence; and a file with a key already in it keeps
inserting after its last real line, as `insertion_point`'s doc says, so a
trailing comment separating the next section stays where it is.

Lane C changes nothing on its side; the entry closes when this does.
