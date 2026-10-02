### TD-OILS-AN-ASSIGNMENT-PREFIX-STILL-ADMITS-A-RESERVED-WORD. `v=1 done` is a syntax error where bash runs a command — 2026-08-08 — ✅ FIXED 2026-08-08

**Where:** `userspace/oils/src/parser.rs` — `Parser::parse_simple`'s word loop
and `Parser::parse_coproc`'s name.

**What.** bash recognises a reserved word only where the token before it is one
of a fixed list — a separator, an operator, `{`, `}`, `!`, `time`, `coproc`, or
another reserved word (parse.y, `reserved_word_acceptable`). A plain `WORD` is
not on that list, and an assignment prefix *is* a plain `WORD`. So after one, a
reserved word is an ordinary command name:

```text
input            bash                                osh
v=1 done         line 1: done: command not found     syntax error near `done'
v=1 then         line 1: then: command not found     syntax error near `then'
v=1 fi           …: fi: command not found            syntax error near `fi'
v=1 do           …: do: command not found            syntax error near `do'
v=1 esac         …: esac: command not found          syntax error near `esac'
v=1 in           …: in: command not found            syntax error near `in'
v=1 {            …: {: command not found             syntax error near `{'
v=1 }            …: }: command not found             syntax error near `}'
v=1 !            …: !: command not found             syntax error near `!'
```

`if`, `while` and the other compound openers are the same rule seen from the
other side: `v=1 if true; then echo y; fi` runs a command called `if` with the
argument `true`, and bash's error is then about the `then` that follows a `;` —
`syntax error near unexpected token 'then'` — where osh blames the `if`.

**Fixed**, by *deleting* the second place `RESERVED` was consulted rather than
adding a predicate. `parse_simple`'s word loop had its own reserved-word break,
guarded by "no command word yet", which fired exactly where bash's answer is no:
after an assignment prefix or a redirection's filename. The loop's *first*
iteration never needed it — `parse_command` has already dispatched or rejected
any reserved word standing there — so removing the break leaves `RESERVED` tested
at the single place `Parser::reserved_here`, and every call site of that is a
position where bash's predicate holds. That is the shape the predicate was going
to be bolted onto; with the duplicate gone there is nothing left to bolt it to.

The one position that did need a *new* test is `coproc`'s name: `COPROC` is on
`reserved_word_acceptable`'s list, so the word after it is looked up like any
other, and bash's production is `COPROC WORD compound_command`. `coproc done {
echo y; }` is `syntax error near `done'`, not a coproc named `done`.
`Parser::parse_coproc` now refuses a `reserved_here()` word as the name.

Which openers are *absent* from bash's list is what makes the rest come out
right: `for`, `case`, `select` and `function` are not on it, so `for done in a`
names a variable, `case done in done)` matches, and `function done { …; }`
defines a function — all of which already worked and still do.

**The echoed-line slice went with it.** The `` `v=1cho done' `` echo was a
consequence, not a separate bug: osh was reporting an error on a line it had no
business erroring on, and slicing the joined line to find it. Now that `v=1\⏎echo
done` parses, the probes that *do* land an error on a continuation-joined line
(`echo a\⏎b ;;`, `v=1\⏎echo done ;;`, `ec\⏎ho a ;;`, `echo \⏎a\b ;;`) all echo
byte-for-byte what bash echoes.

**Pinned by**
`a_prefix_spends_the_position_where_a_reserved_word_would_be_acceptable`
(parser.rs) and the corpus case
`a-prefix-spends-the-position-where-a-reserved-word-would-be-acceptable.sh`,
which walks all eighteen reserved words after an assignment and after a
redirection, the four compounds whose error moves to the next acceptable slot,
eight positions where a reserved word *is* acceptable, the `coproc` name, and the
openers absent from bash's list.

**Impact was** a syntax error where bash runs a command, so a script that reached
one of these lines died at parse time instead of at that line. Found while
building the corpus case for
TD-OILS-AN-ESCAPED-BACKSLASH-BEFORE-A-NEWLINE-WAS-READ-AS-A-CONTINUATION, whose
`v=1\⏎echo done` probe joins into exactly this shape.
