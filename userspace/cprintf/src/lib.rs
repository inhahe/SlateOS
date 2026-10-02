#![deny(clippy::all)]

//! C's numbers, as glibc reads and writes them.
//!
//! Three modules, which lived in `coreutils` until 2026-10-01 and moved here
//! when a program outside that bundle needed them -- `file`, whose magic rules
//! describe what they found with a `printf` format and read their float values
//! with `strtod` and `strtof`. `coreutils` re-exports all three under their old
//! paths, so no utility changed.
//!
//! - [`extfloat`] -- the x87 80-bit `long double` in software: `strtold`,
//!   arithmetic, comparison and `printf`'s `%a %e %f %g`, exact because the
//!   decimal/binary question is answered over integers. It also reads the two
//!   narrower formats, `strtod` and `strtof`, with the same machinery rounded
//!   to 53 and 24 bits as glibc rounds them.
//! - [`cfmt`] -- the C conversions that are not floating point: `%d %i %o %u
//!   %x %X %c %s`, with the flag, width and precision rules around them. It
//!   delegates the floating point ones to [`extfloat`].
//! - [`bignat`] -- the arbitrary-precision naturals the conversions are exact
//!   over.
//!
//! Every answer is measured against glibc rather than recalled: see
//! `scripts/extfloat-diff.sh` and `scripts/printf-diff.sh`.

// Limb and exponent arithmetic is what these modules are: every product
// is formed in a type twice the width of its factors, every carry and borrow
// is explicit, and exponents are bounded by the formats' ranges before they
// are added -- the properties `scripts/extfloat-diff.sh` measures against
// glibc case by case. `arithmetic_side_effects` would flag each line of it.
#![allow(clippy::arithmetic_side_effects)]

pub mod bignat;
pub mod cfmt;
pub mod extfloat;
