/*
 * SlateOS: what this C library's <malloc.h> has that musl's does not declare
 * -- see <math.h> in this directory for why this file exists.
 */

/* A system header, as the musl ones it extends are: warnings in it are not
 * the program's. (scripts/check-libc-overlay.py defines the macro, to hold
 * it to -Wall -Wextra itself.) */
#ifndef _SLATEOS_OVERLAY_WARNINGS
#pragma GCC system_header
#endif

#include_next <malloc.h>

#ifndef _SLATEOS_MALLOC_H
#define _SLATEOS_MALLOC_H

#include <bits/slateos-features.h>

#ifdef __cplusplus
extern "C" {
#endif

/* The heap's statistics, in ints: each saturates where the heap is larger. */
struct mallinfo {
	int arena;    /* bytes in the heap's segments */
	int ordblks;  /* free chunks */
	int smblks;   /* free fastbin blocks: none, here */
	int hblks;    /* separately mapped blocks */
	int hblkhd;   /* bytes in separately mapped blocks */
	int usmblks;  /* the most the heap has held */
	int fsmblks;  /* bytes in free fastbin blocks: none, here */
	int uordblks; /* bytes in use */
	int fordblks; /* bytes free */
	int keepcost; /* bytes malloc_trim could release */
};

/* The same, in size_t. */
struct mallinfo2 {
	size_t arena;
	size_t ordblks;
	size_t smblks;
	size_t hblks;
	size_t hblkhd;
	size_t usmblks;
	size_t fsmblks;
	size_t uordblks;
	size_t fordblks;
	size_t keepcost;
};

struct mallinfo mallinfo(void) _SLATEOS_DEPRECATED("mallinfo is deprecated: use mallinfo2");
struct mallinfo2 mallinfo2(void);
/* Print the statistics to stderr. */
void malloc_stats(void);
/* Give free memory back to the system, keeping the argument's bytes; 1 if
 * any went back. */
int malloc_trim(size_t);
/* valloc, rounded up to whole pages. */
void *pvalloc(size_t);

#ifdef __cplusplus
}
#endif

#endif /* _SLATEOS_MALLOC_H */
