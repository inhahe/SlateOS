/*
 * SlateOS: what this C library's <math.h> has that musl's does not declare.
 *
 * C here is compiled against musl's headers, and musl's declare only what
 * musl defines. This library defines more -- glibc's extensions, C23's
 * additions -- and a C program cannot call a function no header declares.
 * So each header in this directory stands in front of musl's, includes it,
 * and declares the rest, each under the feature macros glibc 2.39 declares
 * it under. In front means `-I posix/include`: zig's driver searches its own
 * libc headers before any `-isystem` directory, which would put these
 * behind the headers they extend.
 * scripts/check-libc-prototypes.py checks every declaration here against
 * the function's definition, and scripts/check-libc-overlay.py checks each
 * against glibc's: its type, and the feature macros it appears under.
 */

/* A system header, as the musl ones it extends are: warnings in it are not
 * the program's. (scripts/check-libc-overlay.py defines the macro, to hold
 * it to -Wall -Wextra itself.) */
#ifndef _SLATEOS_OVERLAY_WARNINGS
#pragma GCC system_header
#endif

#include_next <math.h>

#ifndef _SLATEOS_MATH_H
#define _SLATEOS_MATH_H

#include <bits/slateos-features.h>

#ifdef __cplusplus
extern "C" {
#endif

#ifdef _SLATEOS_USE_MISC
/* The long double Bessel functions. */
long double j0l(long double);
long double j1l(long double);
long double jnl(int, long double);
long double y0l(long double);
long double y1l(long double);
long double ynl(int, long double);

/* The BSD and System V names. */
long double dreml(long double, long double);
int         finitel(long double);
long double significandl(long double);
long double scalbl(long double, long double);
double      gamma(double);
float       gammaf(float);
long double gammal(long double);
int         isinff(float);
int         isinfl(long double);
int         isnanf(float);
int         isnanl(long double);
#endif

#ifdef _SLATEOS_USE_BFP_EXT_C23
/* The rounding directions of fromfp and its family. */
#define FP_INT_UPWARD 0
#define FP_INT_DOWNWARD 1
#define FP_INT_TOWARDZERO 2
#define FP_INT_TONEARESTFROMZERO 3
#define FP_INT_TONEAREST 4

/* What llogb returns for 0 and for a NaN: ilogb's FP_ILOGB0 and
 * FP_ILOGBNAN, INT_MIN both, widened. */
#define FP_LLOGB0 (-0x7fffffffffffffffL - 1)
#define FP_LLOGBNAN (-0x7fffffffffffffffL - 1)

double      nextup(double);
float       nextupf(float);
long double nextupl(long double);
double      nextdown(double);
float       nextdownf(float);
long double nextdownl(long double);

long llogb(double);
long llogbf(float);
long llogbl(long double);

double      roundeven(double);
float       roundevenf(float);
long double roundevenl(long double);

int canonicalize(double *, const double *);
int canonicalizef(float *, const float *);
int canonicalizel(long double *, const long double *);

/* The integer results TS 18661-1 gives them, as glibc 2.39 returns them
 * (C23 has them return the floating type): intmax_t and uintmax_t, which
 * are long and unsigned long here. */
long          fromfp(double, int, unsigned int);
long          fromfpf(float, int, unsigned int);
long          fromfpl(long double, int, unsigned int);
long          fromfpx(double, int, unsigned int);
long          fromfpxf(float, int, unsigned int);
long          fromfpxl(long double, int, unsigned int);
unsigned long ufromfp(double, int, unsigned int);
unsigned long ufromfpf(float, int, unsigned int);
unsigned long ufromfpl(long double, int, unsigned int);
unsigned long ufromfpx(double, int, unsigned int);
unsigned long ufromfpxf(float, int, unsigned int);
unsigned long ufromfpxl(long double, int, unsigned int);

/* The narrowing operations: one rounding, to the narrower type. */
float  fadd(double, double);
float  faddl(long double, long double);
double daddl(long double, long double);
float  fsub(double, double);
float  fsubl(long double, long double);
double dsubl(long double, long double);
float  fmul(double, double);
float  fmull(long double, long double);
double dmull(long double, long double);
float  fdiv(double, double);
float  fdivl(long double, long double);
double ddivl(long double, long double);
float  fsqrt(double);
float  fsqrtl(long double);
double dsqrtl(long double);
float  ffma(double, double, double);
float  ffmal(long double, long double, long double);
double dfmal(long double, long double, long double);
#endif

#ifdef _SLATEOS_USE_C23
/* C23's maximum and minimum operations. */
double      fmaximum(double, double);
float       fmaximumf(float, float);
long double fmaximuml(long double, long double);
double      fminimum(double, double);
float       fminimumf(float, float);
long double fminimuml(long double, long double);
double      fmaximum_mag(double, double);
float       fmaximum_magf(float, float);
long double fmaximum_magl(long double, long double);
double      fminimum_mag(double, double);
float       fminimum_magf(float, float);
long double fminimum_magl(long double, long double);
double      fmaximum_num(double, double);
float       fmaximum_numf(float, float);
long double fmaximum_numl(long double, long double);
double      fminimum_num(double, double);
float       fminimum_numf(float, float);
long double fminimum_numl(long double, long double);
double      fmaximum_mag_num(double, double);
float       fmaximum_mag_numf(float, float);
long double fmaximum_mag_numl(long double, long double);
double      fminimum_mag_num(double, double);
float       fminimum_mag_numf(float, float);
long double fminimum_mag_numl(long double, long double);
#endif

#ifdef _SLATEOS_USE_IEC_60559_EXT
/* NaN payloads and the total order: C23's Annex F. */
double      getpayload(const double *);
float       getpayloadf(const float *);
long double getpayloadl(const long double *);
int setpayload(double *, double);
int setpayloadf(float *, float);
int setpayloadl(long double *, long double);
int setpayloadsig(double *, double);
int setpayloadsigf(float *, float);
int setpayloadsigl(long double *, long double);
int totalorder(const double *, const double *);
int totalorderf(const float *, const float *);
int totalorderl(const long double *, const long double *);
int totalordermag(const double *, const double *);
int totalordermagf(const float *, const float *);
int totalordermagl(const long double *, const long double *);
#endif

#ifdef _SLATEOS_USE_BFP_EXT
/* TS 18661-1's maximum and minimum magnitude, which C23 replaced with
 * fmaximum_mag_num and fminimum_mag_num. */
double      fmaxmag(double, double);
float       fmaxmagf(float, float);
long double fmaxmagl(long double, long double);
double      fminmag(double, double);
float       fminmagf(float, float);
long double fminmagl(long double, long double);
#endif

#ifdef __cplusplus
}
#endif

#endif /* _SLATEOS_MATH_H */
