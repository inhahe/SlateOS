## D-POSIX-LIBC-LACKS-FUNCTIONS-ITS-HEADERS-DECLARE — 104 functions musl's headers declare do not exist in `libc.a`, so a C program calling one does not link (lane D, 2026-09-28) — **Status: FIXED 2026-09-28 -- all 104 (2026-09-28: C11 `<threads.h>` (posix/src/threads.rs), the pthread cleanup helpers, scheduling attributes, `pthread_setschedprio`, the concurrency hint and default attributes, the nine `_l` functions, `wcsnlen`, `wcswcs`, the seven signal functions; then `ecvt`/`fcvt`/`gcvt` (exact digits, design-decisions §1135), `hcreate_r`/`hsearch_r`/`hdestroy_r`, `tcgetwinsize`/`tcsetwinsize`, `posix_close`, `_Fork`, `ftime`, `stime`, `clock_getcpuclockid`, `ftok`, `lcong48`, `scalb`/`scalbf`, `dlinfo`, and `vhangup`/`acct`/`remap_file_pages` as `ENOSYS` -- the kernel has no such facility; then the account-file functions -- `fgetpwent`, `putpwent`, `fgetgrent`, `putgrent`, `fgetspent`, `sgetspent`, `putspent`, `lckpwdf`, `ulckpwdf`, the `getusershell` three, `cuserid`, `getpass` (design-decisions §1137); then `ns_initparse`, `ns_parserr`, `ns_skiprr`, `ns_name_uncompress` and `pthread_tryjoin_np`/`pthread_timedjoin_np` (D-POSIX-TRYJOIN-CANNOT-SEE-A-KILLED-THREAD); then `getdate`, with glibc's `getdate_r` and POSIX's `getdate_err`, replayed against glibc 2.39; then the `ucontext` four, `getcontext`, `setcontext`, `makecontext` and `swapcontext` (`posix/src/ucontext.rs`, and in C in ring 3 `services/ctest-ucontext`, once lane A runs it: `requests/d-a-run-ctest-ucontext.md`). `scripts/check-libc-declared.py` refuses a new one, and its baseline is empty since the 22 `long double` complex functions of D-POSIX-LIBM-LACKS-GLIBC-EXTENSIONS landed the same day)**

**In short:** C programs here are compiled against musl's headers (`zig cc`)
and linked against our `libc.a`. The headers declare 126 functions the library
does not define -- 22 are the `long double` complex functions
(D-POSIX-LIBM-LACKS-GLIBC-EXTENSIONS) and these 104 are the rest. A program
that calls one compiles and then fails to link. Two are invisible in the
source: musl's `pthread_cleanup_push`/`pthread_cleanup_pop` are macros that
call `_pthread_cleanup_push`/`_pthread_cleanup_pop`, so *any* C program using
cleanup handlers fails to link; and the whole of C11's `<threads.h>` is
missing, so no program written to it links at all.

| Area | Missing |
|---|---|
| C11 threads | `thrd_create` `thrd_current` `thrd_detach` `thrd_equal` `thrd_exit` `thrd_join` `thrd_sleep` `thrd_yield` `mtx_init` `mtx_lock` `mtx_timedlock` `mtx_trylock` `mtx_unlock` `mtx_destroy` `cnd_init` `cnd_signal` `cnd_broadcast` `cnd_wait` `cnd_timedwait` `cnd_destroy` `tss_create` `tss_delete` `tss_get` `tss_set` `call_once` |
| POSIX threads | `_pthread_cleanup_push` `_pthread_cleanup_pop` `pthread_attr_getinheritsched` `pthread_attr_setinheritsched` `pthread_attr_getschedparam` `pthread_attr_setschedparam` `pthread_attr_getschedpolicy` `pthread_attr_setschedpolicy` `pthread_attr_getscope` `pthread_attr_setscope` `pthread_getattr_default_np` `pthread_setattr_default_np` `pthread_getconcurrency` `pthread_setconcurrency` `pthread_setschedprio` `pthread_timedjoin_np` `pthread_tryjoin_np` |
| locale (`_l`) | `strcasecmp_l` `strncasecmp_l` `iswctype_l` `wctype_l` `towctrans_l` `wctrans_l` `wcscasecmp_l` `wcsncasecmp_l` `wcsftime_l` |
| strings | `wcsnlen` `wcswcs` |
| signals (XSI) | `sighold` `sigignore` `sigpause` `sigrelse` `sigandset` `sigorset` `sigisemptyset` |
| accounts | `fgetpwent` `putpwent` `fgetgrent` `putgrent` `fgetspent` `sgetspent` `putspent` `lckpwdf` `ulckpwdf` `getusershell` `setusershell` `endusershell` `cuserid` `getpass` |
| numbers | `ecvt` `fcvt` `gcvt` `lcong48` `scalb` `scalbf` |
| search | `hcreate_r` `hsearch_r` `hdestroy_r` |
| time | `getdate` `stime` `ftime` `clock_getcpuclockid` |
| DNS messages | `ns_initparse` `ns_parserr` `ns_skiprr` `ns_name_uncompress` |
| contexts | `getcontext` `setcontext` `makecontext` `swapcontext` (musl declares them and does not define them either) |
| terminals | `tcgetwinsize` `tcsetwinsize` `vhangup` |
| processes, files, IPC | `_Fork` `posix_close` `acct` `remap_file_pages` `dlinfo` `ftok` |

**Where:** `posix/src/` -- each belongs beside its siblings (`pthread.rs`,
`string.rs`, `wchar.rs`, `signal*.rs`, `pwd.rs`/`grp.rs`, `stdlib.rs`,
`search.rs`, `time.rs`, `termios.rs`, `unistd.rs`).

**How it was found:** preprocess each of the 182 musl headers zig ships, on
its own, with `_GNU_SOURCE`, take every declared function's name, and subtract
the names `libc.a` defines. Six hits are not functions and are left out:
`return` (`tgmath.h`), `volatile` (`sys/io.h`'s inline assembly),
`seqbuf_dump` (a macro's helper in `sys/soundcard.h`), and `cachectl`,
`cacheflush`, `_flush_cache` (MIPS-only `sys/cachectl.h`). No gate does this
today -- `check-libc-abi.py` checks the layouts and numbers of what exists,
not that what is declared exists.

**Proper fix:** implement them, most are a few lines (the `_l` functions
delegate as `isalpha_l` does; `wcsnlen` is `wcslen` with a bound), and add the
subtraction above as a gate with a baseline that can only shrink, so the next
function a header declares cannot go missing unnoticed. `ecvt`/`fcvt`/`gcvt`
need a decision first: glibc's scale by powers of ten in floating point and
so round differently from the exact digits musl's `sprintf`-based versions
give.
