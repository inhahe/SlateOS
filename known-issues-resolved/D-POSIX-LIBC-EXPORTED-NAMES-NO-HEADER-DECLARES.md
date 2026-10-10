## D-POSIX-LIBC-EXPORTED-NAMES-NO-HEADER-DECLARES — `libc.a` exported fifteen names into the program's namespace that no header declared: `<limits.h>`'s constants as data, `select`'s helpers, `execl`'s internal targets, and `readdir64_r` misspelt (lane D, 2026-09-29) — **Status: FIXED 2026-09-29**

**In short:** a C library may give a program only the names its headers
declare, and names reserved to itself (a leading underscore). This one
exported fifteen more: `OPEN_MAX`, `CHILD_MAX`, `LINK_MAX`, `MQ_OPEN_MAX`,
`PATH_MAX_LIMIT`, `SYMLINK_MAX` and `TIMER_MAX` as data -- with 58 more of
`<limits.h>`'s constants, which musl's macros happened to hide; `select`'s
own helpers `fd_set_zero`, `fd_set_set`, `fd_set_clr` and `fd_set_isset`;
`vexecl`, `vexeclp` and `vexecle`, where `execl`'s trampolines jump; and
`readdir_r64`, a misspelling of glibc's `readdir64_r`. A program with a
global of one of those names of its own -- `OPEN_MAX`, in a program that
does not include `<limits.h>`, is legal C -- could fail to link, with two
definitions; and a program built against glibc that calls `readdir64_r`
could not link at all. Found by the second half of
`scripts/check-libc-declared.py`, new the same day, which asks whether every
public name the library defines is declared by some header.

**Fixed:** the limits are Rust constants and nothing more (`limits.rs`, not
`no_mangle`); `select`'s `fd_set_zero` and `fd_set_set` are `pub(crate)`
and unexported, and its `fd_set_clr`, which nothing called, and
`fd_set_isset`, a second copy of the `is_set_in` it reads sets with, are
gone (`poll.rs`); `execl`'s targets are `__slate_vexecl` and the rest, named as
the library's other internal symbols are (`spawn.rs`); and `readdir64_r`
has its name (`dirent.rs`). The gate refuses a new one.

**What the gate lets stand, on purpose** (`UNDECLARED_OK`, each with its
reason): seventeen Linux system calls glibc 2.39 declares no function for
either (`clone3`, `openat2`, `futex` ...), which C makes through
`syscall()`; `sysctl`, `sys_errlist` and `sys_nerr`, which glibc stopped
declaring; the six XSI STREAMS functions, which POSIX.1-2024 removed;
`fpurge`, BSD's name for `__fpurge`; gnulib's `verror` and
`verror_at_line`; SlateOS's own `setkeylayout` and `slateos_spawn_caps`,
whose C declarations wait on a header set for SlateOS's own calls; and the
compiler runtime's 35 `_Float16` and `_Float128` functions, which are the
compiler's to declare.

**Where:** `posix/src/limits.rs`, `poll.rs`, `spawn.rs`, `dirent.rs`;
`scripts/check-libc-declared.py`.
