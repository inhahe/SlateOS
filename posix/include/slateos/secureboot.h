/*
 * SlateOS: <slateos/secureboot.h> -- the kernel's Secure Boot lists, which Linux
 * keeps in the firmware's variables instead. Changing them needs the right
 * to (ENROLL_SECUREBOOT); asking needs none. /proc/secureboot lists them.
 */

/* A system header, as the musl ones it extends are: warnings in it are not
 * the program's. (scripts/check-libc-overlay.py defines the macro, to hold
 * it to -Wall -Wextra itself.) */
#ifndef _SLATEOS_OVERLAY_WARNINGS
#pragma GCC system_header
#endif

#ifndef _SLATEOS_SECUREBOOT_H
#define _SLATEOS_SECUREBOOT_H 1

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* The lists an entry is enrolled in. */
#define SECUREBOOT_PK 0
#define SECUREBOOT_KEK 1
#define SECUREBOOT_DB 2
#define SECUREBOOT_DBX 3
#define SECUREBOOT_MOK 4

/* A verdict's listing: in db or MOK, in dbx (which wins), in neither. */
#define SECUREBOOT_ALLOWED 0
#define SECUREBOOT_FORBIDDEN 1
#define SECUREBOOT_UNLISTED 2

/* What secureboot_verify says of an image. entry is the id of the entry
 * that decided it, 0 when unlisted; enforced and may_run are 0 or 1. */
struct secureboot_verdict {
	uint32_t listing;
	uint32_t entry;
	uint32_t enforced;
	uint32_t may_run;
};

/* Enrol an entry in a list: a subject (UTF-8, 1 to 256 bytes) and its
 * SHA-256 fingerprint, "SHA256:" and 64 hex digits or the digits alone.
 * The new entry's id, or -1 with errno: EPERM without the right, EEXIST
 * for the same list and fingerprint, EINVAL for a list, subject or
 * fingerprint that is not one, ENOSPC for a full table. */
long secureboot_enroll(int, const char *, size_t, const char *, size_t);

/* Remove the entry with that id: 0, or -1 with errno (EPERM, ENOENT). */
int secureboot_remove(unsigned long);

/* Whether the image named (0 to 256 bytes of any value) with that SHA-256
 * may run: 0 with the verdict filled in, or -1 with errno (EINVAL for a
 * hash that is not one, EFAULT for a verdict that cannot be written). */
int secureboot_verify(const void *, size_t, const char *, size_t, struct secureboot_verdict *);

#ifdef __cplusplus
}
#endif

#endif
