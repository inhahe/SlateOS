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
//!   `open_by_handle_at`, `get_nprocs`/`get_nprocs_conf`,
//!   `get_phys_pages`/`get_avphys_pages`, `futimesat`, `tmpnam_r`,
//!   `scandirat`, `get_current_dir_name`
//! - **Device Numbers**: `gnu_dev_major`/`gnu_dev_minor`/`gnu_dev_makedev`
//! - **Dynamic Linking** (stubs): `dlopen`, `dlsym`, `dlclose`, `dlerror`,
//!   `dladdr`, `dl_iterate_phdr`, `__tls_get_addr`
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

pub mod aio;
pub mod alloca;
pub mod ar;
pub mod arpa_inet;
pub mod arpa_nameser;
pub mod assert;
pub mod compiler_rt;
pub mod cpio;
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
pub mod file;
pub mod fmtmsg;
pub mod fnmatch;
pub mod fortify;
pub mod fortify_printf;
pub mod fts;
pub mod ftw;
pub mod getopt;
pub mod glob;
pub mod grp;
pub mod iconv;
pub(crate) mod iconv_8bit;
pub(crate) mod iconv_translit;
pub mod ifaddrs;
pub mod inttypes;
pub mod ioctl;
pub mod langinfo;
pub mod libgen;
pub mod libintl;
pub mod limits;
pub mod linux_acl;
pub mod linux_acpi;
pub mod linux_affinity;
pub mod linux_aio;
pub mod linux_aio_abi;
pub mod linux_aperture;
pub mod linux_at_flags_user_types;
pub mod linux_atm;
pub mod linux_audit;
pub mod linux_auto_fs;
pub mod linux_autofs;
pub mod linux_auxiliary;
pub mod linux_auxv_types;
pub mod linux_auxvec;
pub mod linux_ax25;
pub mod linux_backlight;
pub mod linux_bcache;
pub mod linux_binfmt;
pub mod linux_binfmt_elf;
pub mod linux_binfmts;
pub mod linux_blk_cgroup;
pub mod linux_blk_integrity;
pub mod linux_blk_mq;
pub mod linux_blkdev;
pub mod linux_blkpg;
pub mod linux_bonding;
pub mod linux_bpf;
pub mod linux_bridge;
pub mod linux_bsg;
pub mod linux_btrfs;
pub mod linux_bug;
pub mod linux_can;
pub mod linux_capability;
pub mod linux_cdrom;
pub mod linux_cdx;
pub mod linux_cec;
pub mod linux_ceph;
pub mod linux_cgroup;
pub mod linux_cgroup_freezer;
pub mod linux_cgroup_namespace;
pub mod linux_cgroup_rdma;
pub mod linux_clk;
pub mod linux_clone_args;
pub mod linux_close_range;
pub mod linux_cls_flower;
pub mod linux_cma;
pub mod linux_cn_proc;
pub mod linux_configfs;
pub mod linux_connector;
pub mod linux_copy_file_range;
pub mod linux_coredump;
pub mod linux_coresight;
pub mod linux_cpu_cgroup;
pub mod linux_cpu_set;
pub mod linux_cpufreq;
pub mod linux_cpuidle;
pub mod linux_cpuset;
pub mod linux_cramfs;
pub mod linux_crash_core;
pub mod linux_crypto;
pub mod linux_cxl;
pub mod linux_dax;
pub mod linux_dcb;
pub mod linux_dcbnl;
pub mod linux_dccp;
pub mod linux_debugfs;
pub mod linux_devcoredump;
pub mod linux_devfreq;
pub mod linux_device;
pub mod linux_devlink;
pub mod linux_devmem;
pub mod linux_dirent_types;
pub mod linux_dm_ioctl;
pub mod linux_dm_log_userspace;
pub mod linux_dma_buf;
pub mod linux_dma_engine;
pub mod linux_dma_fence;
pub mod linux_dma_heap;
pub mod linux_dma_mapping;
pub mod linux_dmi;
pub mod linux_dpll;
pub mod linux_drm;
pub mod linux_drm_fourcc;
pub mod linux_drm_mode;
pub mod linux_dsa;
pub mod linux_dvb;
pub mod linux_ecryptfs;
pub mod linux_efi;
pub mod linux_elf;
pub mod linux_energy_model;
pub mod linux_erofs;
pub mod linux_errno;
pub mod linux_errqueue;
pub mod linux_ethtool;
pub mod linux_eventfd;
pub mod linux_eventpoll;
pub mod linux_evm;
pub mod linux_exfat;
pub mod linux_extcon;
pub mod linux_fadvise;
pub mod linux_falloc;
pub mod linux_fallocate;
pub mod linux_fanotify;
pub mod linux_fb;
pub mod linux_fcntl;
pub mod linux_fib_rules;
pub mod linux_fiemap;
pub mod linux_filter;
pub mod linux_firmware;
pub mod linux_fpga;
pub mod linux_fs;
pub mod linux_fscrypt;
pub mod linux_fsnotify;
pub mod linux_fsnotify_user_types;
pub mod linux_fsverity;
pub mod linux_ftrace;
pub mod linux_fuse;
pub mod linux_futex;
pub mod linux_gameport;
pub mod linux_gen_stats;
pub mod linux_genetlink;
pub mod linux_geneve;
pub mod linux_genhd;
pub mod linux_gpio;
pub mod linux_gre;
pub mod linux_handshake;
pub mod linux_hdreg;
pub mod linux_hibernate;
pub mod linux_hid;
pub mod linux_hidraw;
pub mod linux_hmm;
pub mod linux_hugetlb;
pub mod linux_hugetlb_cgroup;
pub mod linux_hwmon;
pub mod linux_i2c;
pub mod linux_icmp;
pub mod linux_ieee802154;
pub mod linux_if_addr;
pub mod linux_if_arp;
pub mod linux_if_bonding;
pub mod linux_if_bridge;
pub mod linux_if_ether;
pub mod linux_if_link;
pub mod linux_if_macvlan;
pub mod linux_if_packet;
pub mod linux_if_tun;
pub mod linux_if_vlan;
pub mod linux_if_xdp;
pub mod linux_igmp;
pub mod linux_iio;
pub mod linux_ima;
pub mod linux_inotify;
pub mod linux_input;
pub mod linux_input_event;
pub mod linux_input_event_codes;
pub mod linux_input_mt;
pub mod linux_interconnect;
pub mod linux_io_cgroup;
pub mod linux_io_prio;
pub mod linux_io_uring;
pub mod linux_io_uring_cmd;
pub mod linux_io_uring_setup_types;
pub mod linux_io_uring_sqe;
pub mod linux_ioctl;
pub mod linux_iommu;
pub mod linux_iopoll;
pub mod linux_ioprio;
pub mod linux_iova;
pub mod linux_ip;
pub mod linux_ip_vs;
pub mod linux_ipc;
pub mod linux_ipc_namespace;
pub mod linux_ipv6;
pub mod linux_ipvlan;
pub mod linux_irq;
pub mod linux_iso9660;
pub mod linux_jffs2;
pub mod linux_joystick;
pub mod linux_kcmp;
pub mod linux_kcov;
pub mod linux_kd;
pub mod linux_kdebug;
pub mod linux_kexec;
pub mod linux_key;
pub mod linux_keyctl;
pub mod linux_keyring;
pub mod linux_kmod;
pub mod linux_kms;
pub mod linux_kobject;
pub mod linux_kvm;
pub mod linux_l2tp;
pub mod linux_landlock;
pub mod linux_leds;
pub mod linux_limits;
pub mod linux_lirc;
pub mod linux_loop;
pub mod linux_lsm;
pub mod linux_macvlan;
pub mod linux_magic;
pub mod linux_mailbox;
pub mod linux_mctp;
pub mod linux_mdev;
pub mod linux_mdio;
pub mod linux_media;
pub mod linux_mei;
pub mod linux_membarrier;
pub mod linux_memcontrol;
pub mod linux_memfd;
pub mod linux_mfd;
pub mod linux_migrate;
pub mod linux_mii;
pub mod linux_misc_cgroup;
pub mod linux_mm;
pub mod linux_mmc;
pub mod linux_module;
pub mod linux_mount;
pub mod linux_mount_api;
pub mod linux_mount_namespace;
pub mod linux_mount_user_types;
pub mod linux_mpls;
pub mod linux_mqueue;
pub mod linux_msi;
pub mod linux_mtd;
pub mod linux_namespaces;
pub mod linux_nbd;
pub mod linux_ndctl;
pub mod linux_neighbour;
pub mod linux_net;
pub mod linux_net_namespace;
pub mod linux_net_tstamp;
pub mod linux_netdev;
pub mod linux_netfilter;
pub mod linux_netfilter_arp;
pub mod linux_netfilter_bridge;
pub mod linux_netfilter_ipv4;
pub mod linux_netfilter_ipv6;
pub mod linux_netlink;
pub mod linux_netlink_route;
pub mod linux_nf_conntrack;
pub mod linux_nf_nat;
pub mod linux_nf_tables;
pub mod linux_nfc;
pub mod linux_nftables;
pub mod linux_nl80211;
pub mod linux_notifier;
pub mod linux_nsfs;
pub mod linux_numa;
pub mod linux_nvme;
pub mod linux_nvme_ioctl;
pub mod linux_nvme_tcp;
pub mod linux_nvmem;
pub mod linux_of;
pub mod linux_oom;
pub mod linux_openat2;
pub mod linux_opp;
pub mod linux_overlayfs;
pub mod linux_panic;
pub mod linux_pci;
pub mod linux_pci_ids;
pub mod linux_pci_regs;
pub mod linux_perf_attr_types;
pub mod linux_perf_cgroup;
pub mod linux_perf_event;
pub mod linux_personality;
pub mod linux_phonet;
pub mod linux_phy;
pub mod linux_phylink;
pub mod linux_pid_namespace;
pub mod linux_pidfd;
pub mod linux_pidfd2_types;
pub mod linux_pids_cgroup;
pub mod linux_pinctrl;
pub mod linux_pkt_sched;
pub mod linux_platform_device;
pub mod linux_pm_qos;
pub mod linux_pm_runtime;
pub mod linux_posix_acl;
pub mod linux_posix_timers;
pub mod linux_power_supply;
pub mod linux_ppp;
pub mod linux_ppp_defs;
pub mod linux_prctl;
pub mod linux_printk;
pub mod linux_proc_ns;
pub mod linux_procfs;
pub mod linux_property;
pub mod linux_psci;
pub mod linux_psi;
pub mod linux_pthread_key_types;
pub mod linux_ptp;
pub mod linux_ptrace;
pub mod linux_pwm;
pub mod linux_quota;
pub mod linux_random;
pub mod linux_rcu;
pub mod linux_readahead;
pub mod linux_reboot;
pub mod linux_regmap;
pub mod linux_regulator;
pub mod linux_remoteproc;
pub mod linux_reset;
pub mod linux_resource;
pub mod linux_rfkill;
pub mod linux_rlimit;
pub mod linux_romfs;
pub mod linux_rpmsg;
pub mod linux_rseq;
pub mod linux_rtc;
pub mod linux_rtnetlink;
pub mod linux_sched;
pub mod linux_sched_ext;
pub mod linux_scsi;
pub mod linux_sctp;
pub mod linux_sdio;
pub mod linux_seccomp;
pub mod linux_seccomp_filter;
pub mod linux_securebit;
pub mod linux_securebits;
pub mod linux_seg6;
pub mod linux_sendfile;
pub mod linux_serdev;
pub mod linux_serial;
pub mod linux_sfp;
pub mod linux_signalfd;
pub mod linux_slip;
pub mod linux_smc;
pub mod linux_sock_diag;
pub mod linux_sockios;
pub mod linux_sound;
pub mod linux_spi;
pub mod linux_splice;
pub mod linux_squashfs;
pub mod linux_stat;
pub mod linux_statx;
pub mod linux_stddef;
pub mod linux_surface_aggregator;
pub mod linux_suspend;
pub mod linux_swap;
pub mod linux_switchdev;
pub mod linux_sync_file;
pub mod linux_sync_file_range;
pub mod linux_sysctl;
pub mod linux_sysfs;
pub mod linux_sysinfo;
pub mod linux_target_core;
pub mod linux_taskstats;
pub mod linux_tc_act;
pub mod linux_tc_actions;
pub mod linux_tc_csum;
pub mod linux_tc_ct;
pub mod linux_tc_mirred;
pub mod linux_tc_pedit;
pub mod linux_tc_police;
pub mod linux_tc_skbedit;
pub mod linux_tc_tunnel_key;
pub mod linux_tc_vlan;
pub mod linux_tcp;
pub mod linux_tcp_states;
pub mod linux_tee;
pub mod linux_thermal;
pub mod linux_thunderbolt;
pub mod linux_time;
pub mod linux_time_namespace;
pub mod linux_timerfd;
pub mod linux_tipc;
pub mod linux_tls;
pub mod linux_tmpfs;
pub mod linux_topology;
pub mod linux_trace;
pub mod linux_tracefs;
pub mod linux_tty;
pub mod linux_tty_user_types;
pub mod linux_tun;
pub mod linux_typec;
pub mod linux_ubi;
pub mod linux_ubifs;
pub mod linux_udmabuf;
pub mod linux_udp;
pub mod linux_uinput;
pub mod linux_uio;
pub mod linux_unistd;
pub mod linux_uprobes;
pub mod linux_usb;
pub mod linux_usb_ch9;
pub mod linux_usb_gadget;
pub mod linux_usb_pd;
pub mod linux_user_namespace;
pub mod linux_userfaultfd;
pub mod linux_uts_namespace;
pub mod linux_utsname;
pub mod linux_utsname_types;
pub mod linux_uuid;
pub mod linux_vdpa;
pub mod linux_veth;
pub mod linux_vgaarb;
pub mod linux_vhost;
pub mod linux_videodev2;
pub mod linux_virtio_balloon;
pub mod linux_virtio_blk;
pub mod linux_virtio_config;
pub mod linux_virtio_console;
pub mod linux_virtio_crypto;
pub mod linux_virtio_fs;
pub mod linux_virtio_gpu;
pub mod linux_virtio_input;
pub mod linux_virtio_net;
pub mod linux_virtio_pci;
pub mod linux_virtio_ring;
pub mod linux_virtio_scsi;
pub mod linux_virtio_types;
pub mod linux_virtio_vsock;
pub mod linux_vlan;
pub mod linux_vm_sockets;
pub mod linux_vsock;
pub mod linux_vt;
pub mod linux_vt_kern;
pub mod linux_vxlan;
pub mod linux_wait;
pub mod linux_wakeup;
pub mod linux_watch_queue;
pub mod linux_watchdog;
pub mod linux_wdt;
pub mod linux_wireguard;
pub mod linux_wireless;
pub mod linux_wmi;
pub mod linux_workqueue;
pub mod linux_wwan;
pub mod linux_xattr;
pub mod linux_xdp;
pub mod linux_xfrm;
pub mod linux_zonefs;
pub mod linux_zram;
pub mod linux_zswap;
pub mod locale;
pub mod lowlevellock;
pub mod malloc;
pub mod math;
pub mod md5;
pub mod mman;
pub mod mntent;
pub mod monetary;
pub mod mqueue;
pub mod net_ethernet;
pub mod net_if;
pub mod net_if_arp;
pub mod net_if_packet;
pub mod net_route;
pub mod netdb;
pub mod netinet;
pub mod netinet_in;
pub mod netinet_tcp;
pub mod nl_types;
pub(crate) mod nss_files;
pub(crate) mod objtable;
pub mod paths;
pub(crate) mod perprocess;
pub mod perthread;
pub mod pipe;
pub mod poll;
pub mod printf;
pub mod process;
pub mod pthread;
pub mod pty;
pub mod ptytab;
pub mod pwd;
pub mod random;
pub mod regex;
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
pub mod stdio;
pub mod stdlib;
pub mod string;
pub mod strings;
pub mod stropts;
pub mod sys_auxv;
pub mod sys_capability;
pub mod sys_epoll;
pub mod sys_eventfd;
pub mod sys_fcntl;
pub mod sys_file;
pub mod sys_fsuid;
pub mod sys_inotify;
pub mod sys_io;
pub mod sys_ioctl;
pub mod sys_klog;
pub mod sys_mman;
pub mod sys_mman_ext;
pub mod sys_mount;
pub mod sys_msg;
pub mod sys_param;
pub mod sys_personality;
pub mod sys_prctl;
pub mod sys_prctl_caps;
pub mod sys_ptrace;
pub mod sys_quota;
pub mod sys_random;
pub mod sys_reboot;
pub mod sys_resource;
pub mod sys_sched;
pub mod sys_select;
pub mod sys_sem;
pub mod sys_sendfile;
pub mod sys_shm;
pub mod sys_signalfd;
pub mod sys_socket;
pub mod sys_stat;
pub mod sys_statvfs;
pub mod sys_swap;
pub mod sys_syscall;
pub mod sys_sysctl;
pub mod sys_sysinfo;
pub mod sys_syslog;
pub mod sys_time;
pub mod sys_timerfd;
pub mod sys_times;
pub mod sys_timex;
pub mod sys_ttydefaults;
pub mod sys_types;
pub mod sys_uio;
pub mod sys_un;
pub mod sys_utsname;
pub mod sys_vfs;
pub mod sys_wait;
pub mod sys_wait_ext;
pub mod sys_xattr;
pub mod syscall;
pub mod sysexits;
pub mod syslog;
pub(crate) mod sysv_ipc;
pub mod sysv_msg;
pub mod sysv_sem;
pub mod sysv_shm;
pub mod tar;
pub mod termios;
pub mod time;
pub mod tls;
pub mod types;
pub mod tz;
pub mod uchar;
pub(crate) mod uio;
pub mod ulimit;
pub mod unistd;
pub mod utime;
pub mod utmpx;
pub mod utsname;
pub mod values;
pub mod wait;
pub mod wchar;
pub mod wordexp;
pub mod x87;
pub mod xattr;
