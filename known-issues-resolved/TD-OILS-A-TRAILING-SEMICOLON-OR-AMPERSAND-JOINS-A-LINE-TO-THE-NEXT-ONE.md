### TD-OILS-A-TRAILING-SEMICOLON-OR-AMPERSAND-JOINS-A-LINE-TO-THE-NEXT-ONE. `alias foo=…;` on its own line is invisible to the `foo` beneath it — 2026-08-06 — ✅ FIXED 2026-08-06

**Where:** `userspace/oils/src/parser.rs`, `IncrementalParser::next_unit` — the
arm that decides what the separator `parse_item` just consumed means.

**What.** The reader takes one *line* at a time; a `;` or `&` written last on a
line separates the command before it from the newline, not from the line
underneath. `next_unit` treated the two operators as "the unit continues" in
every position, so after a trailing one it called `parse_item` again, which
stepped over the newline and pulled the next line into the same unit.

Everything keyed to the reader's granularity shifted by a line:

```text
$ shopt -s expand_aliases; alias zzfoo='echo A';⏎ zzfoo
bash: A
osh : zzfoo: command not found   # both lines parsed before the alias existed

$ sleep 0.05 & sleep 0.5⏎ jobs;⏎ jobs %1
bash: jobs: %1: no such job      # swept at the boundary between the two
osh : [1]+ Done sleep 0.05       # no boundary, so no sweep

$ set -v; echo one;⏎ echo two
bash: echo one; / one / echo two / two
osh : echo one; / echo two / one / two    # both echoed, then both run
```

…and so did `\#`, which counts the reader's units — the measurement that turned
this up.

**Fix (2026-08-06).** After a `;`/`&` separator, `next_unit` looks at the token
following it: a newline there is consumed and ends the unit. Within a line the
two operators still chain, which is what the same arm did before and what the
case below pins alongside.

**Found by** measuring bash's `\#` command number in order to implement it:
every osh number was one short per `&` in the script, and the missing increments
turned out to be units the reader had never separated.

**Tests.**
`tests/corpus/a-trailing-separator-does-not-carry-a-unit-onto-the-next-line.sh`
— the alias, the job-table sweep and the `set -v` echo, each with its
written-on-one-line counterpart. All three diverge without the fix.
