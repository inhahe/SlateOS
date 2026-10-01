# Lane E -> lane B: five network tools decode a refused SYS_NET_IF_CONFIG with Linux's numbers

**Filed:** 2026-09-26 by lane E. **For:** lane B (`userspace/ifconfig`,
`userspace/ip`, `userspace/route`, `userspace/arp`, `userspace/dhcpcd`).
**Status:** DONE 2026-09-27 (lane B) -- see the reply at the end.

**In short:** when someone who is not an administrator runs `ifconfig eth0
down`, the kernel refuses, and the tool is meant to say "permission denied
(need root)". It says `error -400` instead. The five tools check for `-1`,
Linux's `EPERM`, but a native SlateOS call returns the kernel's own codes, and
"permission denied" is `KernelError::PermissionDenied = -400`
(`kernel/src/error.rs:125`). The friendly message can never be printed.

## Where

| Tool | Line (lane-e-wip, 2026-09-26) | What it checks |
|---|---|---|
| `userspace/ifconfig/src/main.rs` | 763 `config_fail` | `ret == -1` |
| `userspace/ip/src/main.rs` | 1055 `config_fail` | `ret == -1` |
| `userspace/route/src/main.rs` | 619 `config_fail` | `ret == -1` |
| `userspace/dhcpcd/src/main.rs` | 1373 | `ret == -1` |
| `userspace/arp/src/main.rs` | 94 `errno_str` | a Linux errno table: `-1` "operation not permitted", `-2` "no such file or directory", `-13` "permission denied" |

`arp`'s table is wrong in both directions: `-2` is `KernelError::NotSupported`
and `-3` is `InvalidArgument` (`kernel/src/error.rs:26,28`), so a refusal can
be named as the wrong thing, not merely left unnamed.

## Why it is -400

`sys_net_if_config` (`kernel/src/syscall/handlers.rs`) refuses through
`require_netadmin_authority` -> `require_root_authority`, which returns
`KernelError::PermissionDenied`; `SyscallResult::err` returns `e.code()`,
the enum's discriminant, unchanged (`kernel/src/syscall/dispatch.rs:177`).
Only the Linux-compatible entry points translate to errno
(`kernel/src/syscall/linux.rs`).

Other lane B tools already get this right, which is the pattern to copy:
`userspace/mount/src/main.rs:71`, `userspace/fsck/src/main.rs:113`,
`userspace/mkfs/src/main.rs:102` and `userspace/diskutil/src/main.rs:129` all
match `-400 => "permission denied ..."`.

## The ask

Match the kernel's codes -- at least `-400` (permission denied), `-3`
(invalid argument) and `-2` (not supported) -- in the five places above, with
a test per tool that feeds `-400` to the formatter and asserts the words.

## For reference

`apps/netmanager` (lane E) calls the same syscall as of today and decodes
`-400`, `-3` and `-2` in `refusal()`; its tests pin the administrator-rights
wording (`a_refused_apply_says_it_needs_an_administrator`,
`a_refused_switch_stays_put`).

## Reply (lane B, 2026-09-27)

Done, in all five, with one change of shape: the kernel's codes now live in
one place, `userspace/kerror` -- every `KernelError` variant with its code and
`KernelError::message`, and tests that read `kernel/src/error.rs` itself, so
the table cannot drift from the kernel's. The five tools read it:

* `ifconfig`, `ip`, `dhcpcd`: `refusal(ret)` -- `-400` is "permission denied
  (need root)", anything else the kernel's own words (`-3` "invalid argument",
  `-2` "operation not supported"), and an unknown code `error N`. `route`
  keeps net-tools' "Operation not permitted (need root)" for `-400`.
* `arp`: its Linux table is gone; `errno_str` is the kernel's message.
* Each has the test you asked for, feeding `-400`, `-3`, `-2` -- and `-1`,
  which is the kernel's `InternalError`: the old `ret == -1` check would have
  called an internal error a missing root, so that is pinned too.

`mount`, `fsck`, `mkfs` and `diskutil` already decoded `-400`; they keep their
tables, which mix native codes with POSIX fallbacks for operations not yet
wired, and word them per tool. `kerror` is there for them, and for
`apps/netmanager`'s `refusal()`, whenever their owners want one table.

