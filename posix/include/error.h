/*
 * SlateOS: <error.h> -- GNU's error reporting, which musl does not have.
 */

/* A system header, as the musl ones it extends are: warnings in it are not
 * the program's. (scripts/check-libc-overlay.py defines the macro, to hold
 * it to -Wall -Wextra itself.) */
#ifndef _SLATEOS_OVERLAY_WARNINGS
#pragma GCC system_header
#endif

#ifndef _ERROR_H
#define _ERROR_H 1

#include <bits/slateos-features.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Flush stdout, then print the program's name, the message formatted from
 * the third argument on and -- if the second is not 0 -- its strerror to
 * stderr; then exit with the first, if it is not 0. */
void error(int, int, const char *, ...) _SLATEOS_PRINTF(3, 4);

/* error, with a file name and line number before the message. */
void error_at_line(int, int, const char *, unsigned int, const char *, ...) _SLATEOS_PRINTF(5, 6);

/* If not NULL, called instead of printing the program's name. */
extern void (*error_print_progname)(void);

/* How many messages the two have printed. */
extern unsigned int error_message_count;

/* If not 0, error_at_line prints nothing for the file and line it printed
 * last. */
extern int error_one_per_line;

#ifdef __cplusplus
}
#endif

#endif /* _ERROR_H */
