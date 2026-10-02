### TD-OILS-CMDSUB-CASE-PATTERN-PAREN. a `case` pattern's `)` inside `$( … )` closes the substitution — ✅ RESOLVED 2026-07-30

**Where:** `userspace/oils/src/lexer.rs` — `read_balanced_inner`, which finds the
end of a substitution by counting parentheses.

**Symptom.** A `case` pattern's `)` has no opening mate, so the count reaches zero
early and the substitution ends in the middle of the `case`:

```
$ bash -c 'x=$(case b in a) echo A;; b) echo B;; esac); echo "x=[$x]"'
x=[B]
$ osh -c '…'
osh: line 1: syntax error near unexpected token `)'
```

This is a common idiom (`x=$(case $y in …) … esac)`), so it matters more than its
neighbours here. It is *not* a here-document problem — it is the other half of "which
`)` closes the construct", and the two are independent: the here-document question is
about where the body *text* comes from (a reader-level question), this one about the
*grammar*.

**Proper fix.** Stop deciding the extent by counting characters. The only thing that
knows which `)` closes a substitution is the grammar, so the scan has to become
token-level and `case`-aware: track a stack of "inside a `case` pattern list" states
in the lexer (as `cond_depth` already does for `[[ … ]]`) and let a `)` in that state
be a pattern terminator rather than a group close. bash gets this for free because
its `$( … )` extent scan (`parse_comsub`) runs the real parser. Doing the same here
would mean the lexer asking the parser where the body ends, which the current layering
(parser depends on lexer) forbids — hence the lexer-side `case` state machine.

**Impact.** Any `case` inside a `$( … )`, `<( … )` or `>( … )` fails to parse. A
`case` inside a plain `( … )` subshell or a function body is fine — those are lexed
inline, with the real token loop.

**Fixed** by the lexer-side state machine the plan called for: `CaseScan` in
`lexer.rs`, consulted by `read_balanced_inner` in its substitution mode. It carries a
stack of `CaseFrame { depth, phase, pat_start }` — nested `case`s can share a depth,
so the frames are a stack and not keyed by it — and `read_subst_body` asks it two
questions at every parenthesis: is this `(` a pattern's *optional open* (which takes
no depth, because the pattern's `)` is what closes it), and does this `)` terminate a
pattern rather than a group.

Answering those needs the reserved words, and a word is reserved only in command
position — `$(printf %s case in f)` is three arguments and its `)` still closes the
substitution — so the scan also tracks the word being read and whether a word
starting there would begin a command. Quoting unmakes a reserved word, so a word with
any quoted character in it is never one (`echo "esac"` does not end a `case`). The
body-terminating tokens are `;;`, `;&` and `;;&`; a bare `;` is not one, which is why
`case b in b) echo B; esac` still ends at its `esac` — that one comes out of the
command-position rule instead.

Measured against bash across 32 shapes before any of it was written (all 32 now
byte-for-byte identical), and the three that were *already* right stayed right. Two
findings worth recording:

- `esac` is reserved wherever a pattern could start, so `case esac in esac) …` is a
  syntax error in bash rather than a match against the word `esac`. Treating it as
  reserved there is what reproduces bash's error — and it also gives an empty
  `case x in esac` for free.
- A `case` still open where the substitution closes is not an error the *scan* should
  raise. bash's parser meets that `)` where it wanted `;;` or `esac` and names it, and
  the failure belongs to the substitution (exit 1) not to the enclosing input
  (exit 2). So the scan appends the `)` to the raw body and lets the body parse name
  it, which lands on both counts.

**Verified:** `tests/corpus/case-in-cmdsub.sh` (39 assertions, byte-for-byte) and
`lexer::tests::substitution_body_ends_at_the_grammars_close_paren` (21 extents plus
the interrupted `case`). One shape is left diverging *on purpose*, logged below as
TD-OILS-CMDSUB-ESAC-PATTERN-BASH-BUG.
