/*
 * SlateOS: <stdbit.h> -- C23's bit utilities (C23 7.18), which musl does not
 * have: fourteen questions about an unsigned integer's bits, a function each
 * for the five standard unsigned types, and the type-generic macros that
 * choose among them. The functions are posix/src/stdbit.rs; glibc 2.39's
 * header declares the same, whatever the feature macros.
 */

/* A system header, as the musl ones beside it are: warnings in it are not
 * the program's. (scripts/check-libc-overlay.py defines the macro, to hold
 * it to -Wall -Wextra itself.) */
#ifndef _SLATEOS_OVERLAY_WARNINGS
#pragma GCC system_header
#endif

#ifndef _STDBIT_H
#define _STDBIT_H 1

#ifndef __cplusplus
#include <stdbool.h>
#endif

#define __STDC_VERSION_STDBIT_H__ 202311L

/* The byte orders, as the compiler reports them: 1234 and 4321, and x86-64's
 * the first. */
#define __STDC_ENDIAN_LITTLE__ __ORDER_LITTLE_ENDIAN__
#define __STDC_ENDIAN_BIG__ __ORDER_BIG_ENDIAN__
#define __STDC_ENDIAN_NATIVE__ __BYTE_ORDER__

#ifdef __cplusplus
extern "C" {
#endif


/* How many 0 bits are above the highest 1 (all, for 0). */
unsigned int stdc_leading_zeros_uc(unsigned char);
unsigned int stdc_leading_zeros_us(unsigned short);
unsigned int stdc_leading_zeros_ui(unsigned int);
unsigned int stdc_leading_zeros_ul(unsigned long);
unsigned int stdc_leading_zeros_ull(unsigned long long);

/* How many 1 bits are above the highest 0. */
unsigned int stdc_leading_ones_uc(unsigned char);
unsigned int stdc_leading_ones_us(unsigned short);
unsigned int stdc_leading_ones_ui(unsigned int);
unsigned int stdc_leading_ones_ul(unsigned long);
unsigned int stdc_leading_ones_ull(unsigned long long);

/* How many 0 bits are below the lowest 1 (all, for 0). */
unsigned int stdc_trailing_zeros_uc(unsigned char);
unsigned int stdc_trailing_zeros_us(unsigned short);
unsigned int stdc_trailing_zeros_ui(unsigned int);
unsigned int stdc_trailing_zeros_ul(unsigned long);
unsigned int stdc_trailing_zeros_ull(unsigned long long);

/* How many 1 bits are below the lowest 0. */
unsigned int stdc_trailing_ones_uc(unsigned char);
unsigned int stdc_trailing_ones_us(unsigned short);
unsigned int stdc_trailing_ones_ui(unsigned int);
unsigned int stdc_trailing_ones_ul(unsigned long);
unsigned int stdc_trailing_ones_ull(unsigned long long);

/* Where the highest 0 is, the top bit 1; 0 if none. */
unsigned int stdc_first_leading_zero_uc(unsigned char);
unsigned int stdc_first_leading_zero_us(unsigned short);
unsigned int stdc_first_leading_zero_ui(unsigned int);
unsigned int stdc_first_leading_zero_ul(unsigned long);
unsigned int stdc_first_leading_zero_ull(unsigned long long);

/* Where the highest 1 is, the top bit 1; 0 if none. */
unsigned int stdc_first_leading_one_uc(unsigned char);
unsigned int stdc_first_leading_one_us(unsigned short);
unsigned int stdc_first_leading_one_ui(unsigned int);
unsigned int stdc_first_leading_one_ul(unsigned long);
unsigned int stdc_first_leading_one_ull(unsigned long long);

/* Where the lowest 0 is, the bottom bit 1; 0 if none. */
unsigned int stdc_first_trailing_zero_uc(unsigned char);
unsigned int stdc_first_trailing_zero_us(unsigned short);
unsigned int stdc_first_trailing_zero_ui(unsigned int);
unsigned int stdc_first_trailing_zero_ul(unsigned long);
unsigned int stdc_first_trailing_zero_ull(unsigned long long);

/* Where the lowest 1 is, the bottom bit 1; 0 if none. */
unsigned int stdc_first_trailing_one_uc(unsigned char);
unsigned int stdc_first_trailing_one_us(unsigned short);
unsigned int stdc_first_trailing_one_ui(unsigned int);
unsigned int stdc_first_trailing_one_ul(unsigned long);
unsigned int stdc_first_trailing_one_ull(unsigned long long);

/* How many 0 bits there are. */
unsigned int stdc_count_zeros_uc(unsigned char);
unsigned int stdc_count_zeros_us(unsigned short);
unsigned int stdc_count_zeros_ui(unsigned int);
unsigned int stdc_count_zeros_ul(unsigned long);
unsigned int stdc_count_zeros_ull(unsigned long long);

/* How many 1 bits there are. */
unsigned int stdc_count_ones_uc(unsigned char);
unsigned int stdc_count_ones_us(unsigned short);
unsigned int stdc_count_ones_ui(unsigned int);
unsigned int stdc_count_ones_ul(unsigned long);
unsigned int stdc_count_ones_ull(unsigned long long);

/* Whether it is a power of two. */
bool stdc_has_single_bit_uc(unsigned char);
bool stdc_has_single_bit_us(unsigned short);
bool stdc_has_single_bit_ui(unsigned int);
bool stdc_has_single_bit_ul(unsigned long);
bool stdc_has_single_bit_ull(unsigned long long);

/* How many bits it needs. */
unsigned int stdc_bit_width_uc(unsigned char);
unsigned int stdc_bit_width_us(unsigned short);
unsigned int stdc_bit_width_ui(unsigned int);
unsigned int stdc_bit_width_ul(unsigned long);
unsigned int stdc_bit_width_ull(unsigned long long);

/* The largest power of two not above it; 0 for 0. */
unsigned char stdc_bit_floor_uc(unsigned char);
unsigned short stdc_bit_floor_us(unsigned short);
unsigned int stdc_bit_floor_ui(unsigned int);
unsigned long stdc_bit_floor_ul(unsigned long);
unsigned long long stdc_bit_floor_ull(unsigned long long);

/* The smallest power of two not below it, or 0 if that does not fit. */
unsigned char stdc_bit_ceil_uc(unsigned char);
unsigned short stdc_bit_ceil_us(unsigned short);
unsigned int stdc_bit_ceil_ui(unsigned int);
unsigned long stdc_bit_ceil_ul(unsigned long);
unsigned long long stdc_bit_ceil_ull(unsigned long long);

#ifdef __cplusplus
}
#endif

