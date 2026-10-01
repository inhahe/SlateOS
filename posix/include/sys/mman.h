/*
 * SlateOS: what this C library's <sys/mman.h> has that musl's does not
 * declare -- see <math.h> in this directory for why this file exists.
 */

/* A system header, as the musl ones it extends are: warnings in it are not
 * the program's. (scripts/check-libc-overlay.py defines the macro, to hold
 * it to -Wall -Wextra itself.) */
#ifndef _SLATEOS_OVERLAY_WARNINGS
#pragma GCC system_header
#endif

#include_next <sys/mman.h>

#ifndef _SLATEOS_SYS_MMAN_H
#define _SLATEOS_SYS_MMAN_H

#ifdef __cplusplus
extern "C" {
#endif

#ifdef _GNU_SOURCE
/* Memory protection keys (Linux; glibc 2.27). SlateOS has none: pkey_alloc
 * answers ENOSPC, as Linux does on a processor without them, and
 * pkey_mprotect takes only -1, which is mprotect. */
#ifndef PKEY_DISABLE_ACCESS
#define PKEY_DISABLE_ACCESS 0x1
#endif
#ifndef PKEY_DISABLE_WRITE
#define PKEY_DISABLE_WRITE 0x2
#endif
int pkey_alloc(unsigned int, unsigned int);
int pkey_set(int, unsigned int);
int pkey_get(int);
int pkey_free(int);
int pkey_mprotect(void *, size_t, int, int);

/* Advice about another process's memory, and the release of a dying one's,
 * through a pidfd (Linux; glibc 2.36). A native program has no pidfds: once
 * the flags and the vector pass, EBADF. */
struct iovec;
ssize_t process_madvise(int, const struct iovec *, size_t, int, unsigned int);
int process_mrelease(int, unsigned int);
#endif

#ifdef __cplusplus
}
#endif

#endif /* _SLATEOS_SYS_MMAN_H */
