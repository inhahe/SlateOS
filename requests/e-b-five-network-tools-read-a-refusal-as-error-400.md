# Lane E -> lane B: five network tools decode a refused SYS_NET_IF_CONFIG with Linux's numbers

**Filed:** 2026-09-26 by lane E. **For:** lane B (`userspace/ifconfig`,
`userspace/ip`, `userspace/route`, `userspace/arp`, `userspace/dhcpcd`).
**Status:** OPEN. Nothing is changed wrongly meanwhile -- only the message is.

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
