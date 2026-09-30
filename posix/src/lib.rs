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
//!   `fallocate`, `splice`, `tee`, `vmsplice`, `mknod`, `mkfifo`,
//!   `openat2`, `faccessat2`, `statx`
//! - **Sockets**: `socket`, `connect`, `bind`, `listen`, `accept`,
//!   `send`, `recv`, `sendto`, `recvfrom`, `shutdown`, `setsockopt`,
//!   `getsockopt`, `getpeername`, `getsockname`, `getaddrinfo`,
//!   `freeaddrinfo`, `getnameinfo`, `gethostbyname`, `gethostbyname2`,
//!   `htons`, `htonl`, `inet_addr`, `inet_ntoa`, `inet_aton`,
//!   `inet_pton`, `inet_ntop`
//! - **I/O Multiplexing**: `poll`, `select`, `pselect`, `signalfd4`,
//!   `epoll_pwait2`, `sockatmark`
//! - **Terminal**: `ioctl` (TIOCGWINSZ, TCGETS, FIONBIO, etc.),
//!   `isatty`, `ttyname`, `tcgetattr`, `tcsetattr`, `cfmakeraw`,
//!   `cfsetspeed`, `tcsendbreak`, `tcdrain`, `tcflow`, `tcflush`,
//!   termios flags, `posix_openpt`, `grantpt`, `unlockpt`, `ptsname`,
//!   `ptsname_r`, `ttyname_r`, `openpty`, `forkpty`, `login_tty`
//! - **Process**: `_exit`, `getpid`, `getppid`, `posix_spawn`,
//!   `posix_spawnp`, `execve`, `execvp`, `execv`, `execvpe`, `fexecve`,
//!   `vfork`, `waitpid`, `sleep`, `nanosleep`, `getpgrp`, `setpgid`,
//!   `setsid`, `getsid`, `pidfd_open`, `pidfd_send_signal`, `pidfd_getfd`,
//!   `issetugid`, `posix_spawn_file_actions_addchdir_np`,
//!   `posix_spawn_file_actions_addclosefrom_np`, `clone3`,
//!   `process_vm_readv`/`process_vm_writev`, `kcmp`
//! - **Memory**: `mmap`, `munmap`, `mprotect`, `mmap64`, `mremap`,
//!   `mlock`/`mlock2`/`munlock`/`mlockall`/`munlockall`, `msync`, `madvise`,
//!   `posix_madvise`, `shm_open`/`shm_unlink`, `memfd_create`
//! - **Pipes**: `pipe`, `pipe2`
//! - **Signals**: Stub constants and handlers (partial), `sigwait`,
//!   `sigtimedwait`, `sigqueue`, `siginterrupt`,
//!   `psiginfo`, `siginfo_t`; `sigaltstack` is **real** -- the stack is
//!   stored, reported per POSIX and used, so a handler registered with
//!   `SA_ONSTACK` runs on it (design-decisions.md 1009)
//! - **Threads**: `pthread` stubs, working mutex ops,
//!   `pthread_setaffinity_np`/`pthread_getaffinity_np` (CPU affinity)
//! - **C Standard Library**: `malloc`/`free`/`calloc`/`realloc`,
//!   `posix_memalign`/`aligned_alloc`/`valloc`/`memalign`/`reallocarray`,
//!   `malloc_usable_size`,
//!   `setjmp`/`longjmp`/`sigsetjmp`/`siglongjmp`, `qsort`, `bsearch`,
//!   `atoi`/`atol`/`atoll`/`strtol`/`strtoul`, `a64l`/`l64a`,
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
//! - **Formatted Messages**: `fmtmsg` (structured error/warning display)
//! - **Message Catalogs**: `catopen`, `catgets`, `catclose` (stubs —
//!   always falls back to default strings)
//! - **Backtrace** (stubs): `backtrace`, `backtrace_symbols`,
//!   `backtrace_symbols_fd`
//! - **DNS Resolver** (stubs): `res_init`, `res_query`, `res_search`,
//!   `res_mkquery`, `res_send`, `dn_expand`, `dn_comp`, `dn_skipname`,
//!   `ns_get16`/`ns_get32`/`ns_put16`/`ns_put32`
//! - **Process Times**: `times` (CPU time accounting stub)
//! - **System V IPC** (stubs): `msgget`/`msgsnd`/`msgrcv`/`msgctl`,
//!   `semget`/`semop`/`semtimedop`/`semctl`,
//!   `shmget`/`shmat`/`shmdt`/`shmctl`
//! - **Password Hashing**: `crypt`, `crypt_r` (stub — returns
//!   `$0$<key>`), `encrypt`, `setkey` (DES stubs — ENOSYS)
//! - **Language Information**: `nl_langinfo`, `nl_langinfo_l`
//!   (C locale date/time formats, day/month names, codeset, etc.)
//! - **Monetary Formatting**: `strfmon`, `strfmon_l` (C locale
//!   decimal formatting with `%n`/`%i` specifiers)
//! - **Search / Data Structures** (`<search.h>`): red-black tree `tsearch`,
//!   `tfind`, `tdelete`, `twalk`, `twalk_r`, `tdestroy`; hash table `hcreate`, `hdestroy`,
//!   `hsearch`; linear search `lfind`, `lsearch`; linked list `insque`,
//!   `remque`
//! - **Resource Limits**: `getrlimit`, `setrlimit`, `getrusage`,
//!   `prlimit`/`prlimit64`
//! - **Timers**: `timer_create`, `timer_settime`, `timer_gettime`,
//!   `timer_delete`, `timer_getoverrun` (stubs — no signal delivery),
//!   `setitimer`/`getitimer`
//! - **System**: `uname`
//! - **Logging**: `openlog`, `syslog`, `closelog`, `setlogmask`
//! - **User/Group**: `getpwnam`, `getpwuid`, `getgrnam`, `getgrgid`,
//!   `getlogin`, password/group enumeration; the account files read from and
//!   written to a caller's stream (`fgetpwent`, `putpwent`, `fgetgrent`,
//!   `putgrent`, `fgetspent`, `sgetspent`, `putspent`), `lckpwdf`,
//!   `cuserid`, `getusershell`, `getpass`
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
//! - **Complex** (`<complex.h>`, C99 Annex G): `cabs`, `carg`, `cproj`,
//!   `csqrt`, `cexp`, `clog`, `cpow`, the circular and hyperbolic functions
//!   and their inverses (and `float` variants; FreeBSD msun's)
//! - **Floating-point environment** (`<fenv.h>`): rounding direction and
//!   exception flags, both units
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
//!   `open_by_handle_at`, `get_nprocs`/`get_nprocs_conf`,
//!   `get_phys_pages`/`get_avphys_pages`, `futimesat`, `tmpnam_r`,
//!   `scandirat`, `get_current_dir_name`
//! - **Device Numbers**: `gnu_dev_major`/`gnu_dev_minor`/`gnu_dev_makedev`
//! - **Dynamic Linking** (the program the one object, as in a static glibc
//!   program): `dlopen`, `dlmopen`, `dlsym`, `dlvsym`, `dlclose`,
//!   `dlerror`, `dlinfo`, `dladdr`, `dladdr1`, `dl_iterate_phdr`,
//!   `_dl_find_object`, `__tls_get_addr`
//! - **Directories**: `opendir`, `closedir`, `readdir`, `rewinddir`,
//!   `seekdir`, `telldir`, `scandir`, `alphasort`, `versionsort`,
//!   `readdir_r`, `fdopendir` (via path tracking), `dirfd`
//! - **File Mode Testing**: `S_ISREG`, `S_ISDIR`, `S_ISLNK`, `S_ISCHR`,
//!   `S_ISBLK`, `S_ISFIFO`, `S_ISSOCK`, `mknod`/`mknodat`,
//!   `mkfifo`/`mkfifoat`
//! - **LP64 Aliases**: `open64`, `lseek64`, `stat64`, `fstat64`, `lstat64`,
//!   `fstatat64`, `fopen64`, `freopen64`, `mmap64`, `prlimit64`
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
    clippy::decimal_bitwise_operands, // ABI/syscall constant tables mirror Linux headers verbatim (e.g. AUDIT_ARCH_ARM = 40 | ...); rewriting to hex obscures the source correspondence
    clippy::unreadable_literal,       // Linux/POSIX constant tables (errno codes, ioctl numbers, capability bits, etc.) are copied from the upstream headers verbatim. Inserting `_` separators breaks easy cross-referencing with man pages and kernel sources.
    clippy::must_use_candidate,       // POSIX C functions return error codes that callers are explicitly allowed to ignore (e.g. write(2) without checking the return value is well-defined C); the error channel is errno, not the return value. Adding #[must_use] everywhere would not match C/POSIX semantics.
    clippy::pub_underscore_fields,    // Linux ABI structs (`struct statx`, `struct sysinfo`, `struct shmid_ds`, etc.) carry padding/reserved fields whose names (`__reserved`, `__unused`, `__pad`) are part of the kernel ABI and cannot be renamed.
    clippy::doc_lazy_continuation,    // POSIX docs frequently use bulleted lists whose continuation lines align under the bullet text for readability of long error-condition descriptions. Reflowing every such list would hurt readability for a purely stylistic check.
    clippy::doc_overindented_list_items, // Same rationale: enumerated POSIX validation-order lists align continuation text under the start of the item text (e.g. "1. reserved fields nonzero   → EINVAL"), which is more readable than a 5-space continuation.
    clippy::match_same_arms,          // POSIX/libc dispatch tables (`sysconf`, `pathconf`, `confstr`, `nl_langinfo`, `name_to_handle_at`/`open_by_handle_at` error mapping, etc.) match on many distinct constants and several happen to return the same value (e.g. multiple `_SC_*` codes returning 1024). Merging them with or-patterns obscures the POSIX code → value correspondence.
    clippy::items_after_statements,   // Function-body `const` definitions placed right next to their first use (e.g. `const KSYS_EVENTFD_SEMAPHORE: u64 = 1;` inside `eventfd_create`) document a kernel-ABI constant at its point of use and are more readable than hoisting them to module scope.
    clippy::too_many_lines,           // POSIX/Linux syscall wrappers are inherently long: flag normalisation, ENAMETOOLONG / EFAULT / EINVAL / EPERM / ELOOP / ... fast-path checks, slow-path delegation, errno translation, and (often) per-arch fixups. Splitting them by category fragments the syscall semantics across helpers and makes the "matches Linux's foo.c::do_bar prologue" comment harder to track.
    clippy::struct_excessive_bools,   // Some POSIX/Linux ABI structs (e.g. termios flag bitfields, file open-mode flags rendered as fields) carry many bool-like fields that mirror upstream layouts.
    // The lints below are new pedantic checks (clippy 1.95+) that fire heavily on a verbatim libc translation
    // layer. The wrappers mirror Linux headers/syscalls byte-for-byte and accommodating these stylistic checks
    // would obscure the kernel-ABI correspondence that makes the port reviewable.
    clippy::similar_names,            // POSIX wrappers routinely name companion variables (e.g. `va`/`vb`, `low`/`lo`, `path`/`pat`) to mirror upstream C source.
    clippy::many_single_char_names,   // Cryptographic/POSIX byte-ops idiomatically use `a`/`b`/`c`/etc. for state words and offsets.
    clippy::ptr_as_ptr,               // Pointer casts in libc wrappers go between many ABI types; `as` keeps call sites compact.
    clippy::ref_as_ptr,               // `&x as *const T` is the canonical FFI pattern; `&raw const x` is newer and not yet universal.
    clippy::ptr_cast_constness,       // Casting between *const T / *mut T is required by C ABIs that drop const at the boundary.
    clippy::needless_pass_by_value,   // Many POSIX functions take owned types by value to match C semantics.
    clippy::missing_errors_doc,       // Error conditions are documented at the POSIX level, not per Rust wrapper.
    clippy::missing_panics_doc,       // Panic-free POSIX wrappers; remaining panics are intentional aborts on invalid kernel state.
    clippy::cast_precision_loss,      // Numeric ABI conversions (time_t, off_t, etc.) intentionally cast.
    clippy::if_not_else,              // POSIX validation often reads better as `if invalid { return EINVAL } else { ok }`.
    clippy::redundant_else,           // Same.
    clippy::semicolon_if_nothing_returned, // Stylistic only.
    clippy::manual_let_else,          // Some explicit `match`/`if let` blocks document a multi-step POSIX validation order.
    clippy::collapsible_if,           // Nested validation ifs mirror upstream kernel source structure.
    clippy::collapsible_else_if,      // Same.
    clippy::needless_range_loop,      // Manual indexing matches upstream loop bodies.
    clippy::option_if_let_else,       // Stylistic; some `match` blocks are clearer.
    clippy::manual_is_multiple_of,    // `% N == 0` is idiomatic in alignment/ABI code.
    clippy::single_match_else,        // Stylistic.
    clippy::map_unwrap_or,            // Stylistic.
    clippy::needless_borrows_for_generic_args, // ABI/syscall calls explicit about borrowing.
    clippy::format_push_string,       // Format-based string assembly in error paths.
    clippy::if_then_some_else_none,   // Stylistic.
    clippy::bool_to_int_with_if,      // POSIX semantics: explicit `if cond { 1 } else { 0 }` matches the C `?:` idiom in upstream code.
    clippy::if_same_then_else,        // ABI shims occasionally have stub branches that collapse but aid future divergence.
    clippy::comparison_chain,         // Comparison chains map directly to upstream qsort/compare callbacks.
    clippy::manual_div_ceil,          // `(a + b - 1) / b` matches upstream kernel ABI rounding macros (DIV_ROUND_UP).
    clippy::let_underscore_untyped,   // Discarding return values from POSIX-shaped APIs is intentional.
    clippy::needless_late_init,       // Late init pattern matches upstream variable scopes.
    clippy::useless_vec,              // Vec literals used as fixture inputs in tests.
    clippy::print_with_newline,       // Test diagnostics; cosmetic only.
    clippy::approx_constant,          // Test fixtures use literal mathematical constants matching upstream tests.
    clippy::float_cmp,                // POSIX float APIs (`strtod`, etc.) test round-trip exactness.
    clippy::cast_lossless,            // Stylistic; explicit `as` keeps width changes visible.
    clippy::range_plus_one,           // Some ranges keep `+1` for ABI clarity (e.g. inclusive POSIX limits).
    clippy::manual_range_contains,    // Two-sided comparisons match upstream argument validation.
    clippy::unnecessary_wraps,        // Returning `Result` keeps wrappers uniform with surrounding ABI shims.
    clippy::while_let_loop,           // Iteration patterns mirror upstream `while ((p = nextent(...)))` style.
    clippy::trivially_copy_pass_by_ref, // ABI structs are often passed by reference for layout stability.
    clippy::no_effect_underscore_binding, // Stub bodies in #[cfg]-gated arms intentionally drop their argument.
    clippy::needless_continue,        // Loop-flow mirrors upstream syscall validation.
    clippy::elidable_lifetime_names,  // Explicit lifetimes document FFI signatures.
    // High-volume new pedantic lints (clippy 1.95+) firing across thousands of test/wrapper sites in the libc port.
    clippy::manual_c_str_literals,    // Tests construct nul-terminated byte strings as `b"...\0"` to mirror C source; rewriting to `c"..."` literals in 1400+ sites obscures the C correspondence.
    clippy::borrow_as_ptr,            // `&x as *const T` is the canonical FFI pattern across our libc wrappers; `&raw const x` is newer and not yet universal across our codebase.
    clippy::assertions_on_constants,  // POSIX/Linux ABI tests `assert!(O_RDONLY == 0)`, `assert!(SIGKILL == 9)`, etc. to lock down constants whose values are ABI-stable; clippy can fold these but the assertions are intentional documentation of ABI guarantees.
    clippy::uninlined_format_args,    // Format-string inlining (`{name}`) is stylistic; positional args keep call sites grep-able against C printf format strings in upstream code.
    clippy::cast_ptr_alignment,       // libc/syscall casts between u8 buffers and ABI structs intentionally bypass alignment checks; alignment is enforced by the caller per the kernel ABI contract.
    clippy::redundant_closure_for_method_calls, // `.map(|x| x.foo())` vs `.map(T::foo)` — first form is more readable in POSIX validation pipelines.
    clippy::unnecessary_cast,         // Explicit casts (e.g. `0 as c_int`) document the ABI-required type at the call site.
    clippy::manual_dangling_ptr,      // `0 as *mut T` / `ptr::null_mut().offset(...)` match C ABI patterns; `ptr::dangling_mut()` is newer.
    clippy::used_underscore_items,    // Linux ABI fields/functions prefixed with `_` (e.g. `_exit`, `__errno_location`) are part of the POSIX namespace and must keep their names.
    clippy::used_underscore_binding,  // Same: `_x` bindings in ABI shims.
    clippy::identity_op,              // ABI tables sometimes use `x | 0` or `x * 1` to keep columns aligned with adjacent rows that have nonzero constants.
    clippy::absurd_extreme_comparisons, // Range checks against ABI limits (e.g. `nfds >= MAX_FDS`) occasionally compare against `usize::MAX` style bounds.
    clippy::explicit_iter_loop,       // `for x in v.iter()` matches upstream loop style in some POSIX validation paths.
    clippy::default_trait_access,     // `T::default()` vs `Default::default()` — both forms appear depending on context.
    clippy::items_after_test_module,  // Inline helper `const`/`fn` items kept after `#[cfg(test)] mod tests` document test-only ABI helpers.
    clippy::bool_assert_comparison,   // Tests assert `assert_eq!(flag, true)` for symmetry with the ABI-constant assertions above.
    clippy::manual_is_power_of_two,   // `(x & (x - 1)) == 0` matches upstream kernel ABI macros (IS_POW2).
    clippy::ignored_unit_patterns,    // `let _ = ...` patterns in ABI shims discard known-unit returns intentionally.
    clippy::get_first,                // `v.get(0)` vs `v.first()` — first form matches upstream indexing patterns.
    clippy::erasing_op,               // `x * 0` / `0 << n` in ABI bitfield assembly mirrors upstream macros.
    clippy::let_unit_value,           // `let _: () = expr;` documents that a syscall returns unit.
    clippy::checked_conversions,      // Manual range checks before `as` cast are more readable than `TryFrom` in ABI wrappers.
    clippy::manual_midpoint,          // `(lo + hi) / 2` in POSIX search routines mirrors upstream code.
    clippy::manual_contains,          // `iter().any(|c| *c == target)` in POSIX byte-string scanning matches upstream C loops.
    clippy::missing_const_for_thread_local, // Thread-local initialisers in POSIX wrappers depend on runtime state.
    clippy::ptr_offset_by_literal,    // `p.offset(N)` in C-ABI pointer arithmetic mirrors upstream code.
    clippy::unnecessary_trailing_comma, // Stylistic.
    clippy::duplicated_attributes,    // Tolerated for crate/sub-module attribute repetition during the libc port.
    non_upper_case_globals,           // POSIX globals: environ, stdin, stdout, optarg, etc.
    non_snake_case,                   // POSIX/C functions: S_ISREG, _Unwind_Resume, etc.
)]
// The POSIX library is a verbatim translation of Linux libc headers / syscall
// wrappers. The wrappers use raw pointer arithmetic and indexing into fixed
// ABI buffers; the test build sees these as `unwrap_used` / `indexing_slicing`
// / `arithmetic_side_effects` even though the production lint rig is fine with
// them (we mirror libc layouts). Suppress the noisy defensive lints in the
// test build only — the production build still enforces the workspace policy.
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        clippy::case_sensitive_file_extension_comparisons,
        clippy::single_char_pattern,
    )
)]

