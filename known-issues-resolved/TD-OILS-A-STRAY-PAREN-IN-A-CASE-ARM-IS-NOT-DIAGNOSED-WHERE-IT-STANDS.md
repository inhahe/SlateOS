### TD-OILS-A-STRAY-PAREN-IN-A-CASE-ARM-IS-NOT-DIAGNOSED-WHERE-IT-STANDS. `case a in a) echo x( y;; esac` is blamed on the `;;` — 2026-08-06 — ✅ FIXED 2026-08-07

**Where:** `userspace/oils/src/parser.rs` — `parse_case_body_inner`.

**What.** A bare `(` after a command word is a syntax error at the `(` in every
context bash has — except that osh gets the `case` arm wrong, and only the
`case` arm:

```
                                osh                     bash
echo x( y                       near `('  line 1        near `('  line 1   ✔
{ echo x( y; }                  near `('  line 1        near `('  line 1   ✔
case a in a) echo x( y;; esac   near `;;' line 1        near `('  line 1   ✘
case a in a) echo x) y;; esac   near `)'  line 1        near `)'  line 1   ✔
```

Nothing about here-documents is involved — a multi-line arm reports the `;;`
line, which is how this was found (a `case`-arm probe was dropped from
`tests/corpus/a-pending-here-document-is-gathered-before-the-token-that-ends-the-list-is-blamed.sh`
for it).

**Why.** Not paren counting at all — the arm's body parser simply had no
*abutment* check. `parse_item`, which drives every other command list, refuses a
list that carries no separator unless what follows terminates the context
(`had_sep` → `at_terminator`, parser.rs:2605). `parse_case_body_inner` ran the
same `& / newline / ;` match but ended it with a bare `_ => {}`, so an
unaccounted token fell through and the loop simply re-entered. `( y;; esac` then
parsed as a *subshell*, and the error surfaced where that subshell broke — on
the `;;`. The `)` twin looked right only by accident: `)` cannot begin a command,
so the re-entered `parse_and_or` failed on it immediately.

That same hole hid a second, quieter bug the entry never named: when the abutting
token begins a *complete* command the re-entry succeeds, and the arm silently
takes two commands where bash takes none —

```
case a in a) ( : ) ( : ) ;; esac      bash: near `('   osh: accepted, rc=0  ✘
case a in a) { :; } { :; } ;; esac    bash: near `{'   osh: accepted, rc=0  ✘
```

**As fixed.** `parse_case_body_inner` now carries `parse_item`'s `had_sep` and
rejects a separator-less body unless it stands at one of the arm's own enders.
The ender set comes from bash's grammar rather than from taste: the arm is
`pattern ')' compound_list`, and `compound_list: newline_list list0 |
newline_list list1` — the `list1` alternative is what lets a body end with no
trailing `;`/`&`/newline at all, so `case a in a) { echo x; } esac` really does
parse. What may follow such a bare body is then fixed by the productions that
*receive* the arm: `;;`, `;&` and `;;&` from `case_clause_sequence`, and `esac`
from `case_clause`. Nothing else reduces, so nothing else is accepted.

`esac` is asked for through `reserved_here`, which already knows a reserved word
counts only in command position. That is load-bearing: after a simple command's
word `esac` is an ordinary *argument*, which is why bash answers
`case a in a) echo x esac` with `unexpected end of file` (the `case` runs off the
input) instead of closing the arm — and osh now matches that too.

**Pinned by** `parser.rs::case_arm_body_ends_like_any_other_list` (the eight
rejections including both silently-accepted shapes, and nine acceptances
covering each of the four enders after a bare body) and
`tests/corpus/a-case-arm-body-ends-where-any-other-list-does.sh`.
