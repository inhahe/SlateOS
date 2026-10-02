## 954. A passing control licenses only the failure it simulates, and the confidence it produces is general

**Date:** 2026-09-21 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** A &middot; **Where:** the method, not a file

**In short:** dd-953 ends by turning dd-942 into a construction rule -- a
corpus you cannot see is checked by including one member whose verdict you
already know. I did exactly that, it passed, and the number it was guarding
was still wrong by a factor of fifteen. The rule is right. What it lacks is
the warning that **a control covers one axis, while the reassurance it gives
covers the whole instrument.**

**The case.** A scan for `unsafe` blocks missing a `// SAFETY:` comment said
2214 blocks, 318 missing, 14.4%.

Before publishing I ran a control, because the scan only looked *backwards*
and a note written as the first line *inside* a block is an ordinary
placement it would miss. The control found 4. The total barely moved, so I
concluded the instrument was sound and wrote that down: *the 318 are
absences, not variations.*

Then I opened a file to start fixing, and the first site had its comment
sitting two lines up, separated from the `unsafe` block by one **safe**
statement -- which a backward walk that stops at the first non-comment line
cannot see. I had been measuring "has a comment immediately above" and
reporting it as "has a SAFETY comment".

| widened to | count |
|---|---|
| comment immediately above | 318 |
| anywhere in the enclosing function | 24 |
| ...excluding `unsafe` inside doc-comment examples | 21 |
| ...without a 40-line cap on "enclosing function" | **8** |

The last cut is the one that stings: the scan *claimed* to search the
enclosing function and capped at 40 lines, while one file carries a comment
governing reads 48 lines below it. 0.36%, not 14.4%. The tree was fine and
the instrument was not.

**Why a passing control made it worse.** Running one is strictly better
than not. But a control that passes is far more persuasive than no control,
and it persuades about the *whole* measurement while being evidence about
*one axis*. Mine varied placement AFTER the block. Every real failure was
placement BEFORE but separated. Planting a comment two lines up with a
statement between would have exposed it in thirty seconds.

**So the construction rule needs a second half:** plant one defect *per
axis the instrument could be blind along*, and enumerate those axes before
planting anything. For a text scan over source they are roughly distance,
direction, intervening content, comment syntax, and whether the match is
code at all.

**Lane C reached the same place from the other side within the hour,** which
is why this is a rule and not an anecdote. Their survey's zero rested on one
planted defect varying one axis. On reading the above they ran three more,
and the informative pair was a frozen field in a type held in a `Vec`
(correctly silent) versus the same field in the same type held singly
(reported). *Correctly excluded* and *blind* are indistinguishable until you
vary the axis that separates them.

**Four more the same day, each a scan answering the question next to mine:**

| the instrument | what it actually matched |
|---|---|
| is this size clamped? | `clip` matched **clippy** in an `allow` attribute, so a syscall looked bounded |
| what size is this allocation? | a `[^)]+` capture stopped at the first `)`, truncating `argv_ptrs.len()` and filing four safe sites as suspicious |
| is this doc link's target unique? | it matched the LAST path segment, so two variants of one enum resolved to two different modules |
| does this applier's anchor resolve? | the text matched 3-4 times, and the duplicates were the sites correctly clamped to 4096 -- a text replace would have converted a safe site and left an unsafe one |

**Every count produced that day shrank when examined:** 47 qualifiable doc
links became 11, 64 demotable became 2, 9 unbounded allocations became 3,
318 missing SAFETY comments became 8. Not one shrank because the tree
improved.

**The operational rule, cheaper than it sounds.** Before a scan's total is
written anywhere, read its *output* -- not its count -- for long enough to
find one entry checkable by hand. Every one of the four above was exposed by
the first or second sample, never by re-reading the regex. A total is a
claim about a population; a sample is the only thing that tests whether the
population is the one you meant.

**Corollary, from breaking a file the same day:** the check that catches an
error must not share machinery with the thing that made it. A script whose
anchors all asserted correctly still wrote a *variable name* into a document
as literal text, because the name sat inside a quoted string. The assertions
were checking anchors; nothing checked that the result was still valid. An
`ast.parse` of the output costs one command and cannot be fooled by the
mistake that produced the file.
