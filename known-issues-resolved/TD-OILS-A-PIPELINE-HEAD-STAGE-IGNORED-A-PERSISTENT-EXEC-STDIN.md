### TD-OILS-A-PIPELINE-HEAD-STAGE-IGNORED-A-PERSISTENT-EXEC-STDIN — 2026-08-06 — ✅ FIXED 2026-08-06

**Where:** `userspace/oils/src/interp.rs` — `Shell::pipeline_head_stdin`.

**What.** The head stage of a pipeline gets its input from that function alone,
and it answered `None` for `StdinSrc::Inherit` on the theory that a persistent
`exec < file` would be "consulted per-command further down". It is not: both
pipeline drivers turn `None` into `Stdio::inherit()`, i.e. the shell's **real**
stdin. So an `exec <` (and, after the fix above, an enclosing group's `< file`)
was invisible to a head stage:

```text
$ bash -c 'exec < six.txt; head -n 1 | cat; head -n 1'
r1
r2
$ osh -c 'exec < six.txt; head -n 1 | cat; head -n 1'
r1                       # the pipeline read the script's own stdin
```

**Fixed by** resolving `Inherit` through `Shell::exec_stdin` there, leaving
`None` for a genuinely unbound fd 0 — which is the only case
`Stdio::inherit()` is right for. Regression tests:
`a_pipeline_head_stage_reads_the_ambient_fd_zero` and
`an_exec_inside_a_redirected_group_rebinds_fd_zero_for_the_rest_of_it`.
