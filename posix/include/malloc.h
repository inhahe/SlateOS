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

/* FILE, for malloc_info: glibc's <malloc.h> includes <stdio.h>. */
#include <stdio.h>

/* mallopt's parameters: the SVID ones, of which glibc's malloc and this one
 * use only M_MXFAST (and here it changes nothing: there are no fastbins)... */
#ifndef M_MXFAST
#define M_MXFAST 1 /* the largest request a fastbin serves */
#endif
#ifndef M_NLBLKS
#define M_NLBLKS 2 /* unused */
#endif
#ifndef M_GRAIN
#define M_GRAIN 3 /* unused */
#endif
#ifndef M_KEEP
#define M_KEEP 4 /* unused */
#endif

/* ...and glibc's. */
#define M_TRIM_THRESHOLD -1 /* free bytes at the top that prompt a trim */
#define M_TOP_PAD -2        /* the extra taken from the system each time */
#define M_MMAP_THRESHOLD -3 /* the size from which a block is mapped alone */
#define M_MMAP_MAX -4       /* how many blocks may be mapped alone at once */
#define M_CHECK_ACTION -5   /* what a corrupted heap does (ignored) */
#define M_PERTURB -6        /* the byte new and freed blocks are filled with */
#define M_ARENA_TEST -7     /* arenas before M_ARENA_MAX is consulted */
#define M_ARENA_MAX -8      /* the most arenas there may be */

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

/* realloc of an array, refusing an overflowing size: glibc declares it here
 * as in <stdlib.h>; musl only there. */
void *reallocarray(void *, size_t, size_t);

/* Set a parameter: 1, or 0 when it is refused. */
int mallopt(int, int);
/* The heap's state as XML on the stream: 0, or EINVAL for options but 0. */
int malloc_info(int, FILE *);

#ifdef __cplusplus
}
#endif

#endif /* _SLATEOS_MALLOC_H */
