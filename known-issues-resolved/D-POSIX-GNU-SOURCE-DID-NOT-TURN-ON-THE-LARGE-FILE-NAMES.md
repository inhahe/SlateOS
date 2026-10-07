## D-POSIX-GNU-SOURCE-DID-NOT-TURN-ON-THE-LARGE-FILE-NAMES — a C program asking for `_GNU_SOURCE` saw no `off64_t`, `fopen64`, `lseek64` or the other large-file names, which glibc gives it, though `libc.a` defines them (lane D, 2026-10-07) — FIXED 2026-10-07
**Status:** FIXED 2026-10-07 — `posix/include/features.h`, glibc's rule that `_GNU_SOURCE` turns on `_LARGEFILE64_SOURCE`; `<dirent.h>`'s `getdents64` made glibc's under it

**In short:** a C program written for glibc that defines `_GNU_SOURCE` --
most GNU software does -- can use the large-file names: the type
`off64_t`, and `fopen64`, `ftello64`, `lseek64`, `stat64` and the rest.
glibc's `<features.h>` turns `_LARGEFILE64_SOURCE` on whenever
`_GNU_SOURCE` is on. musl's did too until 1.2.4, which kept the names for
an explicit `_LARGEFILE64_SOURCE` only; C here is compiled against musl
1.2.5's headers (zig 0.13's). So such a program here saw none of them,
although this library defines the functions. GDB's build stopped on it:
binutils' bfd finds `ftello64` in `libc.a`, finds no declaration of it,
writes its own -- `extern off64_t ftello64 (FILE *stream);` in
`bfd/sysdep.h` -- and that does not compile, because nothing defines
`off64_t`. Every bfd file that includes it failed.

**How it was found.** By `scripts/gdb-spike/` on its first build of GDB
18.1, 2026-10-07.

**The fix.** `posix/include/features.h`, read before musl's by every
header (each of musl's begins with `#include <features.h>`, and the
overlay is searched first), defines `_LARGEFILE64_SOURCE` when
`_GNU_SOURCE` is defined, as glibc 2.39's does, and includes musl's. The
names are then musl's macros for the standard names -- `#define off64_t
off_t`, `#define fopen64 fopen` -- which on x86-64 is glibc's ABI too: its
64-bit and standard types and functions are the same ones there.

One name needed more. Under `_LARGEFILE64_SOURCE` musl's `<dirent.h>`
makes `getdents64` a macro for its own `getdents`, `int (int, struct
dirent *, size_t)`. glibc's `getdents64` is a function of its own,
`ssize_t (int, void *, size_t)`, which this library defines. The overlay's
`<dirent.h>` now drops musl's macro and declares glibc's function under
`_GNU_SOURCE`, as glibc does. Without `_GNU_SOURCE`, glibc declares no
`getdents64`, and the macro named a function nothing declared either. So
`check-libc-overlay.py`'s exception for it is gone, since its visibility
is now glibc's in every setting.

**Considered and not done:** real declarations of all the large-file
functions in the overlay instead of musl's macros. glibc declares them as
functions, and `&fopen64` would then be a distinct address. But on
x86-64 every one is the standard function under another name, and the
macros already give a glibc program everything it can observe. The
functions stay in `libc.a` for objects compiled against glibc's headers.

**What it could have cost.** Every port that defines `_GNU_SOURCE` and
uses a large-file name took a fallback or failed to compile. The ports
already on the image were compiled before this, against the old
headers. They are relinked, not recompiled, when `libc.a` changes, so
nothing about them changes until each is next built from source.

**Checked by:** `check-libc-overlay.py` (69 headers in 16 settings, 627
declarations where glibc 2.39's are, with glibc's types) and its
self-test; `check-libc-prototypes.py`. A compile of `off64_t`, `fopen64`
and `lseek64` succeeds under `_GNU_SOURCE`, under `_LARGEFILE64_SOURCE`
and under both, and fails under `_DEFAULT_SOURCE` and with no macro, as
with glibc.