/* The type-generic forms: the function for the argument's type, or the
 * compiler's own form where it has one, which takes a bit-precise
 * (_BitInt) type as well. Without one, an argument of any type but the five
 * -- a signed one, bool, an unsigned _BitInt -- does not compile. */
#ifndef __cplusplus
#define _SLATEOS_STDBIT(f, x) _Generic((x), unsigned char: f##_uc, unsigned short: f##_us, \
	unsigned int: f##_ui, unsigned long: f##_ul, unsigned long long: f##_ull)(x)
#if defined(__has_builtin)
#if __has_builtin(__builtin_stdc_leading_zeros)
#define _SLATEOS_STDBIT_BUILTINS 1
#endif
#endif

#ifdef _SLATEOS_STDBIT_BUILTINS
#define stdc_leading_zeros(x) (__builtin_stdc_leading_zeros(x))
#define stdc_leading_ones(x) (__builtin_stdc_leading_ones(x))
#define stdc_trailing_zeros(x) (__builtin_stdc_trailing_zeros(x))
#define stdc_trailing_ones(x) (__builtin_stdc_trailing_ones(x))
#define stdc_first_leading_zero(x) (__builtin_stdc_first_leading_zero(x))
#define stdc_first_leading_one(x) (__builtin_stdc_first_leading_one(x))
#define stdc_first_trailing_zero(x) (__builtin_stdc_first_trailing_zero(x))
#define stdc_first_trailing_one(x) (__builtin_stdc_first_trailing_one(x))
#define stdc_count_zeros(x) (__builtin_stdc_count_zeros(x))
#define stdc_count_ones(x) (__builtin_stdc_count_ones(x))
#define stdc_has_single_bit(x) (__builtin_stdc_has_single_bit(x))
#define stdc_bit_width(x) (__builtin_stdc_bit_width(x))
#define stdc_bit_floor(x) (__builtin_stdc_bit_floor(x))
#define stdc_bit_ceil(x) (__builtin_stdc_bit_ceil(x))
#else
#define stdc_leading_zeros(x) _SLATEOS_STDBIT(stdc_leading_zeros, x)
#define stdc_leading_ones(x) _SLATEOS_STDBIT(stdc_leading_ones, x)
#define stdc_trailing_zeros(x) _SLATEOS_STDBIT(stdc_trailing_zeros, x)
#define stdc_trailing_ones(x) _SLATEOS_STDBIT(stdc_trailing_ones, x)
#define stdc_first_leading_zero(x) _SLATEOS_STDBIT(stdc_first_leading_zero, x)
#define stdc_first_leading_one(x) _SLATEOS_STDBIT(stdc_first_leading_one, x)
#define stdc_first_trailing_zero(x) _SLATEOS_STDBIT(stdc_first_trailing_zero, x)
#define stdc_first_trailing_one(x) _SLATEOS_STDBIT(stdc_first_trailing_one, x)
#define stdc_count_zeros(x) _SLATEOS_STDBIT(stdc_count_zeros, x)
#define stdc_count_ones(x) _SLATEOS_STDBIT(stdc_count_ones, x)
#define stdc_has_single_bit(x) _SLATEOS_STDBIT(stdc_has_single_bit, x)
#define stdc_bit_width(x) _SLATEOS_STDBIT(stdc_bit_width, x)
#define stdc_bit_floor(x) _SLATEOS_STDBIT(stdc_bit_floor, x)
#define stdc_bit_ceil(x) _SLATEOS_STDBIT(stdc_bit_ceil, x)
#endif
#endif /* !__cplusplus */

#endif /* _STDBIT_H */
