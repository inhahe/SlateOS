### A-ONE-UID-NO-SAVED-SET-USER-ID -- 2026-10-01 -- OPEN (lane A)

**In short:** a process has one user id. Unix has three: real, effective and
saved. A program that drops root for a moment, by setting its effective id to
a user and back to 0 later, cannot do it here: the first switch is
permanent.

**Where.** `proc::pcb::ProcessCredentials` holds `uid` and `gid` only.
`syscall/linux.rs`'s `apply_uid_change` folds `setreuid`/`setresuid`'s three
ids into the one, effective first (its doc says so). Since design-decisions
§1502, `pcb::change_credentials` takes root's authority away when that one
uid leaves 0. That is Linux's rule for the common case: `setuid` as root sets
all three ids and clears the capabilities. But it is applied to a temporary
`seteuid` too.

**Who it bites.** Programs that switch to a user to touch that user's files
and switch back, as some sudo-style tools and daemons do. Linux-ABI programs
could never switch back here, because their gate is uid-based. Native
programs could, until §1502.

**The proper fix.** Model `ruid`/`euid`/`suid` (and the gids) in
`ProcessCredentials`, decide permission checks by the effective ids, and apply
Linux's capability rule exactly. Root's authority goes only when all three
uids leave 0. While only the effective id is non-zero it is suspended rather
than lost, which in this capability model needs a "suspended" state on the
narrowed entries.

**Reproduce.** A native or Linux-ABI program as root: `seteuid(1000)`, then
`seteuid(0)`. Linux allows the second call; here it is refused.
