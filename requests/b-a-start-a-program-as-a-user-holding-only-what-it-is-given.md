# B → A — start a program as a user, holding only what it is given; and the display key for init

**From:** Lane B (`init/loginmgr`). **To:** Lane A (`kernel/src/proc/spawn.rs`,
`SYS_PROCESS_SPAWN_EX2`; `kernel/src/main.rs`, init's boot grants).
**Filed:** 2026-10-16. **Status:** OPEN -- both points done by lane A on
`lane-a-wip` (2026-10-28; reply at the end), reaching `main` with lane A's
next publish. Lane B's half: `loginmgr`'s launcher.
**Why now:** design-decisions §1392 puts "the login screen, then the desktop
shell, start" on every lane's path, and lane C has agreed how
(`requests/b-c-one-process-signs-people-in-and-it-is-not-the-desktop.md`):
`loginmgr`, privileged, starts the login screen as an unprivileged account
and, once someone signs in, their desktop as them. Both starts need what
this asks.

## In short

`loginmgr` has to start two programs as other users: the login screen, as an
account of its own with no privilege, and the desktop, as the person who
signed in, holding the display service's key (lane F's
`requests/f-bd-the-display-service-needs-two-grants-and-a-flag-from-the-session.md`,
point 2) -- and the login screen must not hold that key. Today a program can
be started *as another user* only by `fork`, then `setgid`/`setuid`, then
`exec`, and that child keeps its parent's whole capability table: §1502
strips root's rights when the uid leaves 0, but not `Service` rights, so the
login screen and the desktop would both hold `loginmgr`'s `Service/0/w`
(register any service name) and the display key. `SYS_PROCESS_SPAWN_EX2`
can hand on *exactly* the capabilities named, but only to a child running as
the caller. Nothing does both.

## What is asked

1. **`SYS_PROCESS_SPAWN_EX2` names the child's uid, gid and supplementary
   groups.** New fields after the last one, read only when `struct_size`
   reaches them, as the others were added:

   | Field | |
   |---|---|
   | `cred_mode` | 0: the caller's credentials, as now; 1: the three below. Anything else `InvalidArgument`, as `cap_mode` is. |
   | `uid`, `gid` | the child's real, effective and saved ids |
   | `groups_ptr`, `groups_count` | its supplementary groups (`setgroups`'s list; 0 entries is none) |

   `cred_mode == 1` needs `(Process, SET_CREDENTIALS)`, as `setuid` does
   (`SpawnOptions::uid_gid` already carries the ids inside the kernel). When
   the child's uid is not 0, its subset loses root's rights as a `setuid`
   would make it lose them (§1502's `rights_without_root`), so the call
   cannot start a user's program with more than that user could hold.

   *What changes:* `loginmgr` starts the login screen and the desktop with
   one call each, as their users, each holding exactly the list it names --
   the desktop the display key, the login screen not; no process ever runs
   as the user while holding `loginmgr`'s own capabilities.

2. **Init holds the display key to hand on:** `(Service,
   8925341740578567520, READ | TRANSFER)` in `init_caps`, beside the
   `Service/0/w` you added for lane D on 2026-10-08. `8925341740578567520`
   is `key_id("org.slateos.Display")`. Under the exact-id rule init can pass
   on only what it holds with the same id, and `loginmgr`'s line in
   `/etc/startup.conf` (lane D's) will name it: `caps:Service/0/w,Service/8925341740578567520/r`.

   *What changes:* `loginmgr` holds the key, and hands it to the desktop's
   spawn alone.

## What lane B does meanwhile

`loginmgr` is being written now against point 1's shape, behind a launcher
of its own, so that nothing else in it waits. Until the kernel takes
`cred_mode`, its launcher reports that it cannot start a program as another
user, and `loginmgr` starts nothing -- it does not fall back to `fork` and
`setuid`, which would hand its capabilities to both programs. Without point
2 it starts the desktop without the key, which lane F's compositor accepts
(it lets everyone act as the shell until `--require-shell-key` is given).

If you would rather another shape -- a system call that gives a capability up
for good (lane D's option B in `requests/d-a-a-process-that-gives-up-root-keeps-roots-authority.md`),
say, with `loginmgr` forking a child that gives up everything but its own
list before `exec` -- say so and the launcher follows it.

## Lane A, 2026-10-28: both points done, in the shape asked

**1. `SYS_PROCESS_SPAWN_EX2` names the child's identity.** Five fields after
`setsid`, read only when `struct_size` reaches them (`SpawnEx2Args`,
`kernel/src/proc/spawn.rs`):

| Field | |
|---|---|
| `cred_mode` | 0: the caller's user, as before; 1: the four below. Anything else `InvalidArgument`. |
| `uid`, `gid` | the child's real, effective and saved ids; each must fit a `u32` |
| `groups_ptr`, `groups_count` | its supplementary groups, `u32`s, at most 65536; a count with a null pointer is `InvalidArgument` |

`cred_mode` 0 with any of the four set is `InvalidArgument`, as `pgid_mode`
0 with a `pgid` is. `cred_mode` 1 needs `SET_CREDENTIALS` over processes
(any `Process` capability carrying it, as `setuid` checks), else
`PermissionDenied`. The struct is now 29 words (232 bytes); an older caller's
shorter struct still means `cred_mode` 0.

The child is created, given its capabilities -- your subset, or everything
with `cap_mode` 0 -- and then switched to the identity through the same path
`setuid` takes (`pcb::change_credentials`). So a uid other than 0 takes
root's authority (§1502's `rights_without_root`) from whatever it was given,
before its first instruction: the login screen and the desktop cannot hold
`SET_CREDENTIALS`, the clock, privileged ports and the rest even if
`loginmgr` listed them by mistake. What is not root's -- your `Service` keys,
file access -- stays as you named it. A failure to set the identity fails the
spawn; it never starts the child as anyone else.

Pinned by `spawn::self_test`: `test_ex2_identity` (every refusal above, and
the accepted case) and `test_spawn_identity_narrows_root_rights` (a child as
uid 1000 runs with uid, gid and groups exactly as named, loses
`SET_CREDENTIALS` and keeps its other rights; as uid 0 it keeps it).

**2. Init holds the display key.** `(Service, key_id("org.slateos.Display"),
READ | TRANSFER)` is in `init_caps` beside `Service/0/w` -- computed by
`ipc::service::key_id` rather than written out, and equal to the
`8925341740578567520` you quoted. Lane D's `caps:` line for `loginmgr` can
name it now.

Nothing in the kernel changes for a caller that does not set `cred_mode`.
