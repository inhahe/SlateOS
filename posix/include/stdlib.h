/*
 * SlateOS: what this C library's <stdlib.h> has that musl's does not declare
 * -- see <math.h> in this directory for why this file exists.
 */

/* A system header, as the musl ones it extends are: warnings in it are not
 * the program's. (scripts/check-libc-overlay.py defines the macro, to hold
 * it to -Wall -Wextra itself.) */
#ifndef _SLATEOS_OVERLAY_WARNINGS
#pragma GCC system_header
#endif

#include_next <stdlib.h>

#ifndef _SLATEOS_STDLIB_H
#define _SLATEOS_STDLIB_H

#include <bits/slateos-features.h>

#ifdef __cplusplus
extern "C" {
#endif

#ifdef _SLATEOS_USE_MISC
/* Random numbers from the kernel's generator, by OpenBSD's names: 32 bits,
 * a buffer's worth, and one below a bound without modulo bias. */
unsigned int arc4random(void);
void         arc4random_buf(void *, size_t);
unsigned int arc4random_uniform(unsigned int);

/* ecvt and fcvt, into the caller's buffer of the given size. */
int ecvt_r(double, int, int *__restrict, int *__restrict, char *__restrict, size_t);
int fcvt_r(double, int, int *__restrict, int *__restrict, char *__restrict, size_t);

/* atexit, whose function is also given the exit status and an argument. */
int on_exit(void (*)(int, void *), void *);
#endif

#ifdef _SLATEOS_USE_MISC
/* int32_t, which glibc's <stdlib.h> has here by including <sys/types.h>. */
#define __NEED_int32_t
#include <bits/alltypes.h>

/* The random family's reentrant forms, over a state array of the caller's
 * that initstate_r lays out and this describes. Zero it before its first
 * initstate_r. */
struct random_data {
	int32_t *fptr;
	int32_t *rptr;
	int32_t *state;
	int rand_type;
	int rand_deg;
	int rand_sep;
	int32_t *end_ptr;
};

int random_r(struct random_data *__restrict, int32_t *__restrict);
int srandom_r(unsigned int, struct random_data *);
int initstate_r(unsigned int, char *__restrict, size_t, struct random_data *__restrict);
int setstate_r(char *__restrict, struct random_data *__restrict);

/* The rand48 family's, over a generator of the caller's. Only zeroed, it is
 * one with X = 0 and POSIX's multiplier and addend. */
struct drand48_data {
	unsigned short __x[3];
	unsigned short __old_x[3];
	unsigned short __c;
	unsigned short __init;
	__extension__ unsigned long long __a;
};

int drand48_r(struct drand48_data *__restrict, double *__restrict);
int erand48_r(unsigned short[3], struct drand48_data *__restrict, double *__restrict);
int lrand48_r(struct drand48_data *__restrict, long *__restrict);
int nrand48_r(unsigned short[3], struct drand48_data *__restrict, long *__restrict);
int mrand48_r(struct drand48_data *__restrict, long *__restrict);
int jrand48_r(unsigned short[3], struct drand48_data *__restrict, long *__restrict);
int srand48_r(long, struct drand48_data *);
int seed48_r(unsigned short[3], struct drand48_data *);
int lcong48_r(unsigned short[7], struct drand48_data *);
#endif

#ifdef _GNU_SOURCE
/* realpath(name, NULL). */
char *canonicalize_file_name(const char *);
#endif

/* ecvt, fcvt and gcvt: glibc declares them by default, musl's header only
 * for _GNU_SOURCE. */
#ifdef _SLATEOS_USE_MISC
char *ecvt(double, int, int *, int *);
char *fcvt(double, int, int *, int *);
char *gcvt(double, int, char *);
#endif

#ifdef _SLATEOS_USE_MISC
/* 4.4BSD's names for strtoll and strtoull. */
long long          strtoq(const char *__restrict, char **__restrict, int);
unsigned long long strtouq(const char *__restrict, char **__restrict, int);
#endif

#ifdef _GNU_SOURCE
/* The conversions in a locale, which is always C's here -- the type
 * spelled as musl's <stdlib.h> spells strtod_l's, not naming locale_t. */
long               strtol_l(const char *__restrict, char **__restrict, int, struct __locale_struct *);
unsigned long      strtoul_l(const char *__restrict, char **__restrict, int, struct __locale_struct *);
long long          strtoll_l(const char *__restrict, char **__restrict, int, struct __locale_struct *);
unsigned long long strtoull_l(const char *__restrict, char **__restrict, int, struct __locale_struct *);
#endif

#ifdef __cplusplus
}
#endif

#endif /* _SLATEOS_STDLIB_H */
