## D-POSIX-SYS-MOUNT-H-COLLIDED-WITH-LINUX-MOUNT-H — a C program including `<sys/mount.h>` and then `<linux/mount.h>` did not compile: the new mount API's enum and struct were defined twice (lane D, 2026-10-07) — FIXED 2026-10-07
**Status:** FIXED 2026-10-07 — `posix/include/sys/mount.h` brings in `<linux/mount.h>` first when there is one, as glibc's does

**In short:** since 2026-10-05 this library's `<sys/mount.h>` declares the
new mount API -- `fsopen`, `fsconfig`, `mount_setattr` and the rest -- with
their `enum fsconfig_command` and `struct mount_attr`, as glibc 2.36 and
later do. The kernel's own `<linux/mount.h>` defines the same enum and
struct. glibc 2.36 shipped with exactly this clash: a program including
both headers, `<sys/mount.h>` first, did not compile. glibc fixed it by
having `<sys/mount.h>` include `<linux/mount.h>` itself when it exists,
and by defining its own copies only when that header has not (`#ifndef
FSOPEN_CLOEXEC`, `#ifndef MOUNT_ATTR_SIZE_VER0`). Our header had glibc's
two guards but not the include before them, so the guards never fired in
that order. Mono's runtime (`mono/metadata/w32file-unix.c`) includes both
headers, and its build stopped there.

**How it was found.** By `scripts/mono-spike/` on its first build of Mono
6.14.1, 2026-10-07: the build's only failure.

**The fix.** The `__has_include(<linux/mount.h>)` block glibc 2.39's
header opens with, ahead of the guarded definitions. Compiles cleanly with
`-Wall -Wextra` in all three orders: `<sys/mount.h>` then `<linux/mount.h>`,
the reverse, and `<sys/mount.h>` alone. The overlay's macros are the
kernel header's own, spelled the same way, so including both redefines
nothing. `check-libc-overlay.py` passes in all 16 of its settings (`struct
mount_attr` keeps glibc's layout, now through the kernel's definition) and
its self-test passes.

**What it could have cost.** Any program written for glibc that uses the
new mount API through the kernel header's constants, or merely includes
both headers -- container tools, systemd's utilities, Mono -- failed to
compile here.
