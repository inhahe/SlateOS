## 1152. `<sys/timex.h>`'s `struct ntptimeval` is glibc's, and musl's is renamed out of the way while its header is read

**Date:** 2026-09-30
**Decided by:** Claude (autonomous)
**Lane:** D

**In short:** a program that asks the clock for its time and error bounds
with `ntp_gettimex` gets them in a `struct ntptimeval`, and on glibc that
struct also carries the TAI offset (how many leap seconds atomic time is
ahead of UTC) and four reserved words. musl's header defines the struct as
it was before the TAI offset -- three fields -- so a glibc program that
reads `tai` did not compile here, and a 72-byte answer written into musl's
32-byte struct would run 40 bytes past it. The overlay's `<sys/timex.h>`
now gives C glibc's struct. C lets a struct be defined only once, so while
musl's header is read, its struct is given another name, which nothing
refers to.

| | before | now |
|---|---|---|
| `struct ntptimeval` | musl's: `time`, `maxerror`, `esterror` (32 bytes) | glibc's: those, `tai`, and four reserved words (72 bytes) |
| `ntp_gettimex` | missing | fills all of it, the reserved words 0 |
| `ntp_gettime` | missing | in C, `ntp_gettimex` under that name, as glibc's header makes it; the library's own `ntp_gettime` fills the older three fields, for what calls it by name, as glibc's does |

**The alternatives:** replace musl's `<sys/timex.h>` outright, as the
overlay's `<glob.h>` replaces musl's -- the overlay would then carry musl's
`struct timex` and its hundred-odd `ADJ_`, `STA_` and `TIME_` constants
itself, and have to keep them in step with musl's; or keep musl's struct and
declare `ntp_gettimex` over it, which leaves glibc programs that read `tai`
uncompilable and every call writing past the caller's struct. The rename is
sound here because musl declares nothing that takes its struct: the name
`__slateos_musl_ntptimeval` is never used again, and `check-libc-overlay.py`
holds the struct that is used to glibc's layout. `<glob.h>` could not be
done this way -- musl's own `glob` and `globfree` are declared with its
`glob_t`.

**Where:** `posix/include/sys/timex.h`; `posix/src/sys_timex.rs`
(`NtpTimeval`, `ntp_gettime`, `ntp_gettimex`).
