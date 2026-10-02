### TD-OILS-REDIR-ERROR-LINE-IS-THE-NEXT-TOKEN. bash reports a nested group's or loop's failing redirect one line late — 2026-08-01 — ⛔ **WONTFIX** (a bash parser artifact, deliberately not replicated)

**Where:** nothing in osh implements it. `userspace/oils/src/interp.rs` —
`Shell::report_redirect_failure` prefixes `err_prefix()`, which names
`current_line`, the line the failing command was actually written on.

A redirection that cannot be applied is reported as `case.sh: line N: TARGET:
MESSAGE`. bash's `N` is not always the line the command sits on. When the
command carrying the failing list is a **group** (`{ …; } >f`) or a **loop**
(`for … done >f`, `while … done >f`), *and* it is nested inside another
compound command, bash names the line of the **token that follows** the
command instead:

```sh
( true
  { echo B; } >/nosuch/dir/f     # line 4
  true )
# bash: line 5;  osh: line 4
```

The one-past applies to the closing token as readily as to a further command —
`( true; { echo B; } >/nosuch/dir/f )` with the `)` on the next line reports
that `)`'s line. It does *not* apply to a **simple** command's list
(`echo B >/nosuch/dir/f`), to a nested `( … )` subshell's list, or to anything
at **top level**, all of which report their own line under both shells. The
enclosing compound may be a subshell, a brace group or a pipeline stage; all
three trigger it. Measured over 10 data points in `target/dvscratch/t3/qb.sh`
and `qc.sh`.

The cause is on bash's parser side: a group/loop command's redirect list is
attached once the terminating token has been read, so the `line_number` the
executor later quotes is that token's, and only a nesting level that has
already advanced the counter makes the difference visible.

osh always reports the line the command was written on, which is what a
`line N:` prefix is for. Replicating the drift would mean carrying a
"line of the next token" through the parser for two command shapes and using
it only when a redirect fails inside a compound — a lot of machinery to
reproduce an off-by-one. The corpus therefore has no case whose expected
output contains a redirect-failure line number for a nested group or loop;
existing cases either use simple commands or filter the prefix off.
