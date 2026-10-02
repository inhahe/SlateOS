### TD-OILS-A-RUN-OF-NEWLINES-AFTER-A-PIPE-MAKES-TIME-RESERVED-AGAIN. `true |⏎⏎time x` — 2026-08-10 — OPEN

**Where:** `userspace/oils/src/lexer.rs`, `CmdPos::time_ok` and the `AfterPipe`
it reads (formerly `pipe_refuses_time`). bash's
`time_command_acceptable` throws out a newline only when `token_before_that` was
a pipe (parse.y:3128-3134), which is exactly one token back — so a *second*
newline hides the pipe and `time` becomes the reserved word again. At that point
bash's own grammar rejects it, because `pipeline '|' newline_list pipeline`
yields a `pipeline` and `timespec` appears only in `pipeline_command`.

osh models the one-token rule faithfully, but its lexer collapses a run of
newlines into a single `Tok::Newline`, so the pipe is still visible two newlines
later and `time` stays an ordinary word.

**Reproduce.**

```sh
eval $'true |\ntime x1[1 2]=v';    echo "1 rc=$?"
eval $'true |\n\ntime x2[1 2]=v';  echo "2 rc=$?"
eval $'true |\n\n\ntime x3[1 2]=v'; echo "3 rc=$?"
eval $'true |\n\ntime true';       echo "4 rc=$?"
```

| row | bash 5.2.37 | osh |
|---|---|---|
| 1 | `time: command not found`, `rc=127` | same |
| 2–4 | `` syntax error near unexpected token `time' ``, `rc=2` | `time: command not found`, `rc=127` |

**The fix** is in two parts, and neither is small. The lexer would have to keep a
run of newlines as separate tokens (or record the run length) for
`CmdPos::advance` to see past the first — its `Tok::Newline` arm is what carries
`AfterPipe::Pipe` forward as `AfterPipe::Newline`, and a second newline would
have to clear it; and the parser would then have to
reject `timespec` after a `|`, which it currently accepts. Both only pay off for
this one corner — bash is arguably misbehaving here, since `true |⏎time x` and
`true |⏎⏎time x` differ only in a blank line. Worth doing only if the newline
run is needed for something else too.

Found while fixing TD-OILS-STARTS-COMMAND-DIVERGES-FROM-COMMAND-TOKEN-POSITION.
