//! POSIX compatibility library for the OS.
//!
//! Provides C-compatible function signatures (`extern "C"`) for standard
//! POSIX operations, backed by our native syscall interface.  Userspace
//! programs written in C (via our cross-toolchain) or Rust can link
//! against this library to get familiar POSIX semantics.
//!
//! ## Design
//!
//! This is a thin translation layer, not a full libc.  It maps POSIX
//! function signatures to our native syscalls with minimal overhead:
//!
//! - **File I/O**: `open`, `close`, `read`, `write`, `lseek`, `stat`,
//!   `fstat`, `lstat`, `fstatat`, `creat`, `unlink`, `mkdir`, `rmdir`,
//!   `rename`, `dup`, `dup2`, `dup3`, `access`, `chmod`, `fchmod`,
//!   `chown`, `fchown`, `lchown`, `umask`, `truncate`, `ftruncate`,
//!   `fsync`, `fdatasync`, `link`, `symlink`, `readlink`, `utimes`,
//!   `futimes`, `utimensat`, `futimens`, `sendfile`, `sendfile64`,
//!   `fallocate`, `splice`, `tee`, `vmsplice`, `mknod`, `mkfifo`
//! - **Sockets**: `socket`, `connect`, `bind`, `listen`, `accept`,
//!   `send`, `recv`, `sendto`, `recvfrom`, `shutdown`, `setsockopt`,
//!   `getsockopt`, `getpeername`, `getsockname`, `getaddrinfo`,
//!   `freeaddrinfo`, `getnameinfo`, `gethostbyname`, `gethostbyname2`,
//!   `htons`, `htonl`, `inet_addr`, `inet_ntoa`, `inet_aton`,
//!   `inet_pton`, `inet_ntop`
//! - **I/O Multiplexing**: `poll`, `select`, `pselect`
//! - **Terminal**: `ioctl` (TIOCGWINSZ, TCGETS, FIONBIO, etc.),
//!   `isatty`, `ttyname`, `tcgetattr`, `tcsetattr`, `cfmakeraw`,
//!   `cfsetspeed`, `tcsendbreak`, `tcdrain`, `tcflow`, `tcflush`,
//!   termios flags, `posix_openpt`, `grantpt`, `unlockpt`, `ptsname`,
//!   `ptsname_r`
//! - **Process**: `_exit`, `getpid`, `getppid`, `posix_spawn`,
//!   `posix_spawnp`, `execve`, `execvp`, `execv`, `fexecve`, `vfork`,
//!   `waitpid`, `sleep`, `nanosleep`, `getpgrp`, `setpgid`, `setsid`,
//!   `getsid`, `pidfd_open`, `pidfd_send_signal`, `pidfd_getfd`,
//!   `issetugid`, `posix_spawn_file_actions_addchdir_np`
//! - **Memory**: `mmap`, `munmap`, `mprotect`, `mmap64`, `mremap`,
//!   `mlock`/`mlock2`/`munlock`/`mlockall`/`munlockall`, `msync`, `madvise`,
//!   `posix_madvise`, `shm_open`/`shm_unlink`, `memfd_create`
//! - **Pipes**: `pipe`, `pipe2`
//! - **Signals**: Stub constants and handlers (partial), `sigwait`,
//!   `sigtimedwait`, `sigqueue`, `sigaltstack`, `siginterrupt`
//! - **Threads**: `pthread` stubs, working mutex ops
//! - **C Standard Library**: `malloc`/`free`/`calloc`/`realloc`,
//!   `posix_memalign`/`aligned_alloc`/`valloc`/`memalign`/`reallocarray`,
//!   `malloc_usable_size`,
//!   `setjmp`/`longjmp`/`sigsetjmp`/`siglongjmp`, `qsort`, `bsearch`,
//!   `atoi`/`atol`/`atoll`/`strtol`/`strtoul`,
//!   `random`/`srandom`/`initstate`/`setstate`,
//!   `drand48`/`lrand48`/`mrand48`/`srand48`/`seed48`/`nrand48`/`erand48`/`jrand48`,
//!   `mktemp`,
//!   `puts`/`fputs`/`fwrite`/`fread`/`perror`, ctype classification,
//!   `__ctype_b_loc`/`__ctype_tolower_loc`/`__ctype_toupper_loc`,
//!   `__ctype_get_mb_cur_max`
//! - **Formatted Output**: `printf`, `fprintf`, `dprintf`, `sprintf`,
//!   `snprintf`, `asprintf` (via assembly trampoline for C variadic capture)
//! - **Formatted Input**: `sscanf`, `scanf`, `fscanf` (string/stdin/stream
//!   scanning with `%d`/`%u`/`%x`/`%o`/`%s`/`%c`/`%f`/`%n`/`%[...]`,
//!   width limits, assignment suppression)
//! - **Pattern Matching**: `fnmatch` (shell wildcards), `glob`/`globfree`
//!   (pathname expansion), `wordexp`/`wordfree` (word expansion)
//! - **Character Encoding**: `iconv_open`, `iconv`, `iconv_close`
//!   (UTF-8/ASCII conversions)
//! - **Resource Limits**: `getrlimit`, `setrlimit`, `getrusage`,
//!   `prlimit`/`prlimit64`
//! - **Timers**: `timer_create`, `timer_settime`, `timer_gettime`,
//!   `timer_delete`, `timer_getoverrun` (stubs — no signal delivery),
//!   `setitimer`/`getitimer`
//! - **System**: `uname`
//! - **Logging**: `openlog`, `syslog`, `closelog`, `setlogmask`
//! - **User/Group**: `getpwnam`, `getpwuid`, `getgrnam`, `getgrgid`,
//!   `getlogin`, password/group enumeration
//! - **Math**: `fabs`, `floor`, `ceil`, `round`, `trunc`, `fmod`,
//!   `sqrt`, `cbrt`, `hypot`, `pow`, `exp`/`exp2`/`expm1`/`exp10`,
//!   `log`/`log2`/`log10`/`log1p`, `sin`, `cos`, `tan`, `sincos`,
//!   `asin`, `acos`, `atan`, `atan2`, `sinh`, `cosh`, `tanh`,
//!   `asinh`, `acosh`, `atanh`,
//!   `frexp`, `ldexp`, `modf`, `scalbn`, `ilogb`, `logb`,
//!   `isnan`, `isinf`, `isfinite`, `copysign`, `fmin`, `fmax`,
//!   `fdim`, `fma`, `remainder`, `remquo`, `rint`, `nearbyint`,
//!   `nextafter`, `erf`, `erfc`, `lgamma`, `lgamma_r`, `tgamma`,
//!   `j0`, `j1`, `jn`, `y0`, `y1`, `yn` (Bessel)
//!   (and `f32` variants)
//! - **Wide Characters** (full UTF-8): `mblen`, `mbtowc`, `wctomb`,
//!   `mbstowcs`, `wcstombs`, `btowc`, `wctob`, `mbsinit`, `mbrtowc`,
//!   `wcrtomb`, `mbrlen`, `wcwidth`, `wcswidth`, `iswalnum`..`iswxdigit`,
//!   `towlower`, `towupper`, `wctype`, `iswctype`, `wctrans`, `towctrans`,
//!   `wcscpy`, `wcsncpy`, `wcslen`, `wcscmp`, `wcsncmp`, `wcscat`,
//!   `wcsncat`, `wcschr`, `wcsrchr`, `wcsstr`, `wcsdup`,
//!   `wcsspn`, `wcscspn`, `wcspbrk`, `wcstok`,
//!   `wcstol`, `wcstoul`, `wcstoll`, `wcstoull`, `wcstod`, `wcstof`,
//!   `wmemcpy`, `wmemset`, `wmemcmp`, `wmemchr`, `wmemmove`,
//!   `mbsrtowcs`, `mbsnrtowcs`, `wcsrtombs`, `wcsnrtombs`,
//!   `nl_langinfo`
//! - **File Tree Walk**: `ftw`, `nftw` (recursive directory traversal)
//! - **BSD Error Functions**: `err`, `errx`, `warn`, `warnx` (and `v*`
//!   variants)
//! - **User Accounting** (stubs): `setutxent`, `getutxent`, `getutxid`,
//!   `getutxline`, `pututxline`, `endutxent`, `utmpxname`
//!   (and glibc aliases `setutent`, `getutent`, etc.)
//! - **Timezone**: `tzset`, `tzname`, `timezone`, `daylight`
//! - **Extended Attributes** (stubs): `getxattr`, `lgetxattr`, `fgetxattr`,
//!   `setxattr`, `lsetxattr`, `fsetxattr`, `listxattr`, `llistxattr`,
//!   `flistxattr`, `removexattr`, `lremovexattr`, `fremovexattr`
//! - **Misc**: `getcwd`, `chdir`, `realpath`, `errno`, `sysconf`,
//!   `getenv`/`setenv`, `pread`, `pwrite`, `readv`, `writev`,
//!   `basename`, `dirname`, `getopt`/`getopt_long`/`getopt_long_only`,
//!   `pathconf`, `confstr`, `strlcpy`, `strlcat`, `mkdtemp`, `flock`,
//!   `setgroups`, `sigaltstack`, `siginterrupt`,
//!   `daemon`, `getloadavg`, `sync`, `syncfs`, `sethostname`, `chroot`,
//!   `flockfile`/`funlockfile`/`ftrylockfile`, `if_nametoindex`,
//!   `if_indextoname`, `ppoll`, `putenv`, `strcasestr`,
//!   `explicit_bzero`, `strtoimax`/`strtoumax`, `getrandom`,
//!   `getentropy`, `clock_nanosleep`, `clock_settime`,
//!   `fchdir` (via path tracking), `getdomainname`/`setdomainname`,
//!   `getdtablesize`, `preadv2`/`pwritev2`, `fadvise64`,
//!   `arch_prctl`, `ioprio_get`/`ioprio_set`, `membarrier`,
//!   `readahead`, `sync_file_range`, `name_to_handle_at`,
//!   `open_by_handle_at`
//! - **Dynamic Linking** (stubs): `dlopen`, `dlsym`, `dlclose`, `dlerror`,
//!   `dladdr`, `dl_iterate_phdr`, `__tls_get_addr`
//! - **Directories**: `opendir`, `closedir`, `readdir`, `rewinddir`,
//!   `seekdir`, `telldir`, `scandir`, `alphasort`, `versionsort`,
//!   `readdir_r`, `fdopendir` (via path tracking), `dirfd`
//! - **File Mode Testing**: `S_ISREG`, `S_ISDIR`, `S_ISLNK`, `S_ISCHR`,
//!   `S_ISBLK`, `S_ISFIFO`, `S_ISSOCK`, `mknod`/`mknodat`,
//!   `mkfifo`/`mkfifoat`
//! - **LP64 Aliases**: `open64`, `lseek64`, `stat64`, `fstat64`, `lstat64`,
//!   `fopen64`, `freopen64`, `mmap64`, `prlimit64`
//! - **glibc Compat**: `__xstat`/`__fxstat`/`__lxstat` (and `*64` variants),
//!   `__libc_malloc`/`__libc_free`/`__libc_realloc`/`__libc_calloc`/`__libc_memalign`,
//!   `__isoc99_sscanf`/`__isoc99_scanf`/`__isoc99_fscanf`,
//!   `__libc_current_sigrtmin`/`__libc_current_sigrtmax`,
//!   `_IO_stdin_`/`_IO_stdout_`/`_IO_stderr_`,
//!   `gnu_get_libc_version`/`gnu_get_libc_release`, `getauxval`
//! - **C++ ABI**: `__cxa_guard_acquire`/`__cxa_guard_release`/`__cxa_guard_abort`,
//!   `__cxa_atexit`, `__cxa_thread_atexit_impl`, `__cxa_pure_virtual`,
//!   `__cxa_allocate_exception`/`__cxa_throw`/`__cxa_begin_catch`/`__cxa_end_catch`,
//!   `__gxx_personality_v0`, `_Unwind_Resume`, `__stack_chk_fail`
//!
//! ## Error Handling
//!
//! POSIX functions return -1 on error and set `errno`.  Our native
//! syscalls return negative error codes.  The translation layer converts
//! native error codes to POSIX errno values (80+ constants matching
//! Linux x86_64).
//!
//! ## Encoding
//!
//! All multibyte ↔ wide character functions use UTF-8 (not ASCII stubs).
//! Full 4-byte UTF-8 decoding/encoding for the entire Unicode range
//! (U+0000..U+10FFFF), with overlong and surrogate rejection.
//!
//! ## References
//!
//! - POSIX.1-2024 (IEEE Std 1003.1-2024)
//! - Linux man pages (for practical POSIX semantics)
//! - Redox relibc (Rust POSIX libc for a custom OS)
//! - musl libc (minimal Linux libc, good reference for what to implement)

