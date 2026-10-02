## 765. A gate that needs one file out of a commit reads the blob, not the tree — the shared seam is `GitTree.read`, and `open_tree` is for gates that need a listing

**Lane:** B
**Date:** 2026-09-04
**Decided by:** Claude (autonomous)

**In short:** Eight checks run when you `git push` and can stop the push. They
used to look at the files sitting in your directory; they were changed one by
one to look at the *commits you are actually sending*, which is not the same
thing if you edited something after committing. There is a shared helper for
doing that. Used the obvious way, the helper first asks git for a list of every
file in the project — about 14,000 of them, roughly four seconds — because most
of these checks want to walk a whole directory. One of them (gate 9, which stops
you deleting a filed request) needs exactly one small file, and usually that
file does not exist at all. The decision is whether it pays the four seconds for
the sake of every check looking the same, or uses the cheaper half of the same
helper. It uses the cheaper half.

**The two things that were weighed.**

| | `open_tree(root, head)` | `GitTree.read(head, path)` |
|---|---|---|
| *What changes:* | every converted checker opens its tree the same way, and the shape is copyable without thought | gate 9 keeps the push about as fast as it was; the reader has to notice why this one is spelled differently |
| Cost per push | one `git ls-tree -r` of the whole repository — 13,821 paths, ~3.8 s measured | one `git cat-file --batch` request, for a path that is absent on almost every push |
| What it buys | listings, `is_file`, `files_under` — none of which gate 9 asks for | nothing beyond the blob |

**Why the cheaper half won.** Uniformity is worth paying for when it buys the
reader something, and here it would buy an index that is discarded unread. The
seam's actual promise is that a checker asks git rather than the disk; both of
these keep it. `RevTree`'s docstring already says why it builds the index — "a
checker asks about several prefixes and the per-call cost of `ls-tree` is the
whole cost" — and that reasoning inverts exactly when the checker asks about one
path. Four seconds on every push, by every lane, to answer a question that is
usually "the file is not there" is the kind of cost that gets a gate switched
off, and this project has already argued (§763, and `run-checker.sh`'s whole
premise) that a gate people route around protects nothing.

**Why it is not simply "use whichever is faster".** The one real argument for
`open_tree` is that the next person converting a gate copies the last one, and a
gate spelled differently invites a wrong copy in either direction — someone
using `GitTree.read` in a loop over a whole crate would reintroduce the
per-file cost the seam exists to remove (measured at ~27 minutes for `posix/src`
before batching). So the choice is only defensible with the rule written down
next to it, which is what this entry is: **`open_tree` when you need to know
what is in a tree; `GitTree.read` when you already know the path and want the
bytes.** Gate 9 is currently the only gate on the second side of that line.

**What it does not decide.** Nothing about correctness — both readers answer
from the commit, and the defect that prompted this (gate 9 reading its waiver
list off the disk while reading its deletions from the commit) is fixed either
way. This is a cost decision layered on top of a correctness fix, and if gate 9
ever needs a listing it should switch to `open_tree` without ceremony.
