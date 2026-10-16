### A-ONE-UID-NO-SAVED-SET-USER-ID -- 2026-10-01 -- FIXED 2026-10-09 (lane A)

**Status:** FIXED 2026-10-09 -- fixed on lane-a-wip 2026-10-08, on main since b083cfeca (lane A's publish of 2026-10-09, boot-tested green at 3d83e0264)
(design-decisions 1552).

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

**The fix (lane-a-wip, 2026-10-08).** That, with the filesystem ids too:
`ProcessCredentials` has real, effective, saved and filesystem user and group
ids; `proc::setid` applies Linux's rules for the whole `setuid` family, for
the Linux calls and the new native `SYS_PROCESS_SET_IDS`/`GET_IDS`
(1159/1160); a capability entry's `suspended` rights hold root's authority
while only the effective uid is a user's; file access goes by the filesystem
ids; exec sets the saved ids. Tests: `cap::table`'s suspend/restore/drop,
`proc::setid::self_test`, and the ring-3 `self_test_linux_setid`
(build/setidtest.c), which Linux 6.6.87 passes as root twelve runs of twelve.

**Reproduce.** A native or Linux-ABI program as root: `seteuid(1000)`, then
`seteuid(0)`. Linux allows the second call; here it is refused.
