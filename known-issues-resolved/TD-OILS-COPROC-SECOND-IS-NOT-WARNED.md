### TD-OILS-COPROC-SECOND-IS-NOT-WARNED. bash warns when a coproc is started while another is still alive; osh started it silently — ✅ RESOLVED — 2026-08-01

**Where:** `userspace/oils/src/interp.rs` — `exec_coproc`. It kept no notion of
"the current coproc", so nothing could notice a second one.

**Reproduce:**

```sh
coproc A { read x; echo "a:$x"; }
coproc B { read y; echo "b:$y"; }
```

bash writes to stderr:

```
bash: line 2: warning: execute_coproc: coproc [1103354:A] still exists
```

…and then starts B anyway. Everything after that matches: both coprocs run,
both endpoint pairs work, `jobs` lists both, `wait` reaps both. osh prints
nothing and behaves identically otherwise.

**Why bash warns.** It has exactly one `sh_coproc` struct, so a second live
coproc is a state it cannot fully represent — the warning is bash admitting it
is about to lose track of the first one's bookkeeping. osh has no such limit
(each coproc is an ordinary job with its own fds), so the warning is pure
diagnostic fidelity, not a shared limitation.

**Impact.** Diagnostic only, and it lands on stderr, so a script that merges
streams (`2>&1`) saw an extra line under bash that it did not under osh.
Found while probing the coproc endpoint dup.

**Fixed** by giving `Shell` a `coproc_tracked: Option<(String, u32)>` — the name
and synthetic pid of the most recently started coproc — set at the end of
`exec_coproc` and consulted at its start by `warn_coproc_still_exists`, which
emits the message through the ordinary `perrln` prefix machinery so it carries
the same `case.sh: line N:` bash gives it.

Three measured details decided the shape:

- **The condition is "not reaped", not "a coproc job exists".** A body that has
  already exited draws no warning even with no `wait` in sight, because bash
  reaps it asynchronously: `coproc F { :; }; sleep 1; coproc G { :; }` is
  silent. So liveness is asked of the job body (`JobBody::is_finished`) rather
  than inferred from the job's presence in the table.
- **Each new coproc replaces the tracked one**, so a chain of three warns twice
  — about the first, then about the second — never about the same one twice.
- **A subshell has no coproc to lose.** bash closes the coproc out in a
  subshell, so a `coproc` started there is silent even while the parent's is
  still running; `clone_for_subshell` therefore starts with `coproc_tracked:
  None`.

Corpus case `coproc-second-warns-about-the-first.sh` covers all of those plus
the unnamed (`COPROC`) form. It collects stderr in a file and filters the pid
out at the end, since the pid is a property of the host, not of the shell.
