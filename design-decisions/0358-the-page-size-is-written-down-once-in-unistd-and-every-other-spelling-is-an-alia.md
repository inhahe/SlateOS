## §358 — The page size is written down once, in `unistd`, and every other spelling is an alias of it

**Date:** 2026-08-22
**Decided by:** Claude (autonomous)

**In short:** SlateOS uses 16 KiB memory pages instead of the usual 4 KiB. That
number had been typed out by hand in eight different places in the POSIX
library, so eight different functions each had their own private copy of it. If
somebody ever changed one and missed another, nothing would report an error —
programs would just quietly get memory that one part of the system thinks is
correctly aligned and another part rejects. The number is now written down in
exactly one place and everything else points at it.

### What was there before

`posix/` carried eight independent literal `16384`s, each with its own comment
saying "our 16 KiB page size":

| Where | Spelling |
|---|---|
| `unistd::sysconf(_SC_PAGESIZE)` / `_SC_PAGE_SIZE` | `16384` |
| `unistd::getpagesize()` | `16384` |
| `crt::getauxval(AT_PAGESZ)` | `16384` |
| `linux_stddef::KERNEL_PAGE_SIZE` | `16384` (and *unused*) |
| `sys_param::PAGE_SIZE` / `NBPG` / `PAGE_SHIFT` / `PAGE_MASK` | `16384` / `14` |
| `sysv_shm::SHMLBA` | `16384` |
| `mman::MMAN_PAGE_SIZE` | `16384` |
| `malloc::REGION_ALIGN`, `valloc`, `pvalloc` | `16 * 1024`, `16384`, `16384` |

The page size is an architectural invariant (`CLAUDE.md` → "16 KiB pages, not
4 KiB"), so it is not going to change on a whim — which is exactly the argument
that let eight copies accumulate.

### The decision

One definition, `unistd::PAGE_SIZE`. Every other name above is now
`= crate::unistd::PAGE_SIZE` (or derived from it), and a test named
`every_spelling_of_the_page_size_agrees` asserts they all match, with arms
inside `mman` and `malloc` for the two constants that are private to their
modules.

`unistd` was chosen over the alternatives because it is where the two
POSIX-blessed ways to *ask* for the page size already live — `sysconf(
_SC_PAGESIZE)` and `getpagesize()`. A reader who wants to know what those
return finds the answer beside them.

The alternatives, and why not:

- **`sys_param::PAGE_SIZE` as the canonical one.** It is the most
  "constants-shaped" home, and BSD's `<sys/param.h>` really is where PAGE_SIZE
  lives. But `sys_param` is a header-mirror module — its job is to reproduce a
  foreign header's names, and a header mirror that is also the project's source
  of truth invites the next mirror to define its own.
- **`linux_stddef::KERNEL_PAGE_SIZE`.** Same objection, more so: it mirrors a
  *Linux* header, and the number is not Linux's.
- **A new `page` module.** Cleanest on paper, one more module to find in
  practice, and it would leave `getpagesize()` documented in terms of a constant
  three files away.
- **Leave it alone.** The invariant genuinely is stable, so the drift risk is
  low. Rejected because the cost of a disagreement is not a compile error or an
  `errno` — `valloc` handing back a pointer that `mlock` then rejects as
  unaligned is a caller that silently gets `EINVAL` from an API it used
  correctly. Low probability, but the failure is undiagnosable, and the fix
  costs nothing.

### Side effect: a real dead-code warning goes away

`malloc::REGION_ALIGN` was used only by the two `#[cfg(not(target_os = "none"))]`
host helpers, so the bare-metal build warned `constant REGION_ALIGN is never
used` — while three feet away `valloc` and `pvalloc` hard-coded the same number.
Making them use `REGION_ALIGN` is simultaneously the deduplication and the fix
for the warning, which is the usual shape of this kind of thing: the duplicate
literal and the dead constant were the same defect seen from two sides.

### What stays a literal, on purpose

Three tests still write `16384` out: `unistd`'s
`every_spelling_of_the_page_size_agrees`, `crt::test_getauxval_page_size`, and
`linux_stddef::test_page_size`. That is the point of them — with everything
aliased, a change to `PAGE_SIZE` would otherwise propagate silently and no test
would notice. The tests are where the number is *pinned*; the code is where it
is *derived*.
