## 841. The backup tool learns character classes; the split with search closes

**Date:** 2026-09-13. **Lane:** C. **Decided by:** Claude (autonomous) --
put to the operator as C-Q9 on 2026-09-06; they replied on 2026-09-07 with no
preference but two questions, both of which are answered with measurements
below. Supersedes the second half of §555.

**In short:** when you tell the backup program which folders to skip, you type
a pattern like `*.tmp`. When you search for a file you type one that looks the
same. Until today they were not the same: the search tools read `[a-z]` as "any
lowercase letter" and the backup tool read it as "the six characters `[`, `a`,
`-`, `z`, `]`". The backup tool now agrees with the search tools. The cost is
that an exclude line somebody already wrote as `cache[1]/` stops skipping a
folder literally called `cache[1]` and starts skipping one called `cache1`.

**What §555 decided, and why it is now wrong.** §555 recorded the difference as
two legitimate dialects: the search tools implement glob, the backup tool
implements gitignore, and those are different languages. The first half of that
-- merging the two search matchers, which disagreed on 646 of 730 236 patterns
-- stands. The second half does not, because the premise is false:

| tool | family | what `[a-z]` means |
|---|---|---|
| `.gitignore` | exclude list | a character class |
| `rsync --exclude` | exclude list | a character class |
| `tar --exclude` | exclude list | a character class |
| `posix::fnmatch` in this tree | glob | a character class |
| `apps/backup` before today | exclude list | five literal characters |

Character classes are not a glob feature that gitignore lacks. They are in both
families, and `apps/backup` was the only matcher in the tree without them. It
was not speaking a second dialect; it was missing a feature of the one it
documents itself as implementing.

**The operator's two questions, answered.**

*(A) What is normal?* The table above. This is the answer that reverses the
recommendation, and it was got wrong the first time by reasoning about what
gitignore is for rather than reading what it does.

*(B) How likely is a user to really benefit?* Zero patterns in this tree's
shipped defaults, fixtures or YAML use a bracket. That number cuts both ways
and is the most useful fact available: the feature is rarely reached for, which
is what the operator suspected -- and **nothing that exists changes meaning**,
which removes the entire argument for leaving the split alone. The operator
also confirmed they have no rules of their own using `[]`.

**The alternative that was rejected outright.** The operator floated removing
character classes from search and indexing instead, so the three agree the
other way. That moves the OS away from both norms *and* from its own
`posix::fnmatch`, to make two programs agree by making both unusual. It also
removes a working feature from a tool where it demonstrably belongs.

**What was deliberately not changed.** Three other differences between the two
matchers stay exactly as they are, because an exclude list matches paths and a
search matches text:

| | search (`apps/globmatch`) | backup (`apps/backup`) |
|---|---|---|
| `*` | crosses `/` | stops at `/` |
| `**` | nothing special | spans directories |
| `?` | any character | any character except `/` |

Only the character-class row was ever in dispute, and only it moved.

**What it cost, recorded because a silent meaning change is the worst kind.**
`a_bracket_in_an_old_exclude_line_now_means_a_class` pins the break next to the
feature: a backup that quietly starts *including* a directory it used to skip
produces no error anybody sees, and the only defence against that is having
written it down where the next reader will be standing.
