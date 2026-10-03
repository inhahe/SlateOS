### TD-OILS-CMDSUB-ABORT-LINENO. bash inflates the reported line number for a special-builtin usage error inside `$( )`; osh reports the true line — 2026-07-28 — ✅ RESOLVED 2026-08-08 (osh already agrees; the recorded scope *and* mechanism were both wrong)

**Resolved.** osh has matched bash here for some time — both shells report
`line 4` for the reproducer below. What this entry got wrong is *why*, and the
correction matters because the entry used its (wrong) diagnosis to decline the
fix and to forbid a corpus case.

The inflated number has nothing to do with the abort, the error class, or the
builtin being special. It is where `$LINENO` reads from inside a `$( )` body,
for *every* command in that body:

| probe on line 2 of a 2-line file | bash |
|---|---|
| `x=$(echo "L $LINENO")` | `L 2` |
| `x=$(for i in 1; do echo "L $LINENO"; done)` | `L 4` |
| `x=$(while true; do echo "L $LINENO"; break; done)` | `L 3` |
| `x=$(break 1 2)` | `line 2` |
| `x=$(shift a b; echo body)` | `line 2` |
| `x=$(for i in 1; do break 1 2; done)` | `line 4` |
| `x=$(while true; do break 1 2; done)` | `line 3` |

So the second ingredient is a **loop** — anything whose re-print gains lines —
not a command-substitution frame *per se*, and the first ingredient does not
exist at all: `$LINENO` and the abort read the same number from the same spot.
The two abort shapes with no loop (`break 1 2`, `shift a b`) report the true
line, which is why the original three-line reproducer looked error-class-specific
— it happened to wrap its `break` in a `for`.

**The actual mechanism.** bash does not keep a `$( )` body's source text. At
*parse* time it re-prints the parsed command and keeps the print:

```c
tcmd = print_comsub (parsed_command);   /* returns static memory */
                                          /* parse.y:4219, in parse_comsub */
ret = make_command_string (command);      /* print_cmd.c:170, print_comsub */
```

and the printer breaks a loop across lines the source never had.
`print_for_command` emits `cprintf (";"); newline ("do\n");`
(print_cmd.c:627) so a `for` body lands on printed line 3, while
`print_until_or_while` emits `semicolon (); cprintf (" do\n");` —
still carrying the comment `/* was newline ("do\n"); */` — at
print_cmd.c:811, so a `while`/`until` body lands on printed line 2. Printed
line 1 is the line the substitution *closes* on. A backtick body is echoed
verbatim rather than re-printed, so it moves nothing. Nesting composes by more
than the sum, because the inner print is embedded in the outer one and parsing
the outer walks the counter to the outer print's last line first: `$(echo
$(for … ))` shifts 3 + 2 and `$(echo $(while … ))` shifts 2 + 1.

The entry's own guess — "seeded from the caller and then advanced again by the
body's own parse" — is half right (the seed) and half wrong (nothing advances
it a second time; the body simply *has* more lines than it was written with).

**Fixed in `HEAD`.**

- `tests/corpus/an-abort-inside-a-substitution-is-blamed-on-the-reprinted-bodys-line.sh`
  covers it. Every section pairs a `$LINENO` probe with an abort probe over the
  same body shape, so the two numbers being equal *is* the assertion; `for`,
  `while`, `until`, backtick, multi-line source, both nestings and the caller's
  own counter are all rows.
- **The `# EXPECT-DIFF:` instruction below is retracted.** There is no
  divergence to waive; the command-substitution abort shape is now in the corpus
  with no waiver at all.

<details><summary>Original entry (diagnosis superseded)</summary>

**Symptom.** A three-line script whose middle line is a command substitution
containing a `break`/`continue` usage error:

```
echo start
x=$(for i in 1; do break 1 2; done; echo body)
echo "cs=[$x] rc=$?"
```

```
$ bash ta.sh          $ osh ta.sh
start                 start
ta.sh: line 4: ...    ta.sh: line 2: ...
cs=[] rc=1            cs=[] rc=1
```

bash blames **line 4** of a file that has only three lines; osh blames line 2,
where the error actually is. Everything else — the message text, the empty
capture, the caller's `rc=1`, the fact that the caller survives — matches
byte-for-byte.

**Scope: it is specific to this one error class inside a command
substitution.** The same abort at top level (no `$( )`) agrees on line 2 in
both shells, and a *different* error inside the same command substitution — an
arithmetic `$((1/0))` — also agrees on line 2 in both. So the divergence needs
both ingredients: a special-builtin usage error (the `jump_to_top_level(DISCARD)`
that runs `top_level_cleanup()` first, see BUG-OILS-EVAL-DISCARD-SCOPE) *and* a
command-substitution frame around it.

**Where.** `userspace\oils\src\interp.rs` — the line recorded in
`Shell::err_prefix()` when the abort is raised inside `command_subst`. Nothing
in osh is wrong here; the entry exists so a future corpus case that trips over
it is recognised rather than "fixed" into agreement.

**Proper fix.** Almost certainly none. bash's number comes from its
command-substitution body being handed to a fresh `parse_and_execute` whose line
counter is seeded from the caller and then advanced again by the body's own
parse — an implementation artifact, not a behaviour, and reproducing it would
mean deliberately reporting a line that does not exist. Left as-is deliberately,
in the same spirit as TD-OILS-NAMEREF-WARNING-COUNT above. The consequence is
that `tests/corpus/eval-discard-scope.sh` deliberately omits the
command-substitution shape even though osh handles it correctly; if that case is
ever added it must carry an `# EXPECT-DIFF:` waiver pointing here.

</details>
