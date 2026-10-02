### TD-OILS-A-MISSING-TERNARY-COLON-BLAMES-THE-WHOLE-BRANCH. `let '1 ? 2+3'` names `2+3` where bash names `3` — 2026-08-09 — ✅ FIXED 2026-08-09

**Where:** `userspace/oils/src/arith.rs`, `AParser::parse_ternary` — a
`let then_start = self.pos;` taken before the middle branch and handed to
``ArithError::with_token("`:' expected for conditional expression", …)`` when
the `:` fails to appear.

**Reproduce.**

```sh
let '1 ? 2+3'
let '1 ? (2+3)'
let '1 ? 2, 3'
let 'x=1' 'x ? x=5'
```

| | bash 5.2.37 | osh (before) |
|---|---|---|
| `1 ? 2+3` | `3` | `2+3` |
| `1 ? 2+3*4` | `4` | `2+3*4` |
| `1 ? (2+3)` | `)` | `(2+3)` |
| `1 ? (2)+3` | `3` | `(2)+3` |
| `1 ? 2, 3` | `3` | `2, 3` |
| `1 ? -2` | `2` | `-2` |
| `x ? x=5` | `5` | `x=5` |
| `0 ? 2+3` | `3` | `2+3` |

Six of eight measured shapes differed. The two that agreed — `1 ? 2` and
`1 ? a[1]` — did so only because a one-token branch has the same first and last
token, which is why the single existing unit test never caught it.

**Why.** bash's `evalerror` prints `lasttp` (expr.c:1517-1530), the token its
*lexer* is holding, and after the middle branch that is the branch's **last**
token, not its first. Nothing about the branch as a whole is ever recorded.
osh already tracks exactly that pointer — `AParser::last_tok_start`, added for
the missing-`)` diagnostic — so the branch-start variable was simply the wrong
one to reach for.

**Fixed** by reporting `self.rest_from(self.last_tok_start)` and deleting
`then_start`. All eight shapes above plus nine more now match byte-for-byte.

One shape still differs, and belongs to the entry below rather than here:
`(( 1 ? 2 3 ))` names `3` in bash and `2 3` in osh, because there the held
token is a *leftover* the parse never consumed — bash's one-token lookahead.
