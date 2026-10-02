## B-SUDOERS-LIST-NEGATIONS-WERE-UNREACHABLE (lane B, 2026-09-12) -- FIXED

**In short:** a sudoers rule can carve out an exception --
`alice ALL, !secret = (ALL) ALL` means *every host except `secret`*. Every list
matcher in our `sudo` walked the list forward and returned at the **first**
entry that matched, so `ALL` answered before `!secret` was ever looked at. The
rule applied on `secret`: the one host it had been written to exclude.

**Three matchers, one cause.**

| | was | effect of the bug |
|---|---|---|
| `host_matches` | forward, first match wins; `!` checked last and only against an exact hostname | `ALL, !secret` granted on `secret` |
| `user_matches` | same shape | `ALL, !mallory` granted to `mallory` |
| `runas_matches` | `.any()`, and **no `!` handling at all** | `(ALL, !root)` permitted running things *as root* |

`runas_matches` is the one worth pausing on: it did not merely order the list
wrongly, it read `!root` as the name of a user called `!root`, which matches
nobody -- so the entry was inert and the `ALL` beside it decided everything.

**Real sudo resolves these lists by the LAST match**, which is why the idiom
works there. This file already knew the rule and applied it one level up:
`check_authorization` iterates privileges `.rev()` with the comment *"last
match wins, like real sudo"*. The lists inside those privileges did the
opposite, in the same file, a hundred lines apart.

**Fixed** with one `list_matches(specs, hit)` helper that strips `!`, evaluates
the caller's predicate, and keeps the **last** decisive verdict. Negation lives
in the helper rather than in each caller's predicate, so a matcher cannot
forget it -- which is exactly how `runas_matches` came not to have it.

Proven before the fix: `host_matches(["ALL", "!secret"], "secret")` asserted
**true** in a test written for the purpose. That test is now the same three
lines inverted, plus one that reverses the list and asserts the answer reverses
with it -- which is what "last match wins" means and what first-match-wins
could not express.

**Found by the same question as the three before it** -- *what fact does it
actually establish, and is that the fact the caller needs?* The caller needs
"does this rule apply here"; the function established "does some entry in the
list mention here", which stops being the same thing the moment the list
contains an exception.