// On our OS target (x86_64-unknown-none, target_os = "none"), build as
// no_std.  On the host (Windows/Linux), use std so `cargo test` works.
#![cfg_attr(target_os = "none", no_std)]
#![deny(clippy::all, clippy::pedantic)]
#![allow(
    clippy::module_name_repetitions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    clippy::missing_safety_doc,       // extern "C" functions are inherently unsafe
    clippy::not_unsafe_ptr_arg_deref, // POSIX functions take raw pointers by design
    clippy::inline_always,            // syscall wrappers must be inlined
    clippy::wildcard_imports,         // syscall constant imports
    clippy::doc_markdown,             // POSIX identifiers (O_CREAT, x86_64) used extensively in docs
    clippy::large_stack_arrays,       // Dir pool is intentionally large (~544 KiB)
)]

// Panic handler for no_std staticlib.
// When linked into a binary that provides its own panic handler,
// the linker will use the binary's version.  This is a fallback.
#[cfg(target_os = "none")]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {
        // SAFETY: hlt is a safe instruction in any privilege level.
        unsafe { core::arch::asm!("hlt", options(nostack, nomem)); }
    }
}

pub mod aio;
pub mod assert;
pub mod crt;
pub mod ctype;
pub mod dlfcn;
pub mod environ;
pub mod epoll;
pub mod err;
pub mod errno;
pub mod fcntl;
pub mod fcntl_ops;
pub mod fnmatch;
pub mod ftw;
pub mod getopt;
pub mod glob;
pub mod iconv;
pub mod inttypes;
pub mod libgen;
pub mod limits;
pub mod ioctl;
pub mod malloc;
pub mod math;
pub mod fdtable;
pub mod file;
pub mod locale;
pub mod mman;
pub mod mqueue;
pub mod pipe;
pub mod poll;
pub mod printf;
pub mod pthread;
pub mod regex;
pub mod scanf;
pub mod sched;
pub mod semaphore;
pub mod process;
pub mod pwd;
pub mod setjmp;
pub mod signal;
pub mod socket;
pub mod spawn;
pub mod stat;
pub mod statvfs;
pub mod stdio;
pub mod stdlib;
pub mod string;
pub mod syscall;
pub mod syslog;
pub mod time;
pub mod types;
pub mod resource;
pub mod unistd;
pub mod utmpx;
pub mod utsname;
pub mod wait;
pub mod wchar;
pub mod wordexp;
pub mod xattr;
pub mod dirent;
