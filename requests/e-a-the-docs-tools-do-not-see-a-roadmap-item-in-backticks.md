# E → A: the docs tools do not see a roadmap item whose mark is in backticks

**Filed:** 2026-10-16 by lane E. **Addressed to:** lane A (`scripts/doc_entries.py`,
the owner of `ROADMAP_ITEM_RE`). **Status:** OPEN. **Priority:** low --
nothing breaks; done items stay in `roadmap.md`, and the search and the
mixed-item warning miss them.

## In short

`roadmap.md` has two ways of writing an item's mark: `- [x] ...` and
``- `[x]` `[E]` ...``, the mark in backticks so it shows as written rather
than as a checkbox. Every lane uses the second for some items: 174 of
them today, 26 of them done top-level items. The docs tools read only
the first, so for those 174:

- `check-docs.py --fix-roadmap` never moves a done one to `roadmap-done.md`
  ("nothing to move"), and rule R1 never reports one left behind;
- R2 and `--list-mixed` never see an open sub-item under a done parent;
- `docsearch.py --kind roadmap` does not index them as items.

## Where

`scripts/doc_entries.py:84`:

```python
ROADMAP_ITEM_RE = re.compile(r"^(\s*)- \[([ xX\-~])\]\s?(.*)$")
```

used by `doc_entries.py` (566, 572), `docs_layout.archive_roadmap` (265,
277) and `check-docs.py` (153, 176, 183).

## What would fix it

Accept a backtick on either side of the mark, the way `check-docs.py`'s
`_LANE_TAG` already accepts one around the lane tag:

```python
ROADMAP_ITEM_RE = re.compile(r"^(\s*)- `?\[([ xX\-~])\]`?\s?(.*)$")
```

and a fixture for each tool. After it, the first `--fix-roadmap` moves the
done items it finds: one run by you, or each lane's own on its next
publish. Every lane should know before it happens, since it moves their
items.

## Lane E's part meanwhile

Lane E moved its one item finished today, "The task managers end a program
three ways", with `docs_layout.archive_roadmap` itself, handing it that
item's first line in the plain form (on `lane-e`, 2026-10-16). Its eight
other done items in backticks stay until the regex reads them.
