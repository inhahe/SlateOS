/*
 * SlateOS: <execinfo.h> -- the calling thread's backtrace, which musl does
 * not have.
 */

/* A system header, as the musl ones it extends are: warnings in it are not
 * the program's. (scripts/check-libc-overlay.py defines the macro, to hold
 * it to -Wall -Wextra itself.) */
#ifndef _SLATEOS_OVERLAY_WARNINGS
#pragma GCC system_header
#endif

#ifndef _EXECINFO_H
#define _EXECINFO_H 1

#ifdef __cplusplus
extern "C" {
#endif

/* Store up to the second argument's return addresses of the frames calling
 * this, innermost first; the number stored. */
int backtrace(void **, int);

/* A malloc'd array of strings describing each address, to be freed as one
 * block. */
char **backtrace_symbols(void *const *, int);

/* The same, written a line each to the descriptor, with no allocation. */
void backtrace_symbols_fd(void *const *, int, int);

#ifdef __cplusplus
}
#endif

#endif /* _EXECINFO_H */
