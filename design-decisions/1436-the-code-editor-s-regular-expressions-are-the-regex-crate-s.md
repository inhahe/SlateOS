## 1436. The code editor's regular expressions are the `regex` crate's

**Date:** 2026-09-28 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C

**In short:** The code editor's find and replace can search for a pattern, not
only for text -- "any word followed by a digit", say -- and put parts of what
it found into the replacement. That needs a regular-expression engine. The
toolkit now uses the one nearly every Rust program uses, the `regex` crate,
rather than one written here. Only programs that search with it carry its code.

**The alternatives:**

| Option | For | Against |
|---|---|---|
| **The `regex` crate** (chosen) | the engine upstream reviews, tests and fuzzes; linear-time matching, so no pattern can hang the editor; Unicode classes; `$1`/`${name}` replacements built in | four packages more in every build of the toolkit (`regex`, `regex-automata`, `regex-syntax`, `aho-corasick`); its code is linked only into programs that call it |
| `regex-lite` | one package, smaller | fewer Unicode classes and slower -- for an editor searching source in any language, the full engine's classes are what "a letter" should mean |
| An engine of our own | no dependency | "ours" is what §539 says not to trust over reviewed upstream code; a backtracking engine written quickly is how an editor hangs on a pathological pattern |
| Lift `apps/regextester`'s Pike VM | already in the tree | it is lane E's application code, not a library; smaller syntax and less tested than the crate |

**Bounded.** A compiled pattern is limited to 4 MiB (`REGEX_SIZE_LIMIT`), so a
pathological one costs an error, not the machine.

**Plain text stays `textfind`'s** -- the tree's one substring search, whose
case folding keeps every offset in the text searched (its module docs record
the three bugs eight copies of it shared). A match of nothing is never a
match, in either mode.
