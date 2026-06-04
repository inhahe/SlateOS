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
//!   `ptsname_r`
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
//!   `sigtimedwait`, `sigqueue`, `sigaltstack`, `siginterrupt`,
//!   `psiginfo`, `siginfo_t`
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
//! - **Database Operations** (stubs): `dbm_open`, `dbm_close`,
//!   `dbm_store`, `dbm_fetch`, `dbm_delete`, `dbm_firstkey`,
//!   `dbm_nextkey`, `dbm_error`, `dbm_clearerr`
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
//! - **Search / Data Structures** (`<search.h>`): BST `tsearch`, `tfind`,
//!   `tdelete`, `twalk`, `tdestroy`; hash table `hcreate`, `hdestroy`,
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

pub mod aio;
pub mod alloca;
pub mod ar;
pub mod arpa_inet;
pub mod arpa_nameser;
pub mod assert;
pub mod cpio;
pub mod crt;
pub mod crypt;
pub mod ctype;
pub mod dirent;
pub mod dlfcn;
pub mod endian;
pub mod environ;
pub mod epoll;
pub mod err;
pub mod errno;
pub mod error;
pub mod execinfo;
pub mod fcntl;
pub mod fcntl_ops;
pub mod fdtable;
pub mod file;
pub mod fmtmsg;
pub mod fnmatch;
pub mod fortify_printf;
pub mod fts;
pub mod ftw;
pub mod getopt;
pub mod glob;
pub mod grp;
pub mod iconv;
pub mod ifaddrs;
pub mod inttypes;
pub mod ioctl;
pub mod langinfo;
pub mod libgen;
pub mod libintl;
pub mod limits;
pub mod linux_9p_types;
pub mod linux_9p_user_types;
pub mod linux_acct_types;
pub mod linux_acct_user_types;
pub mod linux_acl;
pub mod linux_acl_types;
pub mod linux_acl_user_types;
pub mod linux_acpi;
pub mod linux_acpi2_types;
pub mod linux_acpi2_user_types;
pub mod linux_acpi_event_types;
pub mod linux_acpi_event_user_types;
pub mod linux_acpi_nfit_types;
pub mod linux_acpi_nfit_user_types;
pub mod linux_acpi_notify_types;
pub mod linux_acpi_notify_user_types;
pub mod linux_acpi_power_types;
pub mod linux_acpi_power_user_types;
pub mod linux_acpi_sleep_types;
pub mod linux_acpi_sleep_user_types;
pub mod linux_acpi_table_types;
pub mod linux_acpi_table_user_types;
pub mod linux_acpi_tables_types;
pub mod linux_acpi_tables_user_types;
pub mod linux_acpi_thermal_types;
pub mod linux_acpi_thermal_user_types;
pub mod linux_acpi_types;
pub mod linux_acpi_user_types;
pub mod linux_acrn_user_types;
pub mod linux_address_space_types;
pub mod linux_address_space_user_types;
pub mod linux_adfs_types;
pub mod linux_adfs_user_types;
pub mod linux_af_alg_types;
pub mod linux_af_alg_user_types;
pub mod linux_af_can_types;
pub mod linux_af_can_user_types;
pub mod linux_af_vsock_types;
pub mod linux_af_vsock_user_types;
pub mod linux_af_xdp2_types;
pub mod linux_af_xdp_types;
pub mod linux_affinity;
pub mod linux_affs_types;
pub mod linux_affs_user_types;
pub mod linux_afs_types;
pub mod linux_afs_user_types;
pub mod linux_aio;
pub mod linux_aio2_types;
pub mod linux_aio2_user_types;
pub mod linux_aio3_types;
pub mod linux_aio3_user_types;
pub mod linux_aio_abi;
pub mod linux_aio_types;
pub mod linux_aio_user_types;
pub mod linux_alarm_types;
pub mod linux_alarm_user_types;
pub mod linux_alsa3_types;
pub mod linux_alsa3_user_types;
pub mod linux_alsa_control_types;
pub mod linux_alsa_control_user_types;
pub mod linux_alsa_hwdep_types;
pub mod linux_alsa_hwdep_user_types;
pub mod linux_alsa_jack_types;
pub mod linux_alsa_jack_user_types;
pub mod linux_alsa_mixer_types;
pub mod linux_alsa_mixer_user_types;
pub mod linux_alsa_pcm_types;
pub mod linux_alsa_pcm_user_types;
pub mod linux_alsa_rawmidi_types;
pub mod linux_alsa_rawmidi_user_types;
pub mod linux_alsa_seq_types;
pub mod linux_alsa_seq_user_types;
pub mod linux_alsa_timer_types;
pub mod linux_alsa_timer_user_types;
pub mod linux_alsa_types;
pub mod linux_alsa_user_types;
pub mod linux_ambient_cap_types;
pub mod linux_ambient_cap_user_types;
pub mod linux_amt_types;
pub mod linux_amt_user_types;
pub mod linux_ancillary_types;
pub mod linux_ancillary_user_types;
pub mod linux_android_binder_types;
pub mod linux_android_binder_user_types;
pub mod linux_aoe_user_types;
pub mod linux_aperture;
pub mod linux_aperture_user_types;
pub mod linux_apm_bios_types;
pub mod linux_apm_bios_user_types;
pub mod linux_apparmor_types;
pub mod linux_apparmor_user_types;
pub mod linux_arcnet_types;
pub mod linux_arcnet_user_types;
pub mod linux_arm_smccc_types;
pub mod linux_arm_smccc_user_types;
pub mod linux_arp_types;
pub mod linux_arp_user_types;
pub mod linux_at_flags_types;
pub mod linux_at_flags_user_types;
pub mod linux_ata_types;
pub mod linux_ata_user_types;
pub mod linux_atm;
pub mod linux_atm2_types;
pub mod linux_atm2_user_types;
pub mod linux_atm_types;
pub mod linux_atm_user_types;
pub mod linux_atm_zatm_types;
pub mod linux_atm_zatm_user_types;
pub mod linux_atmsap_types;
pub mod linux_atmsap_user_types;
pub mod linux_audit;
pub mod linux_audit2_types;
pub mod linux_audit2_user_types;
pub mod linux_audit3_types;
pub mod linux_audit3_user_types;
pub mod linux_audit_arch_types;
pub mod linux_audit_arch_user_types;
pub mod linux_audit_field_types;
pub mod linux_audit_field_user_types;
pub mod linux_audit_types;
pub mod linux_audit_user_types;
pub mod linux_auto_fs;
pub mod linux_autofs;
pub mod linux_auxiliary;
pub mod linux_auxiliary_bus_types;
pub mod linux_auxiliary_bus_user_types;
pub mod linux_auxv_types;
pub mod linux_auxv_user_types;
pub mod linux_auxvec;
pub mod linux_auxvec2_types;
pub mod linux_auxvec_types;
pub mod linux_auxvec_user_types;
pub mod linux_ax25;
pub mod linux_ax25_2_types;
pub mod linux_ax25_2_user_types;
pub mod linux_ax25_types;
pub mod linux_ax25_user_types;
pub mod linux_backlight;
pub mod linux_backlight_types;
pub mod linux_backlight_user_types;
pub mod linux_balloon_types;
pub mod linux_balloon_user_types;
pub mod linux_bareudp_types;
pub mod linux_bareudp_user_types;
pub mod linux_batadv2_types;
pub mod linux_batadv2_user_types;
pub mod linux_batadv_types;
pub mod linux_batadv_user_types;
pub mod linux_batman2_types;
pub mod linux_batman2_user_types;
pub mod linux_batman_types;
pub mod linux_batman_user_types;
pub mod linux_battery_types;
pub mod linux_battery_user_types;
pub mod linux_bcache;
pub mod linux_bcache2_types;
pub mod linux_bcache2_user_types;
pub mod linux_bcache_types;
pub mod linux_bcache_user_types;
pub mod linux_befs_types;
pub mod linux_befs_user_types;
pub mod linux_binfmt;
pub mod linux_binfmt2_types;
pub mod linux_binfmt2_user_types;
pub mod linux_binfmt_elf;
pub mod linux_binfmt_types;
pub mod linux_binfmt_user_types;
pub mod linux_binfmts;
pub mod linux_bio_types;
pub mod linux_bio_user_types;
pub mod linux_blk_cgroup;
pub mod linux_blk_cgroup_types;
pub mod linux_blk_cgroup_user_types;
pub mod linux_blk_crypto_types;
pub mod linux_blk_crypto_user_types;
pub mod linux_blk_integrity;
pub mod linux_blk_integrity_types;
pub mod linux_blk_integrity_user_types;
pub mod linux_blk_ioctl_types;
pub mod linux_blk_ioctl_user_types;
pub mod linux_blk_mq;
pub mod linux_blk_mq_types;
pub mod linux_blk_mq_user_types;
pub mod linux_blk_request_types;
pub mod linux_blk_request_user_types;
pub mod linux_blk_stat_types;
pub mod linux_blk_stat_user_types;
pub mod linux_blk_throttle_types;
pub mod linux_blk_throttle_user_types;
pub mod linux_blk_types;
pub mod linux_blk_user_types;
pub mod linux_blk_zone_types;
pub mod linux_blk_zone_user_types;
pub mod linux_blkdev;
pub mod linux_blkdev_user_types;
pub mod linux_blkio_cgroup_types;
pub mod linux_blkio_cgroup_user_types;
pub mod linux_blkpg;
pub mod linux_blkpg_user_types;
pub mod linux_blktrace_api_types;
pub mod linux_blktrace_api_user_types;
pub mod linux_blktrace_types;
pub mod linux_blktrace_user_types;
pub mod linux_blkzoned_types;
pub mod linux_blkzoned_user_types;
pub mod linux_block_stat_types;
pub mod linux_block_stat_user_types;
pub mod linux_bond2_types;
pub mod linux_bond2_user_types;
pub mod linux_bonding;
pub mod linux_bonding2_types;
pub mod linux_bonding2_user_types;
pub mod linux_bonding_types;
pub mod linux_bonding_user_types;
pub mod linux_bpf;
pub mod linux_bpf2_types;
pub mod linux_bpf2_user_types;
pub mod linux_bpf3_types;
pub mod linux_bpf4_types;
pub mod linux_bpf5_types;
pub mod linux_bpf_attach_types;
pub mod linux_bpf_attach_user_types;
pub mod linux_bpf_cgroup_types;
pub mod linux_bpf_cmd_types;
pub mod linux_bpf_cmd_user_types;
pub mod linux_bpf_helper_types;
pub mod linux_bpf_helper_user_types;
pub mod linux_bpf_insn_types;
pub mod linux_bpf_insn_user_types;
pub mod linux_bpf_link_types;
pub mod linux_bpf_link_user_types;
pub mod linux_bpf_map_types;
pub mod linux_bpf_map_user_types;
pub mod linux_bpf_perf_types;
pub mod linux_bpf_perf_user_types;
pub mod linux_bpf_prog_types;
pub mod linux_bpf_prog_user_types;
pub mod linux_bpf_trace_types;
pub mod linux_bpf_trace_user_types;
pub mod linux_bpf_types;
pub mod linux_bpf_user_types;
pub mod linux_bridge;
pub mod linux_bridge2_types;
pub mod linux_bridge_types;
pub mod linux_bridge_user_types;
pub mod linux_bsg;
pub mod linux_bsg_types;
pub mod linux_bsg_user_types;
pub mod linux_btrfs;
pub mod linux_btrfs2_types;
pub mod linux_btrfs2_user_types;
pub mod linux_btrfs3_types;
pub mod linux_btrfs3_user_types;
pub mod linux_btrfs_types;
pub mod linux_btrfs_user_types;
pub mod linux_bug;
pub mod linux_bug_user_types;
pub mod linux_bus_types;
pub mod linux_bus_user_types;
pub mod linux_cachefiles_types;
pub mod linux_cachefiles_user_types;
pub mod linux_cachestat_types;
pub mod linux_cachestat_user_types;
pub mod linux_caif2_types;
pub mod linux_caif2_user_types;
pub mod linux_caif_types;
pub mod linux_caif_user_types;
pub mod linux_can;
pub mod linux_can2_types;
pub mod linux_can2_user_types;
pub mod linux_can3_types;
pub mod linux_can3_user_types;
pub mod linux_can_netlink_types;
pub mod linux_can_netlink_user_types;
pub mod linux_can_types;
pub mod linux_can_user_types;
pub mod linux_capability;
pub mod linux_capability_types;
pub mod linux_capability_user_types;
pub mod linux_capability_v3_types;
pub mod linux_capability_v3_user_types;
pub mod linux_capi_types;
pub mod linux_capi_user_types;
pub mod linux_cciss_types;
pub mod linux_cciss_user_types;
pub mod linux_ccix_types;
pub mod linux_ccix_user_types;
pub mod linux_cdrom;
pub mod linux_cdrom2_types;
pub mod linux_cdrom2_user_types;
pub mod linux_cdrom3_types;
pub mod linux_cdrom3_user_types;
pub mod linux_cdrom_types;
pub mod linux_cdrom_user_types;
pub mod linux_cdx;
pub mod linux_cdx_user_types;
pub mod linux_cec;
pub mod linux_cec_types;
pub mod linux_cec_user_types;
pub mod linux_ceph;
pub mod linux_ceph2_types;
pub mod linux_ceph2_user_types;
pub mod linux_ceph_types;
pub mod linux_ceph_user_types;
pub mod linux_cfs_types;
pub mod linux_cfs_user_types;
pub mod linux_cgroup;
pub mod linux_cgroup2_types;
pub mod linux_cgroup2_user_types;
pub mod linux_cgroup3_types;
pub mod linux_cgroup3_user_types;
pub mod linux_cgroup4_types;
pub mod linux_cgroup4_user_types;
pub mod linux_cgroup5_types;
pub mod linux_cgroup5_user_types;
pub mod linux_cgroup_freezer;
pub mod linux_cgroup_freezer_types;
pub mod linux_cgroup_freezer_user_types;
pub mod linux_cgroup_namespace;
pub mod linux_cgroup_rdma;
pub mod linux_cgroup_types;
pub mod linux_cgroup_user_types;
pub mod linux_cgroupstats_types;
pub mod linux_cgroupstats_user_types;
pub mod linux_cifs2_types;
pub mod linux_cifs2_user_types;
pub mod linux_cifs_types;
pub mod linux_cifs_user_types;
pub mod linux_clk;
pub mod linux_clk_provider_types;
pub mod linux_clk_provider_user_types;
pub mod linux_clk_types;
pub mod linux_clk_user_types;
pub mod linux_clock2_types;
pub mod linux_clock2_user_types;
pub mod linux_clock_types;
pub mod linux_clock_user_types;
pub mod linux_clockevent_types;
pub mod linux_clockevent_user_types;
pub mod linux_clocksource_types;
pub mod linux_clocksource_user_types;
pub mod linux_clone3_types;
pub mod linux_clone3_user_types;
pub mod linux_clone_args;
pub mod linux_clone_args_types;
pub mod linux_clone_args_user_types;
pub mod linux_clone_flags_types;
pub mod linux_clone_flags_user_types;
pub mod linux_close_range;
pub mod linux_close_range_types;
pub mod linux_close_range_user_types;
pub mod linux_cls_basic_types;
pub mod linux_cls_basic_user_types;
pub mod linux_cls_bpf_types;
pub mod linux_cls_bpf_user_types;
pub mod linux_cls_flower;
pub mod linux_cls_flower2_types;
pub mod linux_cls_flower2_user_types;
pub mod linux_cls_matchall_types;
pub mod linux_cls_matchall_user_types;
pub mod linux_cls_types;
pub mod linux_cls_user_types;
pub mod linux_cma;
pub mod linux_cma_types;
pub mod linux_cma_user_types;
pub mod linux_cmsg_types;
pub mod linux_cmsg_user_types;
pub mod linux_cn_proc;
pub mod linux_cn_proc_types;
pub mod linux_cn_proc_user_types;
pub mod linux_coda_types;
pub mod linux_coda_user_types;
pub mod linux_comedi_types;
pub mod linux_comedi_user_types;
pub mod linux_compaction_types;
pub mod linux_compaction_user_types;
pub mod linux_component_types;
pub mod linux_component_user_types;
pub mod linux_configfs;
pub mod linux_configfs_types;
pub mod linux_configfs_user_types;
pub mod linux_confstr_types;
pub mod linux_confstr_user_types;
pub mod linux_connector;
pub mod linux_connector_types;
pub mod linux_connector_user_types;
pub mod linux_conntrack_types;
pub mod linux_conntrack_user_types;
pub mod linux_console_types;
pub mod linux_console_user_types;
pub mod linux_cooling_types;
pub mod linux_cooling_user_types;
pub mod linux_copy_file_range;
pub mod linux_copy_file_range_types;
pub mod linux_copy_file_range_user_types;
pub mod linux_core_dump_types;
pub mod linux_core_dump_user_types;
pub mod linux_coredump;
pub mod linux_coredump_filter_types;
pub mod linux_coredump_types;
pub mod linux_coresight;
pub mod linux_coresight_types;
pub mod linux_counter_types;
pub mod linux_cper_types;
pub mod linux_cpu_affinity_types;
pub mod linux_cpu_cgroup;
pub mod linux_cpu_features_types;
pub mod linux_cpu_idle_types;
pub mod linux_cpu_set;
pub mod linux_cpu_set_types;
pub mod linux_cpu_topology_types;
pub mod linux_cpufreq;
pub mod linux_cpufreq2_types;
pub mod linux_cpufreq_gov_types;
pub mod linux_cpufreq_types;
pub mod linux_cpufreq_user_types;
pub mod linux_cpuidle;
pub mod linux_cpuidle_types;
pub mod linux_cpuidle_user_types;
pub mod linux_cpuset;
pub mod linux_cpuset_types;
pub mod linux_cramfs;
pub mod linux_cramfs_types;
pub mod linux_crash_core;
pub mod linux_cred_types;
pub mod linux_crypto;
pub mod linux_crypto3_types;
pub mod linux_crypto_aead_types;
pub mod linux_crypto_akcipher_types;
pub mod linux_crypto_alg_types;
pub mod linux_crypto_cipher_types;
pub mod linux_crypto_hash_types;
pub mod linux_crypto_kpp_types;
pub mod linux_crypto_rng_types;
pub mod linux_crypto_skcipher_types;
pub mod linux_crypto_types;
pub mod linux_crypto_user_types;
pub mod linux_cryptouser_types;
pub mod linux_cxl;
pub mod linux_cxl_mailbox_types;
pub mod linux_cxl_mem_types;
pub mod linux_dasd_types;
pub mod linux_dax;
pub mod linux_dax_types;
pub mod linux_dcb;
pub mod linux_dcb2_types;
pub mod linux_dcb_types;
pub mod linux_dcbnl;
pub mod linux_dcbnl_types;
pub mod linux_dccp;
pub mod linux_dccp2_types;
pub mod linux_dccp_types;
pub mod linux_dccp_user_types;
pub mod linux_deadline_sched_types;
pub mod linux_debugfs;
pub mod linux_debugfs_types;
pub mod linux_dentry_types;
pub mod linux_devcoredump;
pub mod linux_devfreq;
pub mod linux_devfreq2_types;
pub mod linux_devfreq_types;
pub mod linux_devfreq_user_types;
pub mod linux_device;
pub mod linux_device_class_types;
pub mod linux_devlink;
pub mod linux_devlink2_types;
pub mod linux_devlink_types;
pub mod linux_devmem;
pub mod linux_devres_types;
pub mod linux_devtmpfs_types;
pub mod linux_dio_types;
pub mod linux_direct_io_types;
pub mod linux_dirent_types;
pub mod linux_dlm_device_types;
pub mod linux_dlm_netlink_types;
pub mod linux_dlm_plock_types;
pub mod linux_dlm_types;
pub mod linux_dlm_user_types;
pub mod linux_dlmconstants_types;
pub mod linux_dlopen_types;
pub mod linux_dm2_types;
pub mod linux_dm3_types;
pub mod linux_dm_ioctl;
pub mod linux_dm_ioctl_types;
pub mod linux_dm_log_userspace;
pub mod linux_dm_target_types;
pub mod linux_dm_types;
pub mod linux_dm_user_types;
pub mod linux_dma_buf;
pub mod linux_dma_buf2_types;
pub mod linux_dma_buf_types;
pub mod linux_dma_buf_uapi_types;
pub mod linux_dma_coherent_types;
pub mod linux_dma_direction_types;
pub mod linux_dma_engine;
pub mod linux_dma_fence;
pub mod linux_dma_fence_types;
pub mod linux_dma_heap;
pub mod linux_dma_heap2_types;
pub mod linux_dma_heap_types;
pub mod linux_dma_heap_user_types;
pub mod linux_dma_mapping;
pub mod linux_dma_mapping_types;
pub mod linux_dma_pool_types;
pub mod linux_dma_resv_types;
pub mod linux_dma_types;
pub mod linux_dmesg_types;
pub mod linux_dmi;
pub mod linux_dmi_types;
pub mod linux_dnotify_types;
pub mod linux_dpll;
pub mod linux_drbd_types;
pub mod linux_driver_model_types;
pub mod linux_drm;
pub mod linux_drm4_types;
pub mod linux_drm_color_types;
pub mod linux_drm_connector_types;
pub mod linux_drm_crtc_types;
pub mod linux_drm_drv_types;
pub mod linux_drm_fence_types;
pub mod linux_drm_format_types;
pub mod linux_drm_fourcc;
pub mod linux_drm_fourcc_types;
pub mod linux_drm_gem_types;
pub mod linux_drm_ioctl_types;
pub mod linux_drm_lease_types;
pub mod linux_drm_mode;
pub mod linux_drm_mode_types;
pub mod linux_drm_plane_types;
pub mod linux_drm_prime_types;
pub mod linux_drm_property_types;
pub mod linux_drm_syncobj_types;
pub mod linux_drm_types;
pub mod linux_drm_user_types;
pub mod linux_drm_vblank_types;
pub mod linux_dsa;
pub mod linux_dvb;
pub mod linux_dvb_ca_types;
pub mod linux_dvb_types;
pub mod linux_dvb_user_types;
pub mod linux_dvfs_types;
pub mod linux_ecryptfs;
pub mod linux_edac_types;
pub mod linux_edd_types;
pub mod linux_edd_user_types;
pub mod linux_efi;
pub mod linux_efi_types;
pub mod linux_efi_vars_types;
pub mod linux_efivarfs_types;
pub mod linux_efivarfs_user_types;
pub mod linux_efs_types;
pub mod linux_elevator_types;
pub mod linux_elf;
pub mod linux_elf2_types;
pub mod linux_elf_aux_types;
pub mod linux_elf_dynamic_types;
pub mod linux_elf_header_types;
pub mod linux_elf_note_types;
pub mod linux_elf_program_types;
pub mod linux_elf_reloc_types;
pub mod linux_elf_section_types;
pub mod linux_elf_types;
pub mod linux_elf_user_types;
pub mod linux_elfcore_types;
pub mod linux_ematch_types;
pub mod linux_energy_model;
pub mod linux_energy_model_types;
pub mod linux_epoll2_types;
pub mod linux_epoll3_types;
pub mod linux_epoll_types;
pub mod linux_erofs;
pub mod linux_erofs_types;
pub mod linux_errno;
pub mod linux_errno2_types;
pub mod linux_errno_types;
pub mod linux_errqueue;
pub mod linux_ethtool;
pub mod linux_ethtool2_types;
pub mod linux_ethtool3_types;
pub mod linux_ethtool4_types;
pub mod linux_ethtool_cmd_types;
pub mod linux_ethtool_link_types;
pub mod linux_ethtool_netlink_types;
pub mod linux_ethtool_types;
pub mod linux_ethtool_user_types;
pub mod linux_eventfd;
pub mod linux_eventfd2_types;
pub mod linux_eventfd3_types;
pub mod linux_eventfd_types;
pub mod linux_eventfd_user_types;
pub mod linux_eventpoll;
pub mod linux_evm;
pub mod linux_evm_types;
pub mod linux_exec_types;
pub mod linux_exfat;
pub mod linux_exit_types;
pub mod linux_extcon;
pub mod linux_extcon_types;
pub mod linux_f2fs_types;
pub mod linux_fadvise;
pub mod linux_fadvise2_types;
pub mod linux_fadvise_types;
pub mod linux_falloc;
pub mod linux_falloc2_types;
pub mod linux_fallocate;
pub mod linux_fallocate_types;
pub mod linux_fanotify;
pub mod linux_fanotify2_types;
pub mod linux_fanotify3_types;
pub mod linux_fanotify_init_types;
pub mod linux_fanotify_mark_types;
pub mod linux_fanotify_types;
pub mod linux_fanotify_user_types;
pub mod linux_fb;
pub mod linux_fb3_types;
pub mod linux_fb_cmap_types;
pub mod linux_fb_types;
pub mod linux_fb_user_types;
pub mod linux_fbcon_types;
pub mod linux_fbdev2_types;
pub mod linux_fc_types;
pub mod linux_fcntl;
pub mod linux_fcntl_cmd_types;
pub mod linux_fd_types;
pub mod linux_fdreg_types;
pub mod linux_fence_types;
pub mod linux_fib_rule_types;
pub mod linux_fib_rules;
pub mod linux_fib_rules2_types;
pub mod linux_fib_rules_types;
pub mod linux_fib_types;
pub mod linux_fiemap;
pub mod linux_fiemap2_types;
pub mod linux_fiemap_types;
pub mod linux_file_seal_types;
pub mod linux_file_types;
pub mod linux_filelock_types;
pub mod linux_filter;
pub mod linux_filter_user_types;
pub mod linux_firewire_cdev_types;
pub mod linux_firewire_types;
pub mod linux_firewire_user_types;
pub mod linux_firmware;
pub mod linux_firmware_load_types;
pub mod linux_firmware_types;
pub mod linux_flock2_types;
pub mod linux_flock_types;
pub mod linux_fnmatch_types;
pub mod linux_fork_types;
pub mod linux_fou2_types;
pub mod linux_fou_types;
pub mod linux_fou_user_types;
pub mod linux_fpga;
pub mod linux_fpga_dfl_types;
pub mod linux_fpga_types;
pub mod linux_fpga_user_types;
pub mod linux_freezer_cgroup_types;
pub mod linux_fs;
pub mod linux_fs_context_types;
pub mod linux_fs_crypt_types;
pub mod linux_fs_ioctl_types;
pub mod linux_fs_label_types;
pub mod linux_fs_magic_types;
pub mod linux_fs_notify_types;
pub mod linux_fs_user_types;
pub mod linux_fs_verity_types;
pub mod linux_fscache_types;
pub mod linux_fscrypt;
pub mod linux_fscrypt_types;
pub mod linux_fscrypt_user_types;
pub mod linux_fsmount_types;
pub mod linux_fsmount_user_types;
pub mod linux_fsnotify;
pub mod linux_fsnotify_types;
pub mod linux_fsnotify_user_types;
pub mod linux_fsopen_types;
pub mod linux_fsverity;
pub mod linux_fsverity2_types;
pub mod linux_fsverity_types;
pub mod linux_fsverity_user_types;
pub mod linux_ftrace;
pub mod linux_ftrace2_types;
pub mod linux_ftrace_types;
pub mod linux_fuse;
pub mod linux_fuse2_types;
pub mod linux_fuse3_types;
pub mod linux_fuse_types;
pub mod linux_fuse_user_types;
pub mod linux_futex;
pub mod linux_futex2_types;
pub mod linux_futex3_types;
pub mod linux_futex_op_types;
pub mod linux_futex_user_types;
pub mod linux_fwnode_types;
pub mod linux_gameport;
pub mod linux_gameport_user_types;
pub mod linux_gen_stats;
pub mod linux_gen_stats_user_types;
pub mod linux_genetlink;
pub mod linux_genetlink_types;
pub mod linux_genetlink_user_types;
pub mod linux_geneve;
pub mod linux_geneve2_types;
pub mod linux_geneve_types;
pub mod linux_genwqe_types;
pub mod linux_genhd;
pub mod linux_genhd_types;
pub mod linux_genl_types;
pub mod linux_genpd_types;
pub mod linux_getopt_types;
pub mod linux_gfs2_ondisk_types;
pub mod linux_gfs2_types;
pub mod linux_gfs2_user_types;
pub mod linux_ghes_types;
pub mod linux_glob_types;
pub mod linux_gpio;
pub mod linux_gpio2_types;
pub mod linux_gpio3_types;
pub mod linux_gpio_chip_types;
pub mod linux_gpio_consumer_types;
pub mod linux_gpio_event_types;
pub mod linux_gpio_flags_types;
pub mod linux_gpio_ioctl_types;
pub mod linux_gpio_types;
pub mod linux_gpio_user_types;
pub mod linux_gpio_v2_types;
pub mod linux_gpt_types;
pub mod linux_gpt_user_types;
pub mod linux_gre;
pub mod linux_gre2_types;
pub mod linux_gre_types;
pub mod linux_gre_user_types;
pub mod linux_groups_types;
pub mod linux_gtp_types;
pub mod linux_handshake;
pub mod linux_handshake_types;
pub mod linux_handshake_user_types;
pub mod linux_hash_types;
pub mod linux_hdlc_types;
pub mod linux_hdlcdrv_types;
pub mod linux_hdreg;
pub mod linux_hdreg_user_types;
pub mod linux_hfs_types;
pub mod linux_hibernate;
pub mod linux_hibernate_types;
pub mod linux_hid;
pub mod linux_hid2_types;
pub mod linux_hid_bus_types;
pub mod linux_hid_report_types;
pub mod linux_hid_types;
pub mod linux_hid_usage_types;
pub mod linux_hid_user_types;
pub mod linux_hidraw;
pub mod linux_hidraw2_types;
pub mod linux_hidraw_types;
pub mod linux_hidraw_user_types;
pub mod linux_hmm;
pub mod linux_hmm2_types;
pub mod linux_hmm_user_types;
pub mod linux_hpet_types;
pub mod linux_hpet_user_types;
pub mod linux_hrtimer_types;
pub mod linux_hrtimer_user_types;
pub mod linux_hsr2_types;
pub mod linux_hsr_netlink_types;
pub mod linux_hsr_types;
pub mod linux_hsr_user_types;
pub mod linux_hugepage_types;
pub mod linux_hugetlb;
pub mod linux_hugetlb_cgroup;
pub mod linux_hugetlb_cgroup_types;
pub mod linux_hugetlb_types;
pub mod linux_hugetlb_user_types;
pub mod linux_hwmon;
pub mod linux_hwmon2_types;
pub mod linux_hwmon_types;
pub mod linux_hwrng_types;
pub mod linux_hwtstamp2_types;
pub mod linux_hyperv_types;
pub mod linux_hyperv_user_types;
pub mod linux_i2c;
pub mod linux_i2c2_types;
pub mod linux_i2c3_types;
pub mod linux_i2c_dev_types;
pub mod linux_i2c_dev_user_types;
pub mod linux_i2c_types;
pub mod linux_i2o_types;
pub mod linux_i8042_user_types;
pub mod linux_icmp;
pub mod linux_icmp_types;
pub mod linux_icmp_user_types;
pub mod linux_icmpv6_types;
pub mod linux_icmpv6_user_types;
pub mod linux_iconv_types;
pub mod linux_idle_inject_types;
pub mod linux_idxd_types;
pub mod linux_idxd_user_types;
pub mod linux_ieee802154;
pub mod linux_ieee8021_types;
pub mod linux_if_addr;
pub mod linux_if_arp;
pub mod linux_if_arp_types;
pub mod linux_if_arp_user_types;
pub mod linux_if_bonding;
pub mod linux_if_bridge;
pub mod linux_if_ether;
pub mod linux_if_ether_user_types;
pub mod linux_if_flags_types;
pub mod linux_if_link;
pub mod linux_if_link_types;
pub mod linux_if_link_user_types;
pub mod linux_if_macvlan;
pub mod linux_if_packet;
pub mod linux_if_packet_user_types;
pub mod linux_if_tun;
pub mod linux_if_tun_types;
pub mod linux_if_tun_user_types;
pub mod linux_if_vlan;
pub mod linux_if_vlan_user_types;
pub mod linux_if_xdp;
pub mod linux_if_xdp_user_types;
pub mod linux_ife_types;
pub mod linux_igmp;
pub mod linux_igmp_types;
pub mod linux_igmp_user_types;
pub mod linux_iio;
pub mod linux_iio3_types;
pub mod linux_iio_buffer_types;
pub mod linux_iio_events_types;
pub mod linux_iio_types;
pub mod linux_ila2_types;
pub mod linux_ila_types;
pub mod linux_ima;
pub mod linux_ima_types;
pub mod linux_inet_diag2_types;
pub mod linux_inode_types;
pub mod linux_inotify;
pub mod linux_inotify3_types;
pub mod linux_inotify_flags_types;
pub mod linux_inotify_types;
pub mod linux_input;
pub mod linux_input4_types;
pub mod linux_input_abs_types;
pub mod linux_input_ev_types;
pub mod linux_input_event;
pub mod linux_input_event_codes;
pub mod linux_input_ff_types;
pub mod linux_input_id_types;
pub mod linux_input_key_types;
pub mod linux_input_led_types;
pub mod linux_input_mt;
pub mod linux_input_mt_types;
pub mod linux_input_prop_types;
pub mod linux_input_rel_types;
pub mod linux_input_rep_types;
pub mod linux_input_snd_types;
pub mod linux_input_sw_types;
pub mod linux_input_user_types;
pub mod linux_integrity_types;
pub mod linux_interconnect;
pub mod linux_interconnect_types;
pub mod linux_io_cancel_types;
pub mod linux_io_cgroup;
pub mod linux_io_pgetevents_types;
pub mod linux_io_prio;
pub mod linux_io_prio2_types;
pub mod linux_io_prio_types;
pub mod linux_io_submit_types;
pub mod linux_io_uring;
pub mod linux_io_uring2_types;
pub mod linux_io_uring3_types;
pub mod linux_io_uring4_types;
pub mod linux_io_uring5_types;
pub mod linux_io_uring_cmd;
pub mod linux_io_uring_cmd_types;
pub mod linux_io_uring_cqe_types;
pub mod linux_io_uring_flags_types;
pub mod linux_io_uring_op_types;
pub mod linux_io_uring_register_types;
pub mod linux_io_uring_setup_types;
pub mod linux_io_uring_sqe;
pub mod linux_io_uring_sqe_types;
pub mod linux_io_uring_types;
pub mod linux_io_uring_user_types;
pub mod linux_ioctl;
pub mod linux_ioctl3_types;
pub mod linux_ioctl_user_types;
pub mod linux_iommu;
pub mod linux_iommu2_types;
pub mod linux_iommu_types;
pub mod linux_iommu_user_types;
pub mod linux_iopoll;
pub mod linux_iopoll_types;
pub mod linux_ioprio;
pub mod linux_ioprio_types;
pub mod linux_iova;
pub mod linux_ip;
pub mod linux_ip6_tunnel_types;
pub mod linux_ip_opt_types;
pub mod linux_ip_options_types;
pub mod linux_ip_tunnel_types;
pub mod linux_ip_user_types;
pub mod linux_ip_vs;
pub mod linux_ipc;
pub mod linux_ipc_namespace;
pub mod linux_ipc_namespace_types;
pub mod linux_ipc_perm_types;
pub mod linux_ipc_user_types;
pub mod linux_ipv6;
pub mod linux_ipv6_opt_types;
pub mod linux_ipv6_types;
pub mod linux_ipv6_user_types;
pub mod linux_ipvlan;
pub mod linux_ipvlan2_types;
pub mod linux_ipvs_types;
pub mod linux_irq;
pub mod linux_irq_domain_types;
pub mod linux_irq_types;
pub mod linux_irq_user_types;
pub mod linux_iscsi2_types;
pub mod linux_iscsi_types;
pub mod linux_iscsi_user_types;
pub mod linux_isdn_ppp_types;
pub mod linux_iso9660;
pub mod linux_iso9660_user_types;
pub mod linux_isst_types;
pub mod linux_itimer2_types;
pub mod linux_j1939_2_types;
pub mod linux_jffs2;
pub mod linux_jffs2_types;
pub mod linux_jffs2_user_types;
pub mod linux_jfs_types;
pub mod linux_jiffies_types;
pub mod linux_joystick;
pub mod linux_joystick_types;
pub mod linux_joystick_user_types;
pub mod linux_kasan_types;
pub mod linux_kcm2_types;
pub mod linux_kcmp;
pub mod linux_kcmp2_types;
pub mod linux_kcmp_types;
pub mod linux_kcmp_user_types;
pub mod linux_kcov;
pub mod linux_kcov_types;
pub mod linux_kcov_user_types;
pub mod linux_kcsan_types;
pub mod linux_kd;
pub mod linux_kd2_types;
pub mod linux_kd_types;
pub mod linux_kd_user_types;
pub mod linux_kdebug;
pub mod linux_kdebug_user_types;
pub mod linux_kdump_types;
pub mod linux_kexec;
pub mod linux_kexec2_types;
pub mod linux_kexec_types;
pub mod linux_kexec_user_types;
pub mod linux_kfd_ioctl_types;
pub mod linux_kfd_types;
pub mod linux_key;
pub mod linux_key_types;
pub mod linux_key_user_types;
pub mod linux_keyctl;
pub mod linux_keyctl2_types;
pub mod linux_keyctl_types;
pub mod linux_keyctl_user_types;
pub mod linux_keyring;
pub mod linux_keyring_types;
pub mod linux_keyring_user_types;
pub mod linux_keys2_types;
pub mod linux_klog_types;
pub mod linux_kmemleak_types;
pub mod linux_kmod;
pub mod linux_kmod_types;
pub mod linux_kms;
pub mod linux_kmsg_types;
pub mod linux_kobject;
pub mod linux_kobject_types;
pub mod linux_kprobe_types;
pub mod linux_kprobes_types;
pub mod linux_ksm2_types;
pub mod linux_ksm_types;
pub mod linux_kunit_types;
pub mod linux_kvm;
pub mod linux_kvm_arm_types;
pub mod linux_kvm_para_types;
pub mod linux_kvm_riscv_types;
pub mod linux_kvm_types;
pub mod linux_kvm_user_types;
pub mod linux_l2tp;
pub mod linux_l2tp2_types;
pub mod linux_l2tp3_types;
pub mod linux_l2tp_types;
pub mod linux_l2tp_user_types;
pub mod linux_landlock;
pub mod linux_landlock2_types;
pub mod linux_landlock3_types;
pub mod linux_landlock4_types;
pub mod linux_landlock_access_types;
pub mod linux_landlock_fs_types;
pub mod linux_landlock_rule_types;
pub mod linux_landlock_types;
pub mod linux_landlock_user_types;
pub mod linux_langinfo_types;
pub mod linux_lapb_types;
pub mod linux_ldconfig_types;
pub mod linux_ldt_types;
pub mod linux_ldt_user_types;
pub mod linux_led2_types;
pub mod linux_led_types;
pub mod linux_leds;
pub mod linux_leds_types;
pub mod linux_leds_user_types;
pub mod linux_limits;
pub mod linux_limits_user_types;
pub mod linux_lirc;
pub mod linux_lirc_types;
pub mod linux_lirc_user_types;
pub mod linux_listmount_types;
pub mod linux_llc2_types;
pub mod linux_lldp_types;
pub mod linux_loadpin_types;
pub mod linux_locale_types;
pub mod linux_lockdown_types;
pub mod linux_lockdown_user_types;
pub mod linux_log_priority_types;
pub mod linux_login_types;
pub mod linux_loop;
pub mod linux_loop2_types;
pub mod linux_loop3_types;
pub mod linux_loop_types;
pub mod linux_loop_user_types;
pub mod linux_lsm;
pub mod linux_lsm_types;
pub mod linux_lsm_user_types;
pub mod linux_lustre_types;
pub mod linux_lwtunnel2_types;
pub mod linux_lwtunnel_types;
pub mod linux_macsec2_types;
pub mod linux_macsec_types;
pub mod linux_macsec_user_types;
pub mod linux_macvlan;
pub mod linux_macvlan2_types;
pub mod linux_macvlan_types;
pub mod linux_madvise2_types;
pub mod linux_madvise_types;
pub mod linux_madvise_user_types;
pub mod linux_magic;
pub mod linux_magic_user_types;
pub mod linux_mailbox;
pub mod linux_mailbox_types;
pub mod linux_mce2_types;
pub mod linux_mce_types;
pub mod linux_mctp;
pub mod linux_mctp2_types;
pub mod linux_mctp_serial_types;
pub mod linux_mctp_types;
pub mod linux_mctp_user_types;
pub mod linux_md2_types;
pub mod linux_md_types;
pub mod linux_mdev;
pub mod linux_mdev_types;
pub mod linux_mdio;
pub mod linux_mdio_types;
pub mod linux_media;
pub mod linux_media2_types;
pub mod linux_media3_types;
pub mod linux_media_types;
pub mod linux_mei;
pub mod linux_mei2_types;
pub mod linux_mei_types;
pub mod linux_mei_user_types;
pub mod linux_membarrier;
pub mod linux_membarrier2_types;
pub mod linux_membarrier_types;
pub mod linux_membarrier_user_types;
pub mod linux_memcg_types;
pub mod linux_memcontrol;
pub mod linux_memfd;
pub mod linux_memfd2_types;
pub mod linux_memfd_types;
pub mod linux_memfd_user_types;
pub mod linux_memory_cgroup_types;
pub mod linux_memory_hotplug_types;
pub mod linux_mempolicy_types;
pub mod linux_mempolicy_user_types;
pub mod linux_mfd;
pub mod linux_mfd_types;
pub mod linux_migrate;
pub mod linux_migrate_types;
pub mod linux_mii;
pub mod linux_mincore_types;
pub mod linux_minix_fs_types;
pub mod linux_minix_types;
pub mod linux_misc_cgroup;
pub mod linux_misc_device_types;
pub mod linux_mlock2_types;
pub mod linux_mlock_types;
pub mod linux_mlock_user_types;
pub mod linux_mm;
pub mod linux_mmap_flags_types;
pub mod linux_mmc;
pub mod linux_mmc2_types;
pub mod linux_mmc_ioctl_types;
pub mod linux_mmc_types;
pub mod linux_mmc_user_types;
pub mod linux_module;
pub mod linux_module2_types;
pub mod linux_module_flags_types;
pub mod linux_module_types;
pub mod linux_module_user_types;
pub mod linux_monetary_types;
pub mod linux_mount;
pub mod linux_mount2_types;
pub mod linux_mount3_types;
pub mod linux_mount_api;
pub mod linux_mount_api_types;
pub mod linux_mount_api_user_types;
pub mod linux_mount_attr_types;
pub mod linux_mount_attr_user_types;
pub mod linux_mount_namespace;
pub mod linux_mount_setattr_types;
pub mod linux_mount_types;
pub mod linux_mount_user_types;
pub mod linux_move_mount_types;
pub mod linux_move_mount_user_types;
pub mod linux_mpls;
pub mod linux_mpls2_types;
pub mod linux_mpls3_types;
pub mod linux_mpls_types;
pub mod linux_mpls_user_types;
pub mod linux_mptcp2_types;
pub mod linux_mptcp_diag_types;
pub mod linux_mptcp_pm_types;
pub mod linux_mptcp_types;
pub mod linux_mptcp_user_types;
pub mod linux_mqattr_types;
pub mod linux_mqueue;
pub mod linux_mqueue2_types;
pub mod linux_mqueue_types;
pub mod linux_mqueue_user_types;
pub mod linux_mremap2_types;
pub mod linux_mremap_types;
pub mod linux_mremap_user_types;
pub mod linux_mroute2_types;
pub mod linux_mroute_types;
pub mod linux_mroute_user_types;
pub mod linux_msg_flags_types;
pub mod linux_msgq_types;
pub mod linux_msi;
pub mod linux_msi_user_types;
pub mod linux_msync_types;
pub mod linux_mtd;
pub mod linux_mtd2_types;
pub mod linux_mtd3_types;
pub mod linux_mtd_types;
pub mod linux_mtd_user_types;
pub mod linux_mutex_types;
pub mod linux_n_tty_types;
pub mod linux_n_tty_user_types;
pub mod linux_namespaces;
pub mod linux_namespaces_user_types;
pub mod linux_nbd;
pub mod linux_nbd2_types;
pub mod linux_nbd3_types;
pub mod linux_nbd_types;
pub mod linux_nbd_user_types;
pub mod linux_ndctl;
pub mod linux_ndctl_user_types;
pub mod linux_neigh_types;
pub mod linux_neighbor_types;
pub mod linux_neighbor_user_types;
pub mod linux_neighbour;
pub mod linux_net;
pub mod linux_net_cls_types;
pub mod linux_net_device_types;
pub mod linux_net_namespace;
pub mod linux_net_namespace_types;
pub mod linux_net_tstamp;
pub mod linux_net_tstamp2_types;
pub mod linux_net_tstamp_types;
pub mod linux_net_user_types;
pub mod linux_netconf2_types;
pub mod linux_netdev;
pub mod linux_netdev2_types;
pub mod linux_netdev_user_types;
pub mod linux_netdevice_flags_types;
pub mod linux_netem_types;
pub mod linux_netem_user_types;
pub mod linux_netfilter;
pub mod linux_netfilter2_types;
pub mod linux_netfilter3_types;
pub mod linux_netfilter_arp;
pub mod linux_netfilter_bridge;
pub mod linux_netfilter_ipv4;
pub mod linux_netfilter_ipv6;
pub mod linux_netfilter_types;
pub mod linux_netfilter_user_types;
pub mod linux_netlabel_types;
pub mod linux_netlink;
pub mod linux_netlink2_types;
pub mod linux_netlink4_types;
pub mod linux_netlink_attr_types;
pub mod linux_netlink_msg_types;
pub mod linux_netlink_route;
pub mod linux_netlink_types;
pub mod linux_netlink_user_types;
pub mod linux_netns_types;
pub mod linux_netpoll_types;
pub mod linux_netpoll_user_types;
pub mod linux_netrom2_types;
pub mod linux_netrom_types;
pub mod linux_nexthop2_types;
pub mod linux_nexthop_types;
pub mod linux_nexthop_user_types;
pub mod linux_nf_conntrack;
pub mod linux_nf_conntrack2_types;
pub mod linux_nf_conntrack3_types;
pub mod linux_nf_conntrack_types;
pub mod linux_nf_conntrack_user_types;
pub mod linux_nf_hook_types;
pub mod linux_nf_log_types;
pub mod linux_nf_nat;
pub mod linux_nf_nat_types;
pub mod linux_nf_table_types;
pub mod linux_nf_tables;
pub mod linux_nf_tables2_types;
pub mod linux_nf_tables3_types;
pub mod linux_nf_tables_user_types;
pub mod linux_nf_verdict_types;
pub mod linux_nfc;
pub mod linux_nfc2_types;
pub mod linux_nfc_types;
pub mod linux_nfc_user_types;
pub mod linux_nflog_types;
pub mod linux_nfnetlink2_types;
pub mod linux_nfnetlink_types;
pub mod linux_nfqueue_types;
pub mod linux_nfs2_types;
pub mod linux_nfs4_types;
pub mod linux_nfs_types;
pub mod linux_nfsd_types;
pub mod linux_nft_types;
pub mod linux_nftables;
pub mod linux_nice_types;
pub mod linux_nilfs2_ondisk_types;
pub mod linux_nilfs2_types;
pub mod linux_nitro_enclaves_types;
pub mod linux_nl80211;
pub mod linux_nl80211_3_types;
pub mod linux_nl80211_user_types;
pub mod linux_nl_types;
pub mod linux_nmi_types;
pub mod linux_nmi_user_types;
pub mod linux_notifier;
pub mod linux_ns2_types;
pub mod linux_ns_types;
pub mod linux_nsfs;
pub mod linux_nss_types;
pub mod linux_ntfs_types;
pub mod linux_ntp_types;
pub mod linux_ntp_user_types;
pub mod linux_ntsync_types;
pub mod linux_null_blk_types;
pub mod linux_numa;
pub mod linux_numa2_types;
pub mod linux_numa_types;
pub mod linux_numa_user_types;
pub mod linux_nvme;
pub mod linux_nvme2_types;
pub mod linux_nvme3_types;
pub mod linux_nvme_admin_types;
pub mod linux_nvme_fabrics_types;
pub mod linux_nvme_feat_types;
pub mod linux_nvme_ioctl;
pub mod linux_nvme_ioctl_types;
pub mod linux_nvme_ioctl_user_types;
pub mod linux_nvme_ns_types;
pub mod linux_nvme_opcode_types;
pub mod linux_nvme_queue_types;
pub mod linux_nvme_status_types;
pub mod linux_nvme_tcp;
pub mod linux_nvme_types;
pub mod linux_nvmem;
pub mod linux_ocfs2_types;
pub mod linux_of;
pub mod linux_omap_dss_types;
pub mod linux_oom;
pub mod linux_oom_types;
pub mod linux_oom_user_types;
pub mod linux_open_flags_types;
pub mod linux_open_tree_types;
pub mod linux_openat2;
pub mod linux_openat2_types;
pub mod linux_openat2_user_types;
pub mod linux_opp;
pub mod linux_opp_types;
pub mod linux_orangefs_types;
pub mod linux_overlayfs;
pub mod linux_overlayfs_types;
pub mod linux_overlayfs_user_types;
pub mod linux_ovs_types;
pub mod linux_packet3_types;
pub mod linux_packet_diag_types;
pub mod linux_packet_types;
pub mod linux_packet_user_types;
pub mod linux_page_flags_types;
pub mod linux_pam_types;
pub mod linux_panic;
pub mod linux_panic_user_types;
pub mod linux_papr_miscdev_types;
pub mod linux_papr_pdsm_types;
pub mod linux_parport_types;
pub mod linux_passwd_types;
pub mod linux_pathconf_types;
pub mod linux_pci;
pub mod linux_pci2_types;
pub mod linux_pci_cap_types;
pub mod linux_pci_capability_types;
pub mod linux_pci_class_types;
pub mod linux_pci_command_types;
pub mod linux_pci_config_types;
pub mod linux_pci_doe_types;
pub mod linux_pci_express_types;
pub mod linux_pci_ids;
pub mod linux_pci_ids_types;
pub mod linux_pci_msi_types;
pub mod linux_pci_pm_types;
pub mod linux_pci_power_types;
pub mod linux_pci_regs;
pub mod linux_pci_regs_types;
pub mod linux_pci_types;
pub mod linux_pci_user_types;
pub mod linux_pcie_types;
pub mod linux_percpu_types;
pub mod linux_perf2_types;
pub mod linux_perf3_types;
pub mod linux_perf_attr_types;
pub mod linux_perf_cgroup;
pub mod linux_perf_event;
pub mod linux_perf_event2_types;
pub mod linux_perf_event_user_types;
pub mod linux_perf_format_types;
pub mod linux_perf_hw_types;
pub mod linux_perf_ioctl_types;
pub mod linux_perf_mmap_types;
pub mod linux_perf_sample_types;
pub mod linux_perf_sw_types;
pub mod linux_perf_types;
pub mod linux_personality;
pub mod linux_personality2_types;
pub mod linux_personality_types;
pub mod linux_personality_user_types;
pub mod linux_pfkey_types;
pub mod linux_pfkey_user_types;
pub mod linux_pgroup_types;
pub mod linux_phonet;
pub mod linux_phonet2_types;
pub mod linux_phonet_types;
pub mod linux_phonet_user_types;
pub mod linux_phy;
pub mod linux_phy_types;
pub mod linux_phylink;
pub mod linux_pid_namespace;
pub mod linux_pid_namespace_types;
pub mod linux_pidfd;
pub mod linux_pidfd2_types;
pub mod linux_pidfd3_types;
pub mod linux_pidfd_open_types;
pub mod linux_pidfd_types;
pub mod linux_pidfd_user_types;
pub mod linux_pidns_types;
pub mod linux_pids_cgroup;
pub mod linux_pids_cgroup_types;
pub mod linux_pinconf_types;
pub mod linux_pinctrl;
pub mod linux_pinctrl_types;
pub mod linux_pinmux_types;
pub mod linux_pipe2_types;
pub mod linux_pipe2_user_types;
pub mod linux_pkey_types;
pub mod linux_pkey_user_types;
pub mod linux_pkt_cls_types;
pub mod linux_pkt_cls_user_types;
pub mod linux_pkt_sched;
pub mod linux_pkt_sched_user_types;
pub mod linux_pktcdvd_types;
pub mod linux_pktgen_types;
pub mod linux_platform_device;
pub mod linux_platform_device_types;
pub mod linux_platform_types;
pub mod linux_pm_qos;
pub mod linux_pm_qos_types;
pub mod linux_pm_runtime;
pub mod linux_pm_runtime_types;
pub mod linux_pmu_types;
pub mod linux_posix_acl;
pub mod linux_posix_acl_types;
pub mod linux_posix_acl_user_types;
pub mod linux_posix_fadvise_types;
pub mod linux_posix_sem_types;
pub mod linux_posix_timer_types;
pub mod linux_posix_timers;
pub mod linux_posix_timers2_types;
pub mod linux_posix_timers_types;
pub mod linux_posix_types;
pub mod linux_power2_types;
pub mod linux_power_supply;
pub mod linux_power_supply2_types;
pub mod linux_power_supply_types;
pub mod linux_powercap_types;
pub mod linux_ppdev_types;
pub mod linux_ppdev_user_types;
pub mod linux_ppp;
pub mod linux_ppp2_types;
pub mod linux_ppp3_types;
pub mod linux_ppp_defs;
pub mod linux_ppp_types;
pub mod linux_ppp_user_types;
pub mod linux_pps_types;
pub mod linux_pr_types;
pub mod linux_prandom_types;
pub mod linux_prctl;
pub mod linux_prctl2_types;
pub mod linux_prctl3_types;
pub mod linux_prctl_cap_types;
pub mod linux_prctl_mm_types;
pub mod linux_prctl_types;
pub mod linux_prctl_user_types;
pub mod linux_preadv2_types;
pub mod linux_printk;
pub mod linux_printk_types;
pub mod linux_prlimit_types;
pub mod linux_proc_ns;
pub mod linux_proc_types;
pub mod linux_process_madvise_types;
pub mod linux_process_vm_types;
pub mod linux_procfs;
pub mod linux_property;
pub mod linux_psci;
pub mod linux_psi;
pub mod linux_psi_types;
pub mod linux_psi_user_types;
pub mod linux_psp_sev_types;
pub mod linux_psp_types;
pub mod linux_pthread_barrier_types;
pub mod linux_pthread_cond_types;
pub mod linux_pthread_key_types;
pub mod linux_pthread_mutex_types;
pub mod linux_pthread_rwlock_types;
pub mod linux_pthread_spinlock_types;
pub mod linux_ptp;
pub mod linux_ptp2_types;
pub mod linux_ptp_clock_types;
pub mod linux_ptrace;
pub mod linux_ptrace2_types;
pub mod linux_ptrace_regs_types;
pub mod linux_ptrace_request_types;
pub mod linux_ptrace_types;
pub mod linux_ptrace_user_types;
pub mod linux_pty_types;
pub mod linux_pty_user_types;
pub mod linux_pwm;
pub mod linux_pwm3_types;
pub mod linux_pwm_types;
pub mod linux_pwm_user_types;
pub mod linux_qdisc_fq_codel_types;
pub mod linux_qdisc_fq_types;
pub mod linux_qdisc_hfsc_types;
pub mod linux_qdisc_htb_types;
pub mod linux_qdisc_prio_types;
pub mod linux_qdisc_red_types;
pub mod linux_qdisc_sfq_types;
pub mod linux_qdisc_tbf_types;
pub mod linux_qdisc_types;
pub mod linux_qnx4_types;
pub mod linux_qnx6_types;
pub mod linux_qrtr_types;
pub mod linux_quota;
pub mod linux_quota2_types;
pub mod linux_quota3_types;
pub mod linux_quota_cmd_types;
pub mod linux_quota_types;
pub mod linux_quota_user_types;
pub mod linux_raid_types;
pub mod linux_random;
pub mod linux_random_types;
pub mod linux_random_user_types;
pub mod linux_ras_types;
pub mod linux_raw_socket_types;
pub mod linux_raw_types;
pub mod linux_rbd_types;
pub mod linux_rc_types;
pub mod linux_rcu;
pub mod linux_rcu_types;
pub mod linux_rdma_types;
pub mod linux_rds2_types;
pub mod linux_rds_rdma_types;
pub mod linux_readahead;
pub mod linux_readahead_types;
pub mod linux_reboot;
pub mod linux_reboot2_types;
pub mod linux_reboot_types;
pub mod linux_reboot_user_types;
pub mod linux_reclaim_types;
pub mod linux_regex_types;
pub mod linux_regmap;
pub mod linux_regulator;
pub mod linux_regulator2_types;
pub mod linux_regulator3_types;
pub mod linux_regulator_types;
pub mod linux_reiserfs_types;
pub mod linux_remoteproc;
pub mod linux_remoteproc_cdev_types;
pub mod linux_remoteproc_types;
pub mod linux_resctrl_types;
pub mod linux_reset;
pub mod linux_reset_types;
pub mod linux_resource;
pub mod linux_resource_types;
pub mod linux_rfkill;
pub mod linux_rfkill2_types;
pub mod linux_rfkill_types;
pub mod linux_rfkill_user_types;
pub mod linux_riscv_hwprobe_types;
pub mod linux_rkisp1_types;
pub mod linux_rlimit;
pub mod linux_rlimit2_types;
pub mod linux_rlimit_types;
pub mod linux_rlimit_user_types;
pub mod linux_rmnet_types;
pub mod linux_rnbd_types;
pub mod linux_rng_types;
pub mod linux_robust_list_types;
pub mod linux_romfs;
pub mod linux_romfs_types;
pub mod linux_rose2_types;
pub mod linux_rose_types;
pub mod linux_rpmsg;
pub mod linux_rpmsg_types;
pub mod linux_rseq;
pub mod linux_rseq2_types;
pub mod linux_rseq_types;
pub mod linux_rseq_user_types;
pub mod linux_rt_sched_types;
pub mod linux_rtc;
pub mod linux_rtc2_types;
pub mod linux_rtc_user_types;
pub mod linux_rtnetlink;
pub mod linux_rtnetlink_types;
pub mod linux_rtnetlink_user_types;
pub mod linux_rusage_types;
pub mod linux_rwlock_types;
pub mod linux_rwsem_types;
pub mod linux_rxrpc2_types;
pub mod linux_sadb_types;
pub mod linux_safesetid_types;
pub mod linux_sas_types;
pub mod linux_sched;
pub mod linux_sched2_types;
pub mod linux_sched3_types;
pub mod linux_sched_attr_types;
pub mod linux_sched_debug_types;
pub mod linux_sched_ext;
pub mod linux_sched_ext_types;
pub mod linux_sched_param_types;
pub mod linux_sched_policy_types;
pub mod linux_sched_types;
pub mod linux_sched_user_types;
pub mod linux_scm_types;
pub mod linux_scsi;
pub mod linux_scsi2_types;
pub mod linux_scsi3_types;
pub mod linux_scsi_device_types;
pub mod linux_scsi_host_types;
pub mod linux_scsi_ioctl_types;
pub mod linux_scsi_opcode_types;
pub mod linux_scsi_sense_types;
pub mod linux_scsi_status_types;
pub mod linux_scsi_transport_types;
pub mod linux_scsi_types;
pub mod linux_scsi_user_types;
pub mod linux_sctp;
pub mod linux_sctp2_types;
pub mod linux_sctp_diag_types;
pub mod linux_sctp_types;
pub mod linux_sctp_user_types;
pub mod linux_sdei_types;
pub mod linux_sdio;
pub mod linux_secbit_types;
pub mod linux_secbits_types;
pub mod linux_seccomp;
pub mod linux_seccomp2_types;
pub mod linux_seccomp3_types;
pub mod linux_seccomp_action_types;
pub mod linux_seccomp_filter;
pub mod linux_seccomp_filter_types;
pub mod linux_seccomp_types;
pub mod linux_seccomp_user_types;
pub mod linux_secretmem_types;
pub mod linux_securebit;
pub mod linux_securebits;
pub mod linux_securebits_types;
pub mod linux_securebits_user_types;
pub mod linux_security_xattr_types;
pub mod linux_sed_opal_types;
pub mod linux_seek_types;
pub mod linux_seek_user_types;
pub mod linux_seg6;
pub mod linux_seg6_2_types;
pub mod linux_seg6_types;
pub mod linux_selinux_types;
pub mod linux_selinux_user_types;
pub mod linux_sem_types;
pub mod linux_sem_user_types;
pub mod linux_sendfile;
pub mod linux_sendfile2_types;
pub mod linux_sendfile_types;
pub mod linux_seqlock_types;
pub mod linux_serdev;
pub mod linux_serdev_types;
pub mod linux_serial;
pub mod linux_serial2_types;
pub mod linux_serial_types;
pub mod linux_serial_user_types;
pub mod linux_session_types;
pub mod linux_session_user_types;
pub mod linux_set_mempolicy_types;
pub mod linux_setns_types;
pub mod linux_setuid_types;
pub mod linux_sev_types;
pub mod linux_sfp;
pub mod linux_sg2_types;
pub mod linux_shadow_types;
pub mod linux_shm2_types;
pub mod linux_shmem_types;
pub mod linux_siginfo_types;
pub mod linux_signal_action_types;
pub mod linux_signal_num_types;
pub mod linux_signal_types;
pub mod linux_signal_user_types;
pub mod linux_signalfd;
pub mod linux_signalfd2_types;
pub mod linux_signalfd3_types;
pub mod linux_signalfd_types;
pub mod linux_signalfd_user_types;
pub mod linux_sigset_types;
pub mod linux_siox_types;
pub mod linux_sit_types;
pub mod linux_skbuff_types;
pub mod linux_skbuff_user_types;
pub mod linux_slab_types;
pub mod linux_slimbus_types;
pub mod linux_slip;
pub mod linux_slip_types;
pub mod linux_smack2_types;
pub mod linux_smack_types;
pub mod linux_smackfs_types;
pub mod linux_smb2_types;
pub mod linux_smb_types;
pub mod linux_smbios_types;
pub mod linux_smc;
pub mod linux_smc2_types;
pub mod linux_smc_diag_types;
pub mod linux_smc_types;
pub mod linux_smc_user_types;
pub mod linux_snd_compress_types;
pub mod linux_snd_ctl_types;
pub mod linux_snd_firewire_types;
pub mod linux_snd_hda_types;
pub mod linux_snd_jack_types;
pub mod linux_snd_pcm_format_types;
pub mod linux_snd_rawmidi_types;
pub mod linux_snd_seq_types;
pub mod linux_snd_usb_audio_types;
pub mod linux_so_types;
pub mod linux_sock_cgroup_types;
pub mod linux_sock_diag;
pub mod linux_sock_diag2_types;
pub mod linux_sock_diag_types;
pub mod linux_sock_diag_user_types;
pub mod linux_socket_types;
pub mod linux_sockios;
pub mod linux_sockios_user_types;
pub mod linux_softirq_types;
pub mod linux_sound;
pub mod linux_sound_types;
pub mod linux_soundwire_types;
pub mod linux_spi;
pub mod linux_spi2_types;
pub mod linux_spi3_types;
pub mod linux_spi_types;
pub mod linux_splice;
pub mod linux_splice2_types;
pub mod linux_splice_types;
pub mod linux_squashfs;
pub mod linux_squashfs_fs_types;
pub mod linux_squashfs_types;
pub mod linux_sr_types;
pub mod linux_stack_types;
pub mod linux_stat;
pub mod linux_statfs_types;
pub mod linux_statmount_types;
pub mod linux_statvfs_types;
pub mod linux_statx;
pub mod linux_statx2_types;
pub mod linux_statx_types;
pub mod linux_statx_user_types;
pub mod linux_stddef;
pub mod linux_superblock_types;
pub mod linux_surface_aggregator;
pub mod linux_suspend;
pub mod linux_suspend2_types;
pub mod linux_suspend_types;
pub mod linux_sw_sync_types;
pub mod linux_swap;
pub mod linux_swap2_types;
pub mod linux_swap_types;
pub mod linux_swap_user_types;
pub mod linux_switchdev;
pub mod linux_switchdev_types;
pub mod linux_sync_file;
pub mod linux_sync_file2_types;
pub mod linux_sync_file_range;
pub mod linux_sync_file_types;
pub mod linux_sysconf_types;
pub mod linux_sysctl;
pub mod linux_sysctl_types;
pub mod linux_sysfs;
pub mod linux_sysfs_types;
pub mod linux_sysinfo;
pub mod linux_sysinfo2_types;
pub mod linux_sysinfo_types;
pub mod linux_syslog2_types;
pub mod linux_syslog_action_types;
pub mod linux_syslog_types;
pub mod linux_syslog_user_types;
pub mod linux_sysv_types;
pub mod linux_sysvipc_msg_types;
pub mod linux_sysvipc_sem_types;
pub mod linux_sysvipc_shm_types;
pub mod linux_sysvsem_types;
pub mod linux_target2_types;
pub mod linux_target_core;
pub mod linux_target_core_user_types;
pub mod linux_target_types;
pub mod linux_tasklet_types;
pub mod linux_taskstats;
pub mod linux_taskstats_types;
pub mod linux_taskstats_user_types;
pub mod linux_tc2_types;
pub mod linux_tc3_types;
pub mod linux_tc_act;
pub mod linux_tc_act_types;
pub mod linux_tc_action_types;
pub mod linux_tc_actions;
pub mod linux_tc_csum;
pub mod linux_tc_ct;
pub mod linux_tc_ct_types;
pub mod linux_tc_filter_types;
pub mod linux_tc_gact_types;
pub mod linux_tc_gate_types;
pub mod linux_tc_mirred;
pub mod linux_tc_mirred_types;
pub mod linux_tc_mpls_types;
pub mod linux_tc_nat_types;
pub mod linux_tc_pedit;
pub mod linux_tc_pedit_types;
pub mod linux_tc_police;
pub mod linux_tc_police_types;
pub mod linux_tc_sample_types;
pub mod linux_tc_skbedit;
pub mod linux_tc_skbmod_types;
pub mod linux_tc_tunnel_key;
pub mod linux_tc_tunnel_key_types;
pub mod linux_tc_types;
pub mod linux_tc_vlan;
pub mod linux_tc_vlan_types;
pub mod linux_tcp;
pub mod linux_tcp_congestion_types;
pub mod linux_tcp_diag_types;
pub mod linux_tcp_info_types;
pub mod linux_tcp_metrics_types;
pub mod linux_tcp_opt_types;
pub mod linux_tcp_states;
pub mod linux_tcp_states_types;
pub mod linux_tcp_user_types;
pub mod linux_tdx_guest_types;
pub mod linux_team2_types;
pub mod linux_team_types;
pub mod linux_tee;
pub mod linux_tee2_types;
pub mod linux_tee_types;
pub mod linux_termios_cc_types;
pub mod linux_termios_types;
pub mod linux_termios_user_types;
pub mod linux_thermal;
pub mod linux_thermal2_types;
pub mod linux_thermal3_types;
pub mod linux_thermal_types;
pub mod linux_thermal_zone_types;
pub mod linux_thp_types;
pub mod linux_thp_user_types;
pub mod linux_thread_types;
pub mod linux_thunderbolt;
pub mod linux_thunderbolt2_types;
pub mod linux_thunderbolt_types;
pub mod linux_tick_types;
pub mod linux_time;
pub mod linux_time_namespace;
pub mod linux_time_namespace_types;
pub mod linux_time_user_types;
pub mod linux_timecounter_types;
pub mod linux_timekeeper_types;
pub mod linux_timer2_types;
pub mod linux_timer_types;
pub mod linux_timerfd;
pub mod linux_timerfd2_types;
pub mod linux_timerfd3_types;
pub mod linux_timerfd_types;
pub mod linux_timerfd_user_types;
pub mod linux_times_types;
pub mod linux_timespec_types;
pub mod linux_timeval_types;
pub mod linux_tiocm_types;
pub mod linux_tipc;
pub mod linux_tipc3_types;
pub mod linux_tipc_config_types;
pub mod linux_tipc_types;
pub mod linux_tipc_user_types;
pub mod linux_tls;
pub mod linux_tls3_types;
pub mod linux_tls_thread_types;
pub mod linux_tls_types;
pub mod linux_tls_user_types;
pub mod linux_tmpfs;
pub mod linux_tmpfs_types;
pub mod linux_token_ring_types;
pub mod linux_tomoyo_types;
pub mod linux_tomoyo_user_types;
pub mod linux_topology;
pub mod linux_tpm2_types;
pub mod linux_tpm_types;
pub mod linux_tpm_user_types;
pub mod linux_trace;
pub mod linux_trace_event_types;
pub mod linux_trace_marker_types;
pub mod linux_tracefs;
pub mod linux_tracefs_types;
pub mod linux_tracepoint_types;
pub mod linux_tracing2_types;
pub mod linux_trusted_xattr_types;
pub mod linux_tty;
pub mod linux_tty2_types;
pub mod linux_tty_driver_types;
pub mod linux_tty_flags_types;
pub mod linux_tty_ldisc_types;
pub mod linux_tty_types;
pub mod linux_tty_user_types;
pub mod linux_tun;
pub mod linux_tun2_types;
pub mod linux_tun_user_types;
pub mod linux_tunnel_types;
pub mod linux_typec;
pub mod linux_ubi;
pub mod linux_ubifs;
pub mod linux_ubifs_types;
pub mod linux_ublk_types;
pub mod linux_ublk_user_types;
pub mod linux_udf_types;
pub mod linux_udmabuf;
pub mod linux_udmabuf2_types;
pub mod linux_udmabuf_types;
pub mod linux_udp;
pub mod linux_udp_opt_types;
pub mod linux_udp_types;
pub mod linux_udp_user_types;
pub mod linux_uevent_types;
pub mod linux_uhid_types;
pub mod linux_uhid_user_types;
pub mod linux_uid_gid_types;
pub mod linux_uinput;
pub mod linux_uinput2_types;
pub mod linux_uinput_types;
pub mod linux_uinput_user_types;
pub mod linux_uio;
pub mod linux_uleds_types;
pub mod linux_uleds_user_types;
pub mod linux_uname_types;
pub mod linux_unistd;
pub mod linux_unix_diag2_types;
pub mod linux_unix_diag_types;
pub mod linux_unix_socket_types;
pub mod linux_unix_types;
pub mod linux_unix_user_types;
pub mod linux_unshare_types;
pub mod linux_unwind_types;
pub mod linux_uprobe_types;
pub mod linux_uprobes;
pub mod linux_uprobes_types;
pub mod linux_usb;
pub mod linux_usb4_types;
pub mod linux_usb_ch9;
pub mod linux_usb_ch9_types;
pub mod linux_usb_class_types;
pub mod linux_usb_descriptor_types;
pub mod linux_usb_endpoint_types;
pub mod linux_usb_gadget;
pub mod linux_usb_gadget_types;
pub mod linux_usb_hub_types;
pub mod linux_usb_pd;
pub mod linux_usb_speed_types;
pub mod linux_usb_transfer_types;
pub mod linux_usb_types;
pub mod linux_usbdevice_fs_user_types;
pub mod linux_user_events_types;
pub mod linux_user_namespace;
pub mod linux_user_namespace_types;
pub mod linux_user_xattr_types;
pub mod linux_userfaultfd;
pub mod linux_userfaultfd2_types;
pub mod linux_userfaultfd3_types;
pub mod linux_userfaultfd_types;
pub mod linux_userfaultfd_user_types;
pub mod linux_userio_types;
pub mod linux_userns_types;
pub mod linux_utmp_types;
pub mod linux_uts_namespace;
pub mod linux_uts_namespace_types;
pub mod linux_uts_user_types;
pub mod linux_utsname;
pub mod linux_utsname2_types;
pub mod linux_utsname_types;
pub mod linux_uuid;
pub mod linux_uuid_user_types;
pub mod linux_uvcvideo_types;
pub mod linux_v4l2_2_types;
pub mod linux_v4l2_3_types;
pub mod linux_v4l2_buf_types;
pub mod linux_v4l2_cap_types;
pub mod linux_v4l2_ctrl_types;
pub mod linux_v4l2_event_types;
pub mod linux_v4l2_field_types;
pub mod linux_v4l2_format_types;
pub mod linux_v4l2_jpeg_types;
pub mod linux_v4l2_memory_types;
pub mod linux_v4l2_types;
pub mod linux_v4l2_user_types;
pub mod linux_vbox_types;
pub mod linux_vdpa;
pub mod linux_vdpa_types;
pub mod linux_vdso2_types;
pub mod linux_vdso_elf_types;
pub mod linux_vdso_types;
pub mod linux_vduse_types;
pub mod linux_verity_types;
pub mod linux_veth;
pub mod linux_veth2_types;
pub mod linux_veth_types;
pub mod linux_veth_user_types;
pub mod linux_vfio3_types;
pub mod linux_vfio_iommu_types;
pub mod linux_vfio_types;
pub mod linux_vfio_user_types;
pub mod linux_vfio_zdev_types;
pub mod linux_vgaarb;
pub mod linux_vhost;
pub mod linux_vhost2_types;
pub mod linux_vhost3_types;
pub mod linux_vhost_types;
pub mod linux_vhost_user_types;
pub mod linux_videodev2;
pub mod linux_videodev2_user_types;
pub mod linux_virtio2_types;
pub mod linux_virtio3_types;
pub mod linux_virtio_balloon;
pub mod linux_virtio_balloon_types;
pub mod linux_virtio_blk;
pub mod linux_virtio_blk_types;
pub mod linux_virtio_blk_user_types;
pub mod linux_virtio_config;
pub mod linux_virtio_config_types;
pub mod linux_virtio_console;
pub mod linux_virtio_console_types;
pub mod linux_virtio_crypto;
pub mod linux_virtio_fs;
pub mod linux_virtio_fs_types;
pub mod linux_virtio_gpu;
pub mod linux_virtio_gpu_types;
pub mod linux_virtio_ids_types;
pub mod linux_virtio_input;
pub mod linux_virtio_input_types;
pub mod linux_virtio_iommu_types;
pub mod linux_virtio_mem_types;
pub mod linux_virtio_net;
pub mod linux_virtio_net_types;
pub mod linux_virtio_pci;
pub mod linux_virtio_pci_types;
pub mod linux_virtio_ring;
pub mod linux_virtio_ring_types;
pub mod linux_virtio_scsi;
pub mod linux_virtio_scsi_types;
pub mod linux_virtio_types;
pub mod linux_virtio_user_types;
pub mod linux_virtio_vsock;
pub mod linux_virtio_vsock_types;
pub mod linux_vlan;
pub mod linux_vlan2_types;
pub mod linux_vlan_types;
pub mod linux_vlan_user_types;
pub mod linux_vm_sockets;
pub mod linux_vm_sockets2_types;
pub mod linux_vmalloc_types;
pub mod linux_vsock;
pub mod linux_vsock2_types;
pub mod linux_vsock_diag_types;
pub mod linux_vsock_types;
pub mod linux_vsock_user_types;
pub mod linux_vt;
pub mod linux_vt2_types;
pub mod linux_vt_kern;
pub mod linux_vt_types;
pub mod linux_vt_user_types;
pub mod linux_vxlan;
pub mod linux_vxlan2_types;
pub mod linux_vxlan_user_types;
pub mod linux_w1_types;
pub mod linux_wait;
pub mod linux_wait_types;
pub mod linux_wait_user_types;
pub mod linux_waitid_types;
pub mod linux_wakeup;
pub mod linux_wakeup_types;
pub mod linux_watch_queue;
pub mod linux_watch_queue_types;
pub mod linux_watchdog;
pub mod linux_watchdog2_types;
pub mod linux_watchdog3_types;
pub mod linux_watchdog_types;
pub mod linux_watchdog_user_types;
pub mod linux_wdt;
pub mod linux_winsize_types;
pub mod linux_wireguard;
pub mod linux_wireguard2_types;
pub mod linux_wireguard_types;
pub mod linux_wireguard_user_types;
pub mod linux_wireless;
pub mod linux_wireless_user_types;
pub mod linux_wl_keyboard_types;
pub mod linux_wl_output_types;
pub mod linux_wl_pointer_types;
pub mod linux_wl_seat_types;
pub mod linux_wl_shm_format_types;
pub mod linux_wl_touch_types;
pub mod linux_wmi;
pub mod linux_wmi_user_types;
pub mod linux_wordexp_types;
pub mod linux_workqueue;
pub mod linux_workqueue_types;
pub mod linux_workqueue_user_types;
pub mod linux_writeback2_types;
pub mod linux_writeback_types;
pub mod linux_writeback_user_types;
pub mod linux_wwan;
pub mod linux_wwan_user_types;
pub mod linux_x25_2_types;
pub mod linux_x25_types;
pub mod linux_x25_user_types;
pub mod linux_xattr;
pub mod linux_xattr2_types;
pub mod linux_xattr_flags_types;
pub mod linux_xattr_ns_types;
pub mod linux_xattr_types;
pub mod linux_xattr_user_types;
pub mod linux_xdp;
pub mod linux_xdp2_types;
pub mod linux_xdp3_types;
pub mod linux_xdp_diag_types;
pub mod linux_xdp_types;
pub mod linux_xdp_user_types;
pub mod linux_xen_privcmd_types;
pub mod linux_xfrm;
pub mod linux_xfrm2_types;
pub mod linux_xfrm3_types;
pub mod linux_xfrm_types;
pub mod linux_xfrm_user_types;
pub mod linux_xfs2_types;
pub mod linux_xfs_types;
pub mod linux_xfs_user_types;
pub mod linux_xt_target_types;
pub mod linux_yama_types;
pub mod linux_yama_user_types;
pub mod linux_zoned_storage_types;
pub mod linux_zonefs;
pub mod linux_zonefs_user_types;
pub mod linux_zram;
pub mod linux_zram_types;
pub mod linux_zram_user_types;
pub mod linux_zswap;
pub mod linux_zswap_types;
pub mod linux_zswap_user_types;
pub mod locale;
pub mod malloc;
pub mod math;
pub mod md5;
pub mod mman;
pub mod mntent;
pub mod monetary;
pub mod mqueue;
pub mod ndbm;
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
pub mod paths;
pub mod pipe;
pub mod poll;
pub mod printf;
pub mod process;
pub mod pthread;
pub mod pwd;
pub mod regex;
pub mod resolv;
pub mod resource;
pub mod scanf;
pub mod sched;
pub mod search;
pub mod semaphore;
pub mod setjmp;
pub mod sha2;
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
pub mod sysv_msg;
pub mod sysv_sem;
pub mod sysv_shm;
pub mod tar;
pub mod termios;
pub mod time;
pub mod types;
pub mod uchar;
pub mod ulimit;
pub mod unistd;
pub mod utime;
pub mod utmpx;
pub mod utsname;
pub mod values;
pub mod wait;
pub mod wchar;
pub mod wordexp;
pub mod xattr;
