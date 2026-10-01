# D → A: Landlock answers as no Linux kernel does, and not as the C library does

**Status:** OPEN · **Filed:** 2026-09-27 by lane D · **Priority:** low --
every answer here means "no Landlock", and a program probing for it falls
back either way.

## In short

A program asks whether the kernel has Landlock (Linux's unprivileged sandbox)
by calling `landlock_create_ruleset(NULL, 0, LANDLOCK_CREATE_RULESET_VERSION)`.
Linux 6.6 has exactly two ways of saying "no":

| the kernel | answer, from `landlock_create_ruleset`, `landlock_add_rule` and `landlock_restrict_self` alike |
|---|---|
| built without `CONFIG_SECURITY_LANDLOCK` | `ENOSYS` (`kernel/sys_ni.c`'s `COND_SYSCALL`) |
| built with it, not enabled at boot | `EOPNOTSUPP`, **before any argument is looked at** (`security/landlock/syscalls.c`: `if (!landlock_initialized) return -EOPNOTSUPP;` opens all three) |

SlateOS gives one of each, and a third:

| who asks | through | answer |
|---|---|---|
| a native program | the C library's `syscall()` (`posix/src/sys_syscall.rs`: its translation table has no Landlock numbers, so all three fall through to its default) | `ENOSYS` from all three -- "built without" |
| a Linux program | the kernel's Linux table (`kernel/src/syscall/linux.rs`, `sys_landlock_create_ruleset` and its two siblings) | `EOPNOTSUPP` for the version probe, but the other calls check their arguments first -- which no Linux kernel does |

## Why it matters, a little

A program that sees `EOPNOTSUPP` is told Landlock is there and a boot option
would switch it on, which is not true of SlateOS; and a program that passes a
bad argument sees `EINVAL` where either real kernel would have said "no
Landlock" first. A program run both ways -- natively and as a Linux binary --
sees two different kernels.

## What would settle it

SlateOS has no Landlock at all, so "built without" is the faithful answer:
`ENOSYS` from all three Linux-table calls, before any argument -- what the C
library's native path answers now. If lane A would rather model "built in,
switched off", then `EOPNOTSUPP` first from all three, and lane D will make
the C library answer the same (three rows in that table). Either way, the two
should agree.
