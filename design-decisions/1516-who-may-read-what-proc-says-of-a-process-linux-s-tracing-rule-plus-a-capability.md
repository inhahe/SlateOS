## 1516. Who may read what `/proc` says of a process: Linux's tracing rule, plus a capability

**Date:** 2026-10-01 · **Decided by:** Claude (autonomous) · **Lane:** A

**In short:** `/proc/<pid>/` lets one program look inside another: its
environment variables, which often carry passwords and access tokens; where
its code and data sit in memory, which defeats address randomisation; what it
has open; what it is waiting on. Until now any program could read all of it
for any other. Now the private parts need the same permission Linux asks: the
reader is the program itself, runs as the same user (while the program
allows it), or is the administrator. This kernel adds one more way in -- a
capability for the process. Nothing changes for anyone today, because every
program still runs as uid 0, the administrator. It matters from the first
login that starts a second user's programs.

**What changed:**
- `pcb::may_inspect(reader, target)` decides it (and `pcb::may_access` for
  a change, asking `WRITE` of the capability instead of `READ`).
- **Refused** (`EACCES`) to a reader who may not inspect:
  - reading `environ`, `auxv`, `maps` and `io`;
  - the `cwd`, `root`, `exe` and `fd/<n>` links;
  - listing `fd/` and `fdinfo/`, and reading `fdinfo/<n>`.
- **Blanked** instead: `wchan` reads `0` and `stat` field 35 is 0, as on
  Linux.
- **Open to all**, as on Linux: `status`, `cmdline`, `stat` (all but field
  35), `statm`, `limits`, `mounts`, `cgroup` and the rest.
- **Writing `oom_score_adj`** needs `may_access(..., WRITE)`, and lowering it
  needs uid 0 -- Linux's `CAP_SYS_RESOURCE` for making a process the last one
  the out-of-memory killer picks.

**The rule**, in order. The reader may inspect if any of these holds:
- it is the kernel;
- it is the target;
- it runs as uid 0, Linux's `CAP_SYS_PTRACE`;
- it has the target's uid and gid, and the target is dumpable
  (`PR_SET_DUMPABLE` 1);
- it holds a `Process` capability for the target with `READ`.

A kernel task belongs to no process: only uid 0 and the kernel may inspect
one.

**Alternatives:**

| | What changes | For | Against |
|---|---|---|---|
| **A. Linux's rule, plus a `Process` capability (chosen)** | the files Linux guards, guarded as Linux guards them; a capability opens a process to a holder of another identity | Linux programs and users meet the behaviour they expect; the capability gives a non-root process explorer a way in without ambient authority | two ways in, identity and capability, where the design leans to capabilities alone |
| B. Capabilities only (the signalling rule) | a process sees only itself, its children, and what it holds a capability for | no ambient authority | `ps e`, `top`, `lsof`, debuggers and the explorer cannot see a user's own processes without a capability for each; nothing grants those today |
| C. Leave procfs open | nothing | no work | a second user reads the first one's tokens from `environ` |

**Smaller decisions:**

| decision | alternative | why this one |
|---|---|---|
| Checked when the file is generated, so `stat` of a guarded file reports 0 bytes | report its real size | the size of another process's environment is itself a leak, if a small one |
| `wchan` and field 35 blank rather than refuse | refuse | Linux prints `0`, and `ps -o wchan` must not fail on other users' processes |
| No per-file owner or mode in `stat` yet | report Linux's owner and mode | the VFS has no field for them here; the check at generation is the enforcement either way. `ls -l /proc/<pid>` shows no `0400` until it does |

**Revisit** when users other than uid 0 exist, and whether a parent should
inspect its children without a capability (Linux does not).
