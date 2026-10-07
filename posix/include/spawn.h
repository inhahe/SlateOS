/*
 * SlateOS: what this C library's <spawn.h> has that musl's does not declare
 * -- see <math.h> in this directory for why this file exists.
 */

/* A system header, as the musl ones it extends are: warnings in it are not
 * the program's. (scripts/check-libc-overlay.py defines the macro, to hold
 * it to -Wall -Wextra itself.) */
#ifndef _SLATEOS_OVERLAY_WARNINGS
#pragma GCC system_header
#endif

#include_next <spawn.h>

#ifndef _SLATEOS_SPAWN_H
#define _SLATEOS_SPAWN_H

#include <bits/slateos-features.h>

#ifdef __cplusplus
extern "C" {
#endif

#ifdef _GNU_SOURCE
/* Start the child in the cgroup posix_spawnattr_setcgroup_np names. There
 * are no cgroups here: a spawn asking for one fails with ENOTSUP. */
#define POSIX_SPAWN_SETCGROUP 0x100
#endif

#ifdef _SLATEOS_USE_MISC
/* A file action closing every descriptor from the argument up. */
int posix_spawn_file_actions_addclosefrom_np(posix_spawn_file_actions_t *, int);

/* A file action making the child's process group the foreground group of
 * the terminal at the descriptor. A spawn with it fails with ENOSYS here:
 * the system cannot yet set the group before the child runs. */
int posix_spawn_file_actions_addtcsetpgrp_np(posix_spawn_file_actions_t *, int);

/* The cgroup, by a descriptor of its directory, that POSIX_SPAWN_SETCGROUP
 * starts the child in. */
int posix_spawnattr_getcgroup_np(const posix_spawnattr_t *__restrict, int *__restrict);
int posix_spawnattr_setcgroup_np(posix_spawnattr_t *, int);

/* posix_spawn and posix_spawnp answering a pidfd for the child (glibc 2.39).
 * A native program has no pidfds: ENOSYS, and no child. */
int pidfd_spawn(int *__restrict, const char *__restrict,
                const posix_spawn_file_actions_t *, const posix_spawnattr_t *__restrict,
                char *const *__restrict, char *const *__restrict);
int pidfd_spawnp(int *__restrict, const char *__restrict,
                 const posix_spawn_file_actions_t *, const posix_spawnattr_t *__restrict,
                 char *const *__restrict, char *const *__restrict);
#endif

#ifdef __cplusplus
}
#endif

#endif /* _SLATEOS_SPAWN_H */
