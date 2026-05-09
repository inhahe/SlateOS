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
//!   `fstat`, `unlink`, `mkdir`, `rmdir`, `rename`, `dup`, `dup2`,
//!   `access`, `chmod`, `chown`, `umask`, `truncate`, `ftruncate`,
//!   `fsync`, `link`, `symlink`, `readlink`
//! - **Sockets**: `socket`, `connect`, `bind`, `listen`, `accept`,
//!   `send`, `recv`, `sendto`, `recvfrom`, `shutdown`, `setsockopt`,
//!   `getsockopt`, `getpeername`, `getsockname`, `getaddrinfo`,
//!   `freeaddrinfo`, `gethostbyname`, `htons`, `htonl`, `inet_addr`,
//!   `inet_ntoa`
//! - **I/O Multiplexing**: `poll`, `select`, `pselect`
//! - **Terminal**: `ioctl` (TIOCGWINSZ, TCGETS, FIONBIO, etc.),
//!   `isatty`, `ttyname`, `tcgetattr`, `tcsetattr`, termios flags
//! - **Process**: `_exit`, `getpid`, `getppid`, `posix_spawn`,
//!   `posix_spawnp`, `execve`, `execvp`, `waitpid`, `sleep`,
//!   `nanosleep`, `getpgrp`, `setpgid`, `setsid`, `getsid`
//! - **Memory**: `mmap`, `munmap`, `mprotect`
//! - **Pipes**: `pipe`, `pipe2`
//! - **Signals**: Stub constants and handlers (partial)
//! - **Threads**: `pthread` stubs, working mutex ops
//! - **C Standard Library**: `malloc`/`free`/`calloc`/`realloc`,
//!   `setjmp`/`longjmp`, `qsort`, `bsearch`, `atoi`/`strtol`,
//!   `puts`/`fputs`/`fwrite`/`fread`/`perror`, ctype classification
//! - **Formatted Output**: `printf`, `fprintf`, `sprintf`, `snprintf`
//!   (via assembly trampoline for C variadic capture)
//! - **Formatted Input**: `sscanf` (string scanning with `%d`/`%u`/`%x`/
//!   `%o`/`%s`/`%c`/`%f`/`%n`, width limits, assignment suppression)
//! - **Pattern Matching**: `fnmatch` (shell wildcards), `glob`/`globfree`
//!   (pathname expansion), `wordexp`/`wordfree` (word expansion)
//! - **Character Encoding**: `iconv_open`, `iconv`, `iconv_close`
//!   (UTF-8/ASCII conversions)
//! - **Resource Limits**: `getrlimit`, `setrlimit`, `getrusage`
//! - **Timers**: `timer_create`, `timer_settime`, `timer_gettime`,
//!   `timer_delete`, `timer_getoverrun` (stubs — no signal delivery)
//! - **System**: `uname`
//! - **Logging**: `openlog`, `syslog`, `closelog`, `setlogmask`
//! - **User/Group**: `getpwnam`, `getpwuid`, `getgrnam`, `getgrgid`,
//!   `getlogin`, password/group enumeration
//! - **Math**: `fabs`, `floor`, `ceil`, `round`, `trunc`, `fmod`,
//!   `sqrt`, `pow`, `exp`/`exp2`, `log`/`log2`/`log10`,
//!   `sin`, `cos`, `tan`, `atan2`, `frexp`, `ldexp`, `modf`,
//!   `isnan`, `isinf`, `isfinite`, `copysign`, `fmin`, `fmax`
//!   (and `f32` variants)
//! - **Wide Characters**: `mblen`, `mbtowc`, `wctomb`, `mbstowcs`,
//!   `wcstombs`, `btowc`, `wctob`, `mbsinit`, `mbrtowc`, `wcrtomb`,
//!   `wcwidth`, `wcswidth`, `iswalnum`..`iswxdigit`, `towlower`,
//!   `towupper`, `wcscpy`, `wcslen`, `wcscmp`, `wcscat`, `wcschr`,
//!   `wcsrchr`, `wmemcpy`, `wmemset`, `wmemcmp`, `nl_langinfo`
//! - **File Tree Walk**: `ftw`, `nftw` (recursive directory traversal)
//! - **BSD Error Functions**: `err`, `errx`, `warn`, `warnx` (and `v*`
//!   variants)
//! - **Misc**: `getcwd`, `chdir`, `realpath`, `errno`, `sysconf`,
//!   `getenv`/`setenv`, `pread`, `pwrite`, `readv`, `writev`,
//!   `basename`, `dirname`, `getopt`, `pathconf`, `confstr`,
//!   `strlcpy`, `strlcat`, `mkdtemp`
//!
//! ## Error Handling
//!
//! POSIX functions return -1 on error and set `errno`.  Our native
//! syscalls return negative error codes.  The translation layer converts
//! native error codes to POSIX errno values (70+ constants matching
//! Linux x86_64).
//!
//! ## What This Is NOT
//!
//! This is not a complete C runtime (no `printf` — needs C variadic
//! support, deferred to musl port).  Higher-level facilities like
//! buffered FILE* I/O and locale support will come from porting musl.
//!
//! ## References
//!
//! - POSIX.1-2024 (IEEE Std 1003.1-2024)
//! - Linux man pages (for practical POSIX semantics)
//! - Redox relibc (Rust POSIX libc for a custom OS)
//! - musl libc (minimal Linux libc, good reference for what to implement)

#![no_std]
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
#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {
        // SAFETY: hlt is a safe instruction in any privilege level.
        unsafe { core::arch::asm!("hlt", options(nostack, nomem)); }
    }
}

pub mod assert;
pub mod crt;
pub mod ctype;
pub mod dlfcn;
pub mod environ;
pub mod err;
pub mod errno;
pub mod fcntl;
pub mod fcntl_ops;
pub mod fnmatch;
pub mod ftw;
pub mod getopt;
pub mod glob;
pub mod iconv;
pub mod libgen;
pub mod limits;
pub mod ioctl;
pub mod malloc;
pub mod math;
pub mod fdtable;
pub mod file;
pub mod locale;
pub mod mman;
pub mod pipe;
pub mod poll;
pub mod printf;
pub mod pthread;
pub mod regex;
pub mod scanf;
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
pub mod utsname;
pub mod wait;
pub mod wchar;
pub mod wordexp;
pub mod dirent;
