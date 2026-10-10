## D-POSIX-PROTOTYPES-DISAGREED-WITH-THEIR-DEFINITIONS — eleven functions took or returned a different width than musl's headers declare, and `sigset` was declared but never defined (lane D, 2026-09-29) — **Status: FIXED 2026-09-29 -- the definitions changed to the declarations', `sigset` written, and `scripts/check-libc-prototypes.py` refuses a new disagreement (run by `toolchain/build-sysroot.ps1`)**

**In short:** a C program calls this library's functions through musl's
headers, and the linker joins the two by name alone -- nothing checked that
a function takes and returns what its header says. A new check compares the
two for all 1,475 functions both have, by what the x86-64 calling convention
does with each argument, and found eleven that disagreed. None would stop a
program building; each could make one quietly misbehave.

| Function | Header said | Definition was | What a caller got |
|---|---|---|---|
| `timer_create` | `timer_t` is `void *`, 8 bytes | `i32`, 4 | 4 bytes of its 8-byte `timer_t` written, 4 left as they were: comparing two, or one with `NULL`, compared garbage |
| `timer_delete`, `timer_settime`, `timer_gettime`, `timer_getoverrun` | the same | the same | the id read from half the register |
| `wctype`, `wctype_l` | `wctype_t` is `unsigned long` | `u32` | `wctype("x") == 0` tested an upper half nothing had set |
| `wctrans`, `wctrans_l` | `wctrans_t` is `const int *` | `u32` | the same |
| `iswctype`, `iswctype_l`, `towctrans`, `towctrans_l` | take them back at 8 bytes | at 4 | harmless while the handles were small |
| `readahead` | returns `ssize_t` | `i32` | an error's -1 read as 4,294,967,295 bytes |
| `__fpurge` | returns `int` (musl; glibc says `void`) | nothing | a caller testing the result tested an unset register |
| `sigset` | declared (`<signal.h>`) | **not defined** | a program calling it did not link -- and `check-libc-declared.py`, reading its declaration (a function returning a function pointer) as a variable's, never said so |

**Also:** `ioctl`'s request is `int` in musl's header and `unsigned long` in
glibc's; it now reads only the low 32 bits, since a caller of the first
leaves the rest of the register undefined. Five differences remain, each
harmless and named in the gate's `EXCEPTIONS` with why.

**Where:** `posix/src/time.rs`, `wchar.rs`, `file.rs`, `stdio.rs`,
`ioctl.rs`, `signal.rs`; `scripts/check-libc-prototypes.py`,
`scripts/check-libc-declared.py`.
