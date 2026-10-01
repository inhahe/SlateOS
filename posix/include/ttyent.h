/*
 * SlateOS: <ttyent.h> -- the terminal table, /etc/ttys, which musl does not
 * have.
 *
 * struct ttyent is glibc's, field for field; the functions read the table
 * as glibc's misc/getttyent.c does (posix/src/ttyent.rs).
 */

/* A system header, as the musl ones it extends are: warnings in it are not
 * the program's. (scripts/check-libc-overlay.py defines the macro, to hold
 * it to -Wall -Wextra itself.) */
#ifndef _SLATEOS_OVERLAY_WARNINGS
#pragma GCC system_header
#endif

#ifndef _TTYENT_H
#define _TTYENT_H 1

#include <features.h>

#ifdef __cplusplus
extern "C" {
#endif

#define _PATH_TTYS "/etc/ttys"

/* The words of a line's flags. */
#define _TTYS_OFF "off"
#define _TTYS_ON "on"
#define _TTYS_SECURE "secure"
#define _TTYS_WINDOW "window"

/* A terminal's entry. */
struct ttyent {
	char *ty_name;    /* the terminal's device name */
	char *ty_getty;   /* the command started on it, usually a getty */
	char *ty_type;    /* its terminal type */
#define TTY_ON 0x01       /* logins enabled ("on") */
#define TTY_SECURE 0x02   /* root may log in ("secure") */
	int ty_status;    /* TTY_ON, TTY_SECURE */
	char *ty_window;  /* the command that starts its window system */
	char *ty_comment; /* the line's comment */
};

/* The next entry; a terminal's, from the start; open or rewind the table
 * (1, or 0); close it (1, or 0). */
struct ttyent *getttyent(void);
struct ttyent *getttynam(const char *);
int setttyent(void);
int endttyent(void);

#ifdef __cplusplus
}
#endif

#endif /* _TTYENT_H */
