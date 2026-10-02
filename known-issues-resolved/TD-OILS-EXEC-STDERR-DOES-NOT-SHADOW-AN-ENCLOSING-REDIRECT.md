### TD-OILS-EXEC-STDERR-DOES-NOT-SHADOW-AN-ENCLOSING-REDIRECT. A runtime `exec 2>…` is ignored inside a body that redirected fd 2 — ✅ RESOLVED — 2026-08-01

**Where:** `userspace/oils/src/interp.rs` — `Shell::emit_stderr_depth` and the
`stderr_stack` (declared ~2655, ~25 push/pop sites). A compound command's own
`2>…` pushes a `StderrTarget` for the body's duration; a diagnostic raised inside
the body takes the top of that stack. A `exec 2>…` executed *in* the body sets
`self.exec_stderr` instead, which is only consulted when the stack is empty — so
the enclosing push keeps winning and the `exec` does nothing until the body ends.

**Reproduce** (measured 2026-08-01 against the reference bash):

```sh
v=$( { exec 2> f1; cd /nosuchdir; } 2>&1 )
# bash: v=[]                                f1=[…: cd: /nosuchdir: No such file or directory]
# osh : v=[…: cd: /nosuchdir: No such …]    f1=[]

v=$( { exec 2>&-; cd /nosuchdir; } 2>&1 )
# bash: v=[]      (fd 2 closed, diagnostic dropped)
# osh : v=[…: cd: /nosuchdir: No such file or directory]
```

Exactly backwards in the first case: the message goes wherever the *enclosing*
redirect points rather than where the `exec` just pointed it.

**Impact.** The `{ exec 2> log; …; } 2>&1` idiom — divert a body's diagnostics
partway through, having first pointed fd 2 somewhere for the part before it —
sends everything to the wrong place. Independent of `>&-`: it predates
`TD-OILS-TRANSIENT-CLOSE-OF-A-STD-FD-IS-A-NO-OP` and was found while testing it —
which is why that entry's fd 2 unit tests were written around the
`{ exec 2>&-; … } 2>&1` shape rather than asserting it.

**Related.** fd 1 has the same shape and it is already handled:
`Shell::exec_stdout_shadowing` answers "did a persistent `exec` rebind fd 1 after
this ambient sink was installed?" and both `write_bytes` and `alias_write_fd` ask
it first. fd 2 has no equivalent.

**Fixed (2026-08-01).** The order the two were established in is now recorded,
which is the whole of the question: `Shell::exec_stderr_depth` holds the
`stderr_stack` depth at which the current `exec_stderr` was installed, and
`Shell::stderr_target_at` — the one place both `emit_stderr_depth` and
`stderr_has_no_write_half` now read the stack through — falls through to the base
binding whenever no entry was pushed *after* the `exec`. A depth turned out to be
enough; the generation counter sketched above would have had to be stamped onto
all ~18 push sites, where a depth is read at the 2 sites that consult the stack
and written by the one new `Shell::set_exec_stderr` that every persistent fd 2
assignment goes through.

Two things fell out of getting the restore right:

* `exec_with_redirects` now saves `exec_stderr` whenever the body pushed *any*
  stderr target, not only when it produced a `stderr_file`. A `{ …; } 2>&1`
  collected by a capture pushes a `Buffer` and leaves `stderr_file` unset, so
  without this the newly-shadowing `exec` would have gone on applying after the
  body ended, where bash restores fd 2 with the rest of the group's redirects.
* The depth is saved and restored with it, so a body that ends does not leave the
  shadow pointing into a stack that has since shrunk.

Measured against bash on the nested (`{ { exec 2>f; …; } 2>&1; …; } 2>&1`) and
sourced (`. s 2>e1` where `s` does `exec 2>g1`) shapes as well as the plain ones;
covered by `tests/corpus/exec-stderr-shadows-an-enclosing-redirect.sh` and two
unit tests. `redirect-close-of-stderr.sh` still runs its persistent probes in
subshells — that is its own subject, not a workaround for this.
