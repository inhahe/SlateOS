## D-POSIX-DL-ITERATE-PHDR-NEVER-CALLED-BACK-SO-NO-CXX-THROW-COULD-BE-CAUGHT — `dl_iterate_phdr` returned 0 without calling its callback, so the unwinder C++ programs link found no unwind tables and every `throw` ended in `std::terminate`; `dlopen(NULL)` gave no handle, and `dlerror` was one slot for the whole process (lane D, 2026-09-29) — **Status: FIXED 2026-09-29 (`posix/src/dlfcn.rs`, `posix/src/tls.rs`), on the host; on the target once lane A runs `ctest-cxx-throw` (`requests/d-a-run-ctest-cxx-throw.md`)**

**In short:** a C++ program on SlateOS could throw an exception but never
catch one. The unwinder a C++ program is linked with (zig's libunwind) finds
the tables that say how to unwind each function's stack frame by asking the C
library for the list of loaded objects -- here, the program itself -- and the
C library answered with an empty list. So the unwinder concluded there was
nothing it could unwind, and every `throw` called `std::terminate`, ending the
program. Linking always succeeded, which is why nothing noticed: no C++
program had been run here (B-THE-C-PLUS-PLUS-LINK-LINE-NEEDS-TWO-DECISIONS-AND-ONE-MISSING-FAMILY
says so), and cmake, the first large one ported, uses exceptions.

| Call | Was | Is |
|---|---|---|
| `dl_iterate_phdr` | 0, the callback never called | one call, for the program: its program headers (found through `__ehdr_start`, as the TLS set-up finds them), load bias, name "", TLS module 1 and the calling thread's block of it; the callback's answer |
| `_dl_find_object` (glibc 2.35) | absent | the program's segment holding an address, and its `.eh_frame_hdr` -- what GCC's unwinder asks, where glibc has it |
| `__tls_get_addr` | NULL for everything | module 1's variable in the calling thread's block |
| `dlopen(NULL)`, `dlopen("")` | NULL, "dynamic linking not supported" | the program's handle, which POSIX requires for NULL |
| `dlsym`, `dlvsym` | NULL, the same message | NULL, `<program>: undefined symbol: <name>`, glibc's static answer |
| `dlclose` of the program's handle | -1 | 0 |
| `dlerror` | one slot for the process: a thread could read, or clear, another's message | the calling thread's own, as in glibc |
| `dlinfo` | -1 for every request | the program's link map, namespace, directory, TLS module and block, program headers; glibc's refusals for the rest |
| `dladdr` | 0 for every address | for one inside the program, its name and ELF header (design-decisions §1147) |
| `dlmopen`, `dladdr1` | absent | glibc's static answers |

**Found** reading `dlfcn.rs` for the `dladdr1`, `dlmopen`, `dlvsym` row of
D-POSIX-LIBC-LACKS-WHAT-GLIBCS-HEADERS-DECLARE: zig's `libunwind.a` has
`dl_iterate_phdr` as an undefined symbol (`llvm-nm`), and LLVM's libunwind,
built for Linux, has no other way to find an object's `PT_GNU_EH_FRAME`.

**A second condition, for whoever links C++ by hand:** the unwinder also needs
the program to have been linked with `--eh-frame-hdr`, which builds the
`PT_GNU_EH_FRAME` segment it searches. `zig c++` and rustc pass it; a raw
`rust-lld` invocation -- fastpy's `_link_slateos`, which the C fixtures use --
does not. `services/ctest-cxx-throw/build.py` passes it itself.

**Tests:** `posix/src/dlfcn.rs`'s replay glibc 2.39's static answers
(`posix/tools/oracle/dlfcn_harness.py`) and hold `dl_iterate_phdr`, `dladdr`
and `_dl_find_object` to them over a synthetic ELF image; `tls.rs`'s check
the header reading those share. `services/ctest-cxx-throw` throws and catches
on the target -- an `int`, a `std::runtime_error` through fifty-one frames
with every destructor run, a rethrow, an `exception_ptr`, a derived class
caught as its base -- and exits 42; it is built and staged, and runs once
lane A adds its rung.

**Where:** `posix/src/dlfcn.rs`; `posix/src/tls.rs` (`ProgramHeaders`, the
program's own headers, which the TLS set-up and `<dlfcn.h>` both read);
`posix/include/dlfcn.h`; `services/ctest-cxx-throw/`.
