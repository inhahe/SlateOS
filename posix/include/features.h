/*
 * SlateOS: the one rule of glibc's <features.h> that musl's no longer has.
 *
 * glibc's _GNU_SOURCE turns on _LARGEFILE64_SOURCE -- glibc 2.39's
 * <features.h> undefines and redefines it to 1 under _GNU_SOURCE -- so a
 * program written for glibc that asks for _GNU_SOURCE sees off64_t,
 * fopen64, lseek64 and the rest of the large-file names. musl did the same
 * until 1.2.4, which kept them for an explicit _LARGEFILE64_SOURCE only, and
 * zig 0.13's musl, whose headers C is compiled against here, is 1.2.5. This
 * C library is glibc's to its callers (design-decisions 1141), and libc.a
 * defines the large-file functions as real symbols: so it takes glibc's rule.
 *
 * Without it, a glibc program that probes for one of them finds it in libc.a
 * and no declaration of it anywhere, and then declares it itself -- binutils'
 * bfd/sysdep.h writes `extern off64_t ftello64 (FILE *stream);`, which does
 * not compile when nothing defines off64_t. GDB's build stopped there
 * (scripts/gdb-spike/), in every file of bfd that includes it.
 *
 * Every musl header begins with `#include <features.h>`, and this directory
 * is searched before musl's, so this file is read first by all of them.
 */

/* A system header, as the musl ones it extends are: warnings in it are not
 * the program's. (scripts/check-libc-overlay.py defines the macro, to hold
 * it to -Wall -Wextra itself.) */
#ifndef _SLATEOS_OVERLAY_WARNINGS
#pragma GCC system_header
#endif

#if defined(_GNU_SOURCE) && !defined(_LARGEFILE64_SOURCE)
#define _LARGEFILE64_SOURCE 1
#endif

#include_next <features.h>
