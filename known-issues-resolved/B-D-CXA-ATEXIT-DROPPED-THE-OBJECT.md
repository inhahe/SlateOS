### [D] B-D-CXA-ATEXIT-DROPPED-THE-OBJECT — 2026-09-26 — FIXED 2026-09-26

**Where:** `posix/src/crt.rs` — `__cxa_atexit`, `__cxa_finalize`, `atexit`,
`on_exit`, `at_quick_exit`, `exit`, `quick_exit`,
`__cxa_thread_atexit_impl`, `__libc_start_main`; now built on
`posix/src/exit_list.rs`. Reported by lane A twice
(`requests/a-d-cxa-atexit-drops-this-so-cmake-dies-in-its-static-destructors.md`,
`requests/a-d-cxa-atexit-drops-the-object-so-static-destructors-run-on-null.md`).

**In short:** when a C++ program ends, the C library must run the destructor
of every global object and give each one its object. Ours threw the object
away, so every such destructor ran on a garbage pointer: CMake, the first large
C++ program to finish its work on SlateOS, crashed in its first one (lane A's
Path-Z CMake rung, exit -8). Several neighbouring defects sat in the same
thirty lines.

**What was wrong**, all of it silent:

| | was | now |
|---|---|---|
| `__cxa_atexit(f, obj, dso)` | `f` registered as a no-argument `atexit` handler; `obj` dropped, so `f` ran on whatever `rdi` held | `f(obj)`, as the Itanium C++ ABI requires |
| capacity | three fixed tables of 32; the 33rd registration of any kind returned -1, which no compiler checks, so those destructors never ran | 32 entries in place, then a `malloc`ed block that doubles; no limit |
| `on_exit(f, arg)` | stored in a table `exit` never read | `f(status, arg)`, in the one reverse order with the rest |
| order | `atexit` and `__cxa_atexit` entries in one table, `on_exit` in another | one list per kind of exit, glibc's `__exit_funcs`, newest first across every kind |
| a handler that registers another | the new one was never run (the count was read once) | it runs next, as glibc's `__run_exit_handlers` does |
| `__cxa_finalize(dso)` | a no-op | runs the module's destructors newest first and marks them done, so `exit` does not run them twice; NULL runs every termination function |
| `.fini_array` | registered *after* the constructors, so it ran before every destructor a constructor registered | registered first, as glibc registers `call_fini`, so it runs last |
| `__cxa_thread_atexit_impl` | accepted, and the destructor never run | a per-thread list, run by `pthread_exit` (a returning thread reaches it too) and, for the calling thread, first thing in `exit`; out of memory aborts with a message, as glibc does |
| a NULL handler | undefined behaviour in the Rust signature | refused with -1 and `EINVAL`, nothing registered; a NULL thread destructor ends the process (the thirty-ninth NULL-pointer pass, design-decisions.md §1115) |

**Tests.** `exit_list`'s host tests: the object reaches its destructor; every
kind runs newest first with its arguments; 1000 entries; a handler registered
during exit runs next; `__cxa_finalize` per module, once, and for NULL; a
restart when a destructor registers another; finalised entries reused rather
than piled up; `quick_exit`'s list separate; `thread_local` destructors newest
first, per thread, every node freed. `crt.rs` repeats the regression pins at
the ABI (`__cxa_atexit`, `on_exit`, 280 registrations, a NULL handler). The
ring-3 check is lane A's Path-Z CMake rung, which this change should turn
green.

**Not glibc's, and not visible yet.** glibc's `atexit` passes its caller's
`__dso_handle`; ours cannot, so `__cxa_finalize(dso)` for one module never
runs an `atexit` handler that module registered. Nothing unloads modules
(`dlclose` is a stub), so nothing can see the difference until something does.
