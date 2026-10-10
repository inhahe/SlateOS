## 1184. `dlsym` finds what the program exports, and `dlopen` of the C library's own names opens the program

**Date:** 2026-10-07
**Decided by:** Claude (autonomous)
**Lane:** D

**In short:** every program here is one static executable, and the C
library's `dlopen` and `dlsym` have answered as glibc's static ones do.
`dlopen` of any file fails, and `dlsym` never finds anything. Programs that
call C functions by name at run time cannot work that way. Mono's
`DllImport("libc")` and Python's `ctypes.CDLL("libc.so.6")` are the two the
ports reach first. Now a program linked to export its symbols
(`--export-dynamic`, as Mono's runtime is) has them found by `dlsym`, and
named by `dladdr`. Opening one of the C library's own names --
`libc.so.6`, `libm.so.6` and their sisters -- opens the program, which
holds all of them. A program that exports nothing is answered exactly as
before.

**What was measured.** glibc 2.39's static `dlsym` answers NULL even for a
program linked with `-Wl,--export-dynamic`, whose symbol table is in memory.
Dynamic glibc finds the same symbols (`gcc -static` against `-rdynamic`,
2026-10-07). So no glibc answer exists for this case to copy: the static
one cannot see the table, and the dynamic one has a loader behind it.

**The choice:**

| Option | *What changes* | For | Against |
|---|---|---|---|
| **A. Look in the program's own dynamic symbol table** (chosen) | `dlsym` finds an exported symbol; `dladdr` names one; `dlopen("libc.so.6")` is the program | POSIX's `dlsym` of the global handle, and what dynamic glibc does; the table is already in memory; nothing changes for a program that exports nothing | parts from glibc's static answer for exporting programs, which the oracle (`dlfcn_oracle.txt`) does not cover -- its probes link nothing exported |
| B. Keep glibc's static answer | nothing | the oracle holds everywhere | Mono's calls into C fail with `DllNotFoundException`, `ctypes` cannot reach libc |
| C. A table of the C library's functions built into `dlsym` | `dlsym` finds libc's functions in every program, exported or not | works without `--export-dynamic` | a second symbol table of ~2,000 names in every program that links `dlsym`; finds libc's functions but not the program's own, which is what `DllImport("__Internal")` and plugins ask for |

**Why A.** It answers the question these calls exist for -- "what is called
this?" -- from the program's own description of itself. It costs nothing
for programs that do not ask, and a program that wants to be asked opts in
the standard way, at link time. §1147 made the same move for `dladdr`, from
glibc's static non-answer to POSIX's answer, and was the precedent.

**The rules are glibc's,** from its `do_lookup_x`, `check_match` and
`determine_info`:
- a symbol is found if it is defined, global, weak or unique, and of
  default or protected visibility;
- a TLS symbol gives the calling thread's copy;
- an IFUNC gives what its resolver returns;
- versions follow glibc where the program has any. `dlvsym` wants the
  named version, hidden or not. `dlsym` wants an unversioned definition,
  else the one default version, and nothing if two would answer.

The lookup goes through the GNU hash table, else the SysV one. Each was
checked, through a port of the code, to find all 4,730 names of Mono's real
`mono-sgen`, which ld.lld linked.

**`dlopen`'s names** are glibc's sonames for the parts of the C library:
`libc.so.6`, `libm.so.6`, `libdl.so.2`, `libpthread.so.0`, `librt.so.1`,
`libutil.so.1`, `libcrypt.so.1`, `libresolv.so.2` and `libanl.so.1`. Each
is matched alone or as the last part of a path. They are loaded already, so
`RTLD_NOLOAD` finds them too. The handle is the program's, so a lookup
through it finds only what the program exports. `libc.so`, the linker-script
name glibc's `dlopen` also refuses, is not among them.

**Revisit if** a program needs libc's functions by name without being
linked to export them. That would be option C, for those programs alone.

**Where:** `posix/src/dlfcn.rs` -- `Exports`, `lookup`, `addr_info`,
`OWN_NAMES` -- and its tests.
