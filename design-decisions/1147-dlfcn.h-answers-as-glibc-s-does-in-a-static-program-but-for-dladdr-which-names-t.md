## 1147. `<dlfcn.h>` answers as glibc's does in a static program, but for `dladdr`, which names the program, and `RTLD_NOLOAD`, which is not an error

**Date:** 2026-09-29
**Decided by:** Claude (autonomous)
**Lane:** D

**In short:** every program here is linked statically -- nothing is ever
loaded into it -- so the dynamic linker's calls (`dlopen`, `dladdr` and the
rest) describe one object, the program, and this library answers them as
glibc does in a statically linked program, with two exceptions. For an
address inside the program, `dladdr` names the program instead of saying "no
object here": that is what glibc says for a dynamically linked program and
what POSIX describes, static glibc saying nothing only because it keeps no
address range for its program. And `dlopen` of a file with `RTLD_NOLOAD`
("only if it is already loaded") returns NULL with no error message, where
static glibc goes looking for the file and complains when it is missing:
nothing here looks for files.

| Call | Here | glibc 2.39, statically linked | Why |
|---|---|---|---|
| `dladdr(addr)`, `addr` in the program | 1: argv[0], the ELF header, no symbol | 0 | POSIX: the executable is an object `dladdr` describes; glibc's dynamic answer is this one; `backtrace_symbols` and error reporters can then name the program |
| `dlopen(file, RTLD_NOLOAD)` | NULL, no message | NULL, with `file: cannot open shared object file: No such file or directory` for a missing file and no message for one found and not loaded | nothing is searched here; "not loaded" is the answer asked for, and glibc's own for a file it finds |
| `dlopen(file)` | NULL, `file: cannot open shared object file: dynamic loading is not supported` | loads it, or says why not | there is no dynamic loader |
| `dlinfo(RTLD_DI_SERINFOSIZE)` | an empty search path: 16 bytes, no directories | glibc's four default directories | nothing is searched |
| `dl_iterate_phdr` | one call; `dlpi_adds` 1 | two, the vDSO's too; `dlpi_adds` 2 | there is no vDSO |

Everything else -- `dlopen`'s handles and mode checks, the messages,
`RTLD_NEXT`, `dlclose`, `dlinfo`'s requests, `dlmopen`'s one namespace,
`_dl_find_object`'s segment -- is glibc's static answer exactly, replayed by
`posix/src/dlfcn.rs`'s tests from `posix/tools/oracle/dlfcn_harness.py`'s
record of it.

**Where:** `posix/src/dlfcn.rs`.
