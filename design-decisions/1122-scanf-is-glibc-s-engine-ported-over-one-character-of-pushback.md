## 1122. `scanf` is glibc's engine, ported, over one character of pushback

**Date:** 2026-09-27
**Lane:** D
**Decided by:** Claude (autonomous)

**In short:** `scanf` reads text into a program's variables.  Ours read a
whole line from the file descriptor, around the stream, and parsed only that
line (`known-issues.md` → `D-POSIX-SCANF-READS-AROUND-THE-STREAM`).  The fix
had to change how the engine reads -- one character at a time, giving back at
most one -- and every conversion's answer at its edges depends on that.  So
the engine is now glibc's, ported as it is (`vfscanf-internal.c`, byte path,
C locale), and consumes exactly what glibc's consumes; the few places it does
not follow glibc are listed here.

**Why a port and not a repair.**  The old engine looked ahead as far as it
liked in a string (`peek_at`) and backed up (`0xZ`); a stream can give back
one character.  Rebuilding it on one-character pushback means deciding, at
every edge, what is consumed -- which is what glibc's code already decides,
character by character, and what programs have been written against.

**Where it is not glibc's, on purpose:**

- **A NULL destination** stops the scan (a conversion error) for every
  conversion; glibc does that for strings (`STRING_ARG`) and faults for
  numbers.  §1115's rule: a failure where the call can report one.
- **`%l[` over an invalid sequence** is `EILSEQ`; glibc stores whatever
  `mbrtowc` left in the destination.
- **The fast widths** of C23's `wf16` and `wf32` are 32 bits, musl's
  `int_fast16_t` and `int_fast32_t` (§1119); glibc's are 64, and storing 64
  bits into a program's 32-bit variable would overwrite what follows it.
- **`0b` for `%i`** is not taken: glibc takes it only in its C23 entry points
  (`__isoc23_*`), which this library does not provide; `%b` takes it.
- **The `'` and `I` flags** are read and do nothing: the C locale has no
  thousands separator and no other digits.

**How to reverse.**  Each deviation is one branch in `scanf.rs`; the engine
is otherwise glibc's, and a later difference should be judged against its
source, which the comments cite.
