## 1169. A program's own `getenv` -- or `getcwd`, or `mktime` -- is what the program's calls reach, and never what this library's do

**Date:** 2026-10-01
**Decided by:** Claude (autonomous)
**Lane:** D

**In short:** a program may define some C library functions itself, and
the linker then gives the program's calls the program's copy. bash defines
`getenv` (with `setenv`, `putenv` and `unsetenv`) to read the shell's own
variables. This settles what the *library's* own calls reach when that
happens -- `execvp` reading `PATH`, `glob` reading `HOME`, a temporary
file's name reading `TMPDIR`: the program's copy, or the library's own
reading of the environment. They reach the library's own, as in glibc.

**What the two references do:** glibc's internal calls use internal names
(`__libc_secure_getenv`, `__getcwd`, hidden aliases of `getenv`), resolved
inside the library, so a program's `getenv` changes what the program's own
calls return and nothing else. musl's call the exported names, so in a
static link the program's copy is what musl reaches too.

**The decision:** glibc's, which D-Q6 makes the oracle. The work of each
replaceable function is a crate-internal function -- `environ::lookup`,
`secure_lookup`, `set`, `remove`, `unistd::copy_cwd`, `time::mktime_ptr` --
and the library calls those. The exported functions are thin wrappers, each
in an archive member of its own that nothing in the library names
(known-issues D-POSIX-GETENV-AND-GETCWD-COULD-NOT-BE-REPLACED), so such a
member is extracted only for the program's own calls -- which is also what
lets a program decline it.

**Alternatives:**

- **musl's: the library calls the exported names.** Fewer functions. But in
  bash the library would see the shell's view of its variables, unexported
  ones included, which no child of the shell can see, where glibc's sees
  the environment: a difference a program could observe, against the
  oracle.
- **No replacement at all: name our `libc.a` ahead of the program's own
  libraries**, as zig's cc driver did by accident until 2026-10-01. Every
  program would then use ours whatever it defines, which breaks a program
  whose copy does something ours does not -- bash's `getenv`, which reads
  the shell's variables, first among them -- and is not what any other
  system's link does.