// Panic handler for no_std staticlib.
// When linked into a binary that provides its own panic handler,
// the linker will use the binary's version.  This is a fallback.
#[cfg(target_os = "none")]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {
        // SAFETY: hlt is a safe instruction in any privilege level.
        unsafe {
            core::arch::asm!("hlt", options(nostack, nomem));
        }
    }
}

// Emits C `_Static_assert`s for `scripts/check-libc-abi.py` to compile against
// musl. Test-only: it is a checking apparatus, not part of the library.
#[cfg(test)]
mod abi_layout;

// Replays glibc's answers for the account-file functions
// (`posix/tools/oracle/accounts_harness.py`). Test-only.
#[cfg(test)]
mod accounts_oracle;

pub mod aio;
pub mod aliases;
pub mod alloca;
pub mod argz;
pub mod assert;
pub mod besl;
pub mod c23math;
pub mod compiler_rt;
pub mod complex;
pub mod complexl;
pub mod crt;
pub mod crypt;
pub mod ctype;
pub(crate) mod decfloat;
pub mod dirent;
pub mod dlfcn;
// Ed25519 lives here, next to `crypt` and `sha2`, because this crate is
// already where lane B's cryptographic primitives are written once and shared:
// `sshd`, `ssh` and `ftpd` each had their own fake of it. See the module doc.
pub mod ed25519;
pub mod endian;
pub mod environ;
pub mod epoll;
pub mod err;
pub mod errno;
pub mod error;
pub mod execinfo;
pub(crate) mod exit_list;
pub mod fcntl;
pub mod fcntl_ops;
pub mod fdtable;
pub mod fenv;
pub mod file;
pub(crate) mod fmadd;
pub mod fmtmsg;
pub mod fnmatch;
pub mod fortify;
pub mod fortify_printf;
pub mod fstab;
pub mod fts;
pub mod ftw;
pub mod gai;
pub mod gai_a;
pub mod getopt;
pub mod getpass;
pub mod glob;
pub mod gshadow;
pub mod hosts;
pub mod iconv;
pub(crate) mod iconv_8bit;
pub(crate) mod iconv_combining;
pub(crate) mod iconv_prefix;
pub(crate) mod iconv_translit;
pub mod inet;
pub mod inet6;
pub(crate) mod interrupt;
pub mod inttypes;
pub mod ioctl;
pub mod langinfo;
pub mod ld80;
/// The C calling convention for `long double` (`ld_c!`).
mod ld_abi;
pub mod legacy;
pub(crate) mod lgamma;
pub mod libgen;
pub mod libintl;
pub mod limits;
pub mod linux_aio_abi;
pub mod linux_at_flags_user_types;
pub mod linux_auxv_types;
pub mod linux_bpf;
pub mod linux_clone_args;
pub mod linux_close_range;
pub mod linux_dirent_types;
pub mod linux_fanotify;
pub mod linux_filter;
pub mod linux_fsnotify_user_types;
pub mod linux_futex;
pub mod linux_io_uring;
pub mod linux_io_uring_setup_types;
pub mod linux_ipc;
pub mod linux_landlock;
pub mod linux_limits;
pub mod linux_memfd;
pub mod linux_module;
pub mod linux_mount_user_types;
pub mod linux_perf_attr_types;
pub mod linux_perf_event;
pub mod linux_pidfd2_types;
pub mod linux_pthread_key_types;
pub mod linux_seccomp;
pub mod linux_stddef;
pub mod linux_time;
pub mod linux_tty_user_types;
pub mod linux_userfaultfd;
pub mod linux_utsname_types;
pub mod locale;
pub mod lowlevellock;
pub mod malloc;
pub mod math;
pub mod mathl;
pub mod mcheck;
pub mod md5;
pub mod mman;
pub mod mntent;
pub mod monetary;
pub mod mqueue;
pub mod narrow;
pub mod netdb;
pub mod netgroup;
pub mod nl_types;
pub(crate) mod nss_files;
pub(crate) mod objtable;
pub mod paths;
pub(crate) mod perprocess;
pub mod perthread;
pub mod pipe;
pub mod poll;
pub mod printf;
pub mod prng;
pub mod process;
pub mod pthread;
pub mod pty;
pub mod ptytab;
pub mod pwd;
pub mod random;
pub mod regex;
pub(crate) mod rem_pio2_large;
pub mod resolv;
pub mod resource;
pub mod scanf;
pub mod sched;
pub mod search;
pub mod semaphore;
pub mod setjmp;
pub mod sha2;
pub mod shadow;
/// `#!` interpreter lines and the argument rewrite they imply — the pure half
/// of running a script, driven by `spawn`'s `execve` and `posix_spawn`.
pub(crate) mod shebang;
pub mod sigevent;
pub mod signal;
pub mod socket;
pub mod spawn;
pub mod stat;
pub mod statvfs;
pub mod stdbit;
pub mod stdio;
pub mod stdio_mem;
pub mod stdlib;
pub mod string;
pub mod strings;
pub mod stropts;
pub mod sys_auxv;
pub mod sys_capability;
pub mod sys_fsuid;
pub mod sys_io;
pub mod sys_mount;
pub mod sys_param;
pub mod sys_prctl;
pub mod sys_quota;
pub mod sys_syscall;
pub mod sys_sysctl;
pub mod sys_times;
pub mod sys_timex;
pub mod sys_wait;
pub mod syscall;
pub mod syslog;
pub(crate) mod sysv_ipc;
pub mod sysv_msg;
pub mod sysv_sem;
pub mod sysv_shm;
pub(crate) mod tempname;
pub mod threads;
pub mod time;
pub mod tls;
pub mod ttyent;
pub mod types;
pub mod tz;
pub mod uchar;
pub mod ucontext;
pub(crate) mod uio;
pub mod ulimit;
pub mod unistd;
pub mod usershell;
pub mod utime;
pub mod utmpx;
pub mod utsname;
pub mod wait;
pub mod wchar;
pub mod wordexp;
pub mod x87;
pub mod xattr;
