/*
 * SlateOS: <printf.h>, which musl does not have -- glibc's window onto its
 * printf engine: the argument types a format takes, parse_printf_format
 * (posix/src/printf_h.rs).
 */

#ifndef _SLATEOS_OVERLAY_WARNINGS
#pragma GCC system_header
#endif

#ifndef _SLATEOS_PRINTF_H
#define _SLATEOS_PRINTF_H

#include <stdarg.h>
#include <stddef.h>
#include <stdio.h>

#ifdef __cplusplus
extern "C" {
#endif

struct printf_info {
    int prec;                      /* Precision; -1 for none.  */
    int width;                     /* Width.  */
    wchar_t spec;                  /* Format letter.  */
    unsigned int is_long_double:1; /* L flag.  */
    unsigned int is_short:1;       /* h flag.  */
    unsigned int is_long:1;        /* l flag.  */
    unsigned int alt:1;            /* # flag.  */
    unsigned int space:1;          /* Space flag.  */
    unsigned int left:1;           /* - flag.  */
    unsigned int showsign:1;       /* + flag.  */
    unsigned int group:1;          /* ' flag.  */
    unsigned int extra:1;          /* For special use.  */
    unsigned int is_char:1;        /* hh flag.  */
    unsigned int wide:1;           /* Nonzero for wide character streams.  */
    unsigned int i18n:1;           /* I flag.  */
    unsigned int is_binary128:1;   /* The argument is a binary128.  */
    unsigned int __pad:3;          /* Unused so far.  */
    unsigned short int user;       /* Bits for user-installed modifiers.  */
    wchar_t pad;                   /* Padding character.  */
};

typedef int printf_function(FILE *__stream, const struct printf_info *__info,
                            const void *const *__args);
typedef int printf_arginfo_size_function(const struct printf_info *__info, size_t __n,
                                         int *__argtypes, int *__size);
typedef int printf_arginfo_function(const struct printf_info *__info, size_t __n,
                                    int *__argtypes);
typedef void printf_va_arg_function(void *__mem, va_list *__ap);

size_t parse_printf_format(const char *__restrict __fmt, size_t __n,
                           int *__restrict __argtypes);

enum {
    PA_INT,     /* int */
    PA_CHAR,    /* int, cast to char */
    PA_WCHAR,   /* wide char */
    PA_STRING,  /* const char *, a '\0'-terminated string */
    PA_WSTRING, /* const wchar_t *, wide character string */
    PA_POINTER, /* void * */
    PA_FLOAT,   /* float */
    PA_DOUBLE,  /* double */
    PA_LAST
};

#define PA_FLAG_MASK 0xff00
#define PA_FLAG_LONG_LONG (1 << 8)
#define PA_FLAG_LONG_DOUBLE PA_FLAG_LONG_LONG
#define PA_FLAG_LONG (1 << 9)
#define PA_FLAG_SHORT (1 << 10)
#define PA_FLAG_PTR (1 << 11)

#ifdef __cplusplus
}
#endif

#endif /* _SLATEOS_PRINTF_H */
