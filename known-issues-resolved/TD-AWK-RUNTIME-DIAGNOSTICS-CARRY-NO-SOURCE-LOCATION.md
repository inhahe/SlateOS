## TD-AWK-RUNTIME-DIAGNOSTICS-CARRY-NO-SOURCE-LOCATION (lane B, 2026-08-24) — **FIXED** 2026-10-01

**In short:** when an `awk` program dies partway through — a division by zero,
say — we print `awk: fatal: division by zero attempted` and stop. gawk prints
`awk: cmd. line:1: fatal: division by zero attempted`, naming the line of the
user's script that blew up. On a one-liner the difference is cosmetic; on a
200-line `-f` script it is the difference between a fixable error and a hunt.

**Where.** `userspace/coreutils/src/bin/awk/`. The gap is structural, not a
missing `format!`: **nothing in the front end records a source position at
all.** `lex.rs` tracks newlines only for their grammatical role (a newline
terminates a statement), never as a counter; `ast.rs` has no line field on any
node; so `interp.rs` has nothing to report even if it wanted to. The two
`Fatal(...)` sites for division and modulo in `interp.rs` are representative —
every runtime diagnostic in the file is in the same position.

**Reproduce.**

```
$ awk 'BEGIN {x = 0; print 1 % x}'
awk: fatal: division by zero attempted in `%'          # ours
awk: cmd. line:1: fatal: division by zero attempted in `%'   # gawk
```

`scripts/awk-diff.sh` records both of these as xfails with this entry's reason;
they will XPASS (and so fail the harness) the moment the locations land, which
is the intended prompt to convert them back to `msg_case`.

**What the target format is.** gawk's runtime diagnostic is

```
awk: <source>:<line>: [(FILENAME=<f> FNR=<n>) ]<severity>: <message>
```

where `<source>` is the literal `cmd. line` for a program given in argv or with
`-e`, and the file name for one given with `-f`; the parenthesised input
position is present only when a *main rule* is executing (not in `BEGIN`/`END`);
`<severity>` is `fatal`, `error` or `warning`. Note that gawk emits it only once
a rule has been entered — see the deliberate divergence already recorded in
`awk-diff.sh` for the unopenable-second-operand case, where gawk's prefix is
*stale* state pointing at a line unrelated to the failure. We should not
reproduce that half: a location we print should be the location that failed.

**Proper fix.** Thread a position through the front end: give `lex.rs` a line
counter and attach it to each token, carry it into the statement and expression
nodes in `ast.rs` that can fail at runtime, and have `interp.rs` hold a "current
statement" position that its `Fatal` constructor reads. The source name is
already known at that point — `main.rs` distinguishes an argv/`-e` program from
a `-f` one when it assembles the text, and needs to keep that distinction rather
than discarding it after concatenation. This is a tranche of its own, not a
patch.

**Severity.** Low for correctness — the exit status, the message text and
everything already written to stdout all match gawk exactly. Medium for
usability on long scripts, which is the case that most needs it.

**Progress (2026-10-01).** Statements and rules now carry the line they begin
on (`ast::Loc`, `source.rs` maps an offset to `cmd. line:N` or `file:N` per
`-f` file), every runtime diagnostic is placed as gawk's `err()` places it,
and parse-time warnings and fatals are placed too. `awk-diff.sh` gained 60
placement rows, all agreeing; the two xfails above are `msg_case` again. Two
claims above were measured wrong and are corrected here: the `(FILENAME=…
FNR=…)` part is present whenever gawk's integer `FNR` is above 0 — END after
input has it, a rule that set `FNR = 0.5` does not — not "only while a main
rule runs"; and the unopenable-second-operand location is not stale state to
avoid but the same "line of the last instruction executed" (`interpret.h`
sets `sourceline` from every instruction) that places everything else, so it
agrees now rather than being special-cased. **Left:** gawk's line is the
*operator's*, not the statement's — `if (1 &&\n 1/z)` is line 2's `/`, and
so is `print 1,\n 1/z` — so a statement that spans lines can still be
placed on its first line where gawk names a later one. That needs a location
on every expression node, which is the next change.

**Status: FIXED 2026-10-01.** Every expression node now carries its token's
line, and the program is compiled to instructions that each carry one, run by
a loop that updates the current line at every instruction as gawk's
`interpret.h` does (`design-decisions.md` §1056) -- so placement is gawk's
for multi-line statements too. `awk-diff.sh` has the multi-line rows.

#### TD-A-SED-KEPT-TWO-COPIES-OF-ITS-TRANSFORM-LOOP (lane A, 2026-08-25) — ✅ FIXED (`5e523d20a`)

**In short:** `sed` had its line-editing loop written out twice — once for
`sed script file` and once for `cat file | sed script`. The two copies drifted,
and by the time this was written they disagreed about the newline at the end of
the output: the file form printed one, the pipe form did not. So the same script
over the same bytes produced a different number of bytes depending on which end
the text came in from, and `cat f | sed 's/a/b/' | wc -l` counted one line short.

**Where.** `kernel/src/kshell.rs`, `cmd_sed` and `cmd_sed_input`. Both ended:

```rust
if output.ends_with('\n') { output.pop(); }
shell_println!("{}", output);   // cmd_sed      -- puts one back
shell_print!("{}", output);     // cmd_sed_input -- leaves it off
```

**This is the third drift in the same pair, which is why the fix is structural
rather than another patch.**

| When | What had drifted | How it was fixed |
|---|---|---|
| earlier | the pipe copy had lost the `-i` arm, so `cat f \| sed -i 's/a/b/'` filtered the pipe, left `f` untouched, and exited 0 | `classify_sed_args` extracted |
| earlier | `filter_map` dropped unparseable `-e` expressions and ran the rest | `parse_sed_scripts` extracted; the fix had to be applied to *both* copies |
| this one | the trailing newline | `sed_apply` extracted |

Each time, the shared *parser* was factored out and the shared *loop* was left
duplicated — so the next divergence landed in the part that was still copied.

**Two more faults fell out of writing the single copy.**

*Empty output printed a blank line.* `sed '/./d' f` deletes every line, so
`output` is empty; `pop()` on an empty string does nothing and `shell_println!`
then wrote a newline. A script that removes all the text emitted a line.

*A file with no final newline gained one.* `str::lines` cannot tell `"a\nb"`
from `"a\nb\n"`, so a loop that appends `\n` after every kept line invents one.
GNU `sed` writes such a file back without a final newline. The rule is not "pop
the last newline" — that gets `printf 'a\nb' | sed '2d'` wrong, where the
surviving line `a` did come with a newline and keeps it. It is: a newline
follows every emitted line *except* the last line of an input that did not end
in one, which is a property of the input rather than of what survived.

**Fixed by** `sed_apply(text, script, suppress) -> String`, called by both
halves, with both printing it verbatim via `shell_print!`. Covered by
self-test rung 38, whose assertions compare whole captured byte strings rather
than searching them — the two halves already agreed on *which lines* appear, so
only a byte comparison can see this class of divergence. Rung 38 also asserts
that `-i` writes the same bytes the terminal would have shown, which was a third
spelling of the output under the old code (it wrote the pre-`pop` string to the
file and the post-`pop` string to the terminal).

**Still duplicated, same shape, not yet fixed:** `cmd_awk`/`cmd_awk_input` each
carry their own copy of the BEGIN/record/END driver, and
`cmd_mapfile`/`cmd_mapfile_input` each carry their own copy of the `-t` flag
scan. Both are the same accident waiting for its first divergence.
