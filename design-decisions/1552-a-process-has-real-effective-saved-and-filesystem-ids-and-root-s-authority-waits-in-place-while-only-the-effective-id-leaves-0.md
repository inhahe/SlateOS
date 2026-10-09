## 1552. A process has real, effective, saved and filesystem ids, and root's authority waits in place while only the effective id leaves 0

**Date:** 2026-10-08 · **Decided by:** Claude (autonomous; revisits §1502, which is also Claude's) · **Lane:** A

**In short:** a Unix program that starts as root can set aside root for a
while. It acts as an ordinary user, then switches back (`seteuid(1000)` …
`seteuid(0)`). Or it can give root up for good (`setuid(1000)`). SlateOS
had one user id per process, so every switch was for good, and a program that
only meant to step aside lost root permanently. A process now has the four ids
Linux has. Root's authority follows them: it is set aside while only the
effective id is a user's, it comes back with it, and it is gone once no id is
0. Writing this also turned up a worse bug: a program started by any user ran
as root (see 3).

**What it is.** `pcb::ProcessCredentials` holds the real, effective, saved and
filesystem user ids (`ruid`, `uid`, `suid`, `fsuid`) and the same for groups.
`uid`/`gid` stay the effective ids, the ones permission checks have always
read. `proc::setid` holds Linux's rules for `setuid`, `setreuid`, `setresuid`,
`setfsuid` and their group twins (`kernel/sys.c`), shared by the Linux calls
and the new native `SYS_PROCESS_SET_IDS` (1159) and `SYS_PROCESS_GET_IDS`
(1160). Each change is decided and made under one lock
(`pcb::update_credentials`), so two threads' `setuid`s cannot both decide
against the same old ids. `exec` sets the saved and filesystem ids to the
effective ones, as Linux's does. File access and file ownership go by the
filesystem ids. `/proc/<pid>/status` prints all four columns.

### 1. Root's authority is put aside in each capability entry, not taken out of the table

**Chosen:** a capability entry has a second set of rights, `suspended`.
While the effective uid is not 0 but the real or saved one is, root's rights
(`cap::rights_without_root`) move there from `rights`. They permit nothing,
and they move back when the effective uid returns to 0
(`CapTable::suspend`, `restore`). When every id leaves 0 they are dropped
(`drop_suspended`). An entry with all of its rights put aside stays in the
table, valid but permitting nothing.

**Alternative:** take root's rights out of the table into a stash on the
process, and put them back on the way in.

| | In place (chosen) | A stash |
|---|---|---|
| A revocation meanwhile | finds the entry and is final | misses a stashed entry; putting it back undoes the revocation, and could name a reused device or IRQ |
| A handle given away, copied, closed | carries or loses only what it may use; nothing to reconcile | the stash must match handles and resources on the way back |
| What `SYS_CAP_QUERY` shows meanwhile | the entry, with no rights | nothing |
| Fork | the table's copy carries it | a second thing to copy |

The visible entry with no rights is the one cost. It is also honest: Linux
shows a non-empty `CapPrm` and an empty `CapEff` for the same state. A
spawned child's copy of the table carries the rights set aside too
(`insert_with_suspended`), as a forked child's does.

### 2. Who may set an id

**Chosen:** a Linux program is privileged when its effective uid is 0 or it
holds `SET_CREDENTIALS` over processes, as Linux's `CAP_SETUID` is held while
the effective uid is 0. A native program is privileged only through the right
(`setid::Authority::Native`).

**Alternative:** let an id of 0 count for native programs too. Rejected: it
is ambient authority (power you have by being someone, not by holding a
token), which the design rules out. Within that, a native program moves
between its ids by Linux's rules unprivileged, as any Unix program may.

### 3. A spawned child is its parent's user

**Chosen:** a spawn copies the parent's credentials, with what an exec does to
them (`pcb::inherit_credentials`). A child with no user id of 0 loses the
root rights its copied table had set aside. If the parent's credentials
cannot be read, the spawn fails rather than start the child as root.

**What it fixes:** every new process record starts with root's credentials,
and spawn changed them only when a kernel caller named a uid. So a program
spawned by a user's shell had uid 0. That uid passed every file permission
check (`fs::vfs` lets uid 0 through), and its Linux `setuid` would set any id.
Fork was not affected, because it copies the credentials. This is
`known-issues/A-SPAWNED-CHILD-RUNS-AS-ROOT.md`.

**Alternative:** start every child as root and let it drop to a user, as the
test fixtures assumed. Rejected: `posix_spawn` is specified as a fork and an
exec, and a user's child being root is the escalation itself.

### 4. Which id each reader takes

Linux reads different ids in different places, and the code that read the one
`uid` was audited for each:

| Reader | Id | Linux |
|---|---|---|
| permission checks, `uid == 0` tests, a peer's `SO_PEERCRED`, `/proc` writes' privilege | effective | `euid`, `CAP_*` |
| file access, a new file's owner, `chattr`'s owner test, deferred file operations | filesystem | `fsuid` |
| a signal's `si_uid` (kill, SIGCHLD, ptrace's), per-user counts (`RLIMIT_SIGPENDING`, SCM_RIGHTS in flight), `PRIO_USER`, a send's own `SCM_CREDENTIALS` | real | `current_uid()`, `task_uid` |
| credentials a sender states | any of real, effective, saved | `scm_check_creds` |
| `/proc/<pid>` inspection by its user | reader's filesystem ids against the target's real, effective and saved ids | `__ptrace_may_access` |

**Revisits §1502**, which took root's authority away whenever the one uid
left 0. That rule still holds when every id leaves 0. Only the temporary
drop changed.
