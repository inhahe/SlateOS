### TD-OILS-A-REPRINTED-COMPOUND-COMMAND-IS-KEPT-ON-ONE-LINE. `$(if true; then echo a; fi)` should come back over three lines — 2026-08-07 — ✅ FIXED 2026-08-07 (layout; see the `$LINENO` follow-on below)

**Where:** `userspace/oils/src/unparse.rs` — `program_inline`, which every
substitution body goes through (`comsub_reprint`, and `part_src`'s `ProcSub`
arm).

**What.** bash re-prints a `$( … )` body with its ordinary command printer, the
same one `declare -f` uses for a function body, so a compound command inside a
substitution is laid out over lines exactly as it would be at the top level.
osh renders substitution bodies with `program_inline`, which joins statements
with `; ` on one line. The two agree for everything that is *already* one line;
they diverge for the seven constructs whose printer emits newlines.

**Repro** (`declare -f` of a function whose body is `: $( … )`; bash 5.2.37
left, osh right — everything is one `declare -f`, only the body row is shown):

```text
$(if true; then echo a; fi)      $(if true; then      $(if true; then echo a; fi)
                                 echo a;
                                 fi)
$(for i in a b; do echo $i; done)
                                 $(for i in a b;      one line
                                 do
                                     echo $i;
                                 done)
$(while false; do echo a; done)  $(while false; do    one line
                                     echo a;
                                 done)
$(until false; do echo a; done)  (same shape)         one line
$(select i in a; do echo $i; done)
                                 (same shape as for)  one line
$(case x in x) echo a ;; esac)   $(case x in          one line
                                     x)
                                         echo a
                                     ;;
                                 esac)
$(f() { echo a; })               $(function f ()      $(f () { echo a; })
                                 {
                                     echo a
                                 })
```

Measured to agree already: `{ … }`, `( … )`, a `;`-list, a pipeline,
`[[ … ]] && …`, and `coproc`.

**Where it shows.** Anywhere a substitution body is printed back rather than
run: `declare -f`, `type`, bare `set`, and the text an arithmetic diagnostic
quotes back.

**And it is not only cosmetic.** The re-print is spliced into the text the shell
goes on to read, so its extra newlines are counted:

```sh
q=$(echo $(( 0 + $(if true; then echo 0; fi) )); echo L=$LINENO)
#   written on line 19            bash: L=21          osh: L=19
```

`$LINENO` after a re-printed compound, inside the same body, sits as many lines
lower as the re-print added. Everything else re-parses to the same command.

**Proper fix:** print a substitution body with the same renderer that prints a
function body, instead of `program_inline`. That renderer already exists
(`declare -f` produces exactly bash's layout for all seven constructs above —
the corpus pins it); what is missing is a form of it that returns the body
alone, indented relative to the substitution rather than to column 0, so it can
be spliced inside `$( … )`. The function-definition row wants
`function f () \n{ \n…\n}`, which is what bash's own function printer emits.

**Found by** the shape survey for
TD-OILS-A-REPRINTED-SUBSTITUTION-BODY-LOST-BASHS-LEADING-SPACE-GUARD,
2026-08-07.

**Fixed 2026-08-07 — but by deleting the second printer, not by adding a third.**
The diagnosis above named `program_inline` as the culprit, and it was; what the
proposed fix got wrong is that it treated the one-line renderer as something to
be *replaced at one call site*. It is not a call site problem. **bash has exactly
one command printer** — `make_command_string_internal` (print_cmd.c:182–378) —
and reaches it three ways, each of which only sets a flag first:

| entry point | sets | what it prints |
|---|---|---|
| `print_function_def` / `named_function_string` | `inside_function_def` | `declare -f`, `declare -fx` |
| `print_comsub` | `printing_comsub` | the body spliced back into `$( … )` |
| `make_command_string` | neither | what `jobs` shows |

osh had grown two printers instead: `command_block`/`unparse_function` (the
`inside_function_def` mode) and `command_inline`/`program_inline`/`and_or_inline`
(a hand-written approximation of the other two). The fix removes the second one
outright and parameterises the first with a `Fmt { level, in_func_def, comsub }`
— bash's three globals, passed rather than global. ~130 lines of `command_inline`
and its two helpers are gone.

Two osh-only inventions died with it:

* **`terminate_last: bool`**, threaded through every block renderer, was a lossy
  stand-in for bash's `semicolon()` (print_cmd.c:1512–1521), which is *conditional*:
  it emits `;` unless the last byte printed is `&` or `\n`. That one predicate
  covers backgrounded statements and here-document bodies for free, which is why
  the flag could never be got right by hand.
* **The subshell "strip the first line's indent" hack** (item 2 of
  TD-OILS-DECLAREF-QUIRKS) — bash's `cm_subshell` arm prints `"( "`, bumps
  `skip_this_indent`, and prints the body at the *same* depth, which the new model
  expresses directly.

Two byte divergences fell out of reading the printer rather than from a failing
case, and are fixed in the same pass:

* `print_case_clauses` (print_cmd.c:769) joins patterns with
  `command_print_word_list (clauses->patterns, " | ")` — osh joined with `"|"`,
  so `case x in a|b)` came back as `a|b)` where bash writes `a | b)`.
* `print_for_command_head` (print_cmd.c:605–610) is unconditional
  (`cprintf ("for %s in ", …)`); bash has no wordless case because the *grammar*
  synthesises `map_list` as `"$@"` (parse.y:839–854; `select` at 907–922). osh
  printed `for i;`, bash prints `for i in "$@";` — and for an explicitly empty
  list, `for i in ;`, trailing space and all.

**Correction to the diagnosis above.** It says the missing renderer is one
"indented relative to the substitution rather than to column 0". Measurement says
the opposite: the body is laid out **from column 0**. `print_comsub` runs at
*parse* time, while the printer's `indentation` global is still 0, so a
substitution nested three levels deep still re-prints its body at depth 0. That
is why `Fmt::COMSUB` starts at `Indent::DECLARE` and not at the enclosing depth.

**Verified** byte-for-byte against bash 5.2.37 on 43 hand-built shapes —
if/while/until/for/for-arith/select/case/function/group/subshell/coproc/`[[ ]]`,
`&`-terminated, here-document-terminated, nested, and `&&`-across-a-newline —
each rendered three ways (`declare -f`, `declare -fx`, and inside a `$( … )`),
plus `jobs` on a compound command. Plus the corpus and 1360 unit tests.

**The `$LINENO` half** was a second entry for a few minutes and is fixed too —
see TD-OILS-A-SPLICED-REPRINT-DOES-NOT-MOVE-LINENO below.
