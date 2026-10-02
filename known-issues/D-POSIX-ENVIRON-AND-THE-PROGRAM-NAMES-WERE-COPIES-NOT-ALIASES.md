## D-POSIX-ENVIRON-AND-THE-PROGRAM-NAMES-WERE-COPIES-NOT-ALIASES — `environ` and `__environ`, `program_invocation_name` and `__progname_full`, `program_invocation_short_name` and `__progname` were separate variables, so a program assigning one name changed nothing the others showed (lane D, 2026-09-29) — **Status: FIXED 2026-09-29 (`posix/src/environ.rs`, `posix/src/crt.rs`)**

**In short:** C gives three of its variables several names each -- the
environment list (`environ`, `__environ`, `_environ`), the program's name as
it was started (`program_invocation_name`, `__progname_full`), and its last
component (`program_invocation_short_name`, `__progname`) -- and in glibc
each group is one variable, so a program can assign through any name and
the C library sees it through all of them. Here each name was a variable of
its own. So a program that set its name for error messages the way gnulib
does (every GNU tool: `set_program_name` assigns `program_invocation_name`)
went on being reported under the old name, and one that replaced its
environment through `__environ` did not replace it at all; `_environ` did not
exist.

| Names | Was | Is |
|---|---|---|
| `environ`, `__environ`, `_environ` | `environ`, and `__environ` a copy written only when `setenv` and its kin changed the list -- not when the program assigned `environ`; no `_environ` | one variable: `__environ`, and `environ` and `_environ` weak aliases of it |
| `program_invocation_name`, `__progname_full` | two variables, both set at start-up and never again | one variable: `__progname_full`, and a weak alias |
| `program_invocation_short_name`, `__progname` | two variables, likewise | one variable: `__progname`, and a weak alias |

Rust cannot give a static two names, so each variable is defined in
assembly with all its labels on one word, strong and weak as glibc has them
(a program that defines one of the weak names itself still links).
`scripts/check-libc-shape.py`'s new CHECK 4 holds `libc.a` to that: each
group's names in one member, at one address, the first strong and the rest
weak. Run on the archive before the fix, it reports all three.

**Where:** `posix/src/environ.rs`, `posix/src/crt.rs`;
`scripts/check-libc-shape.py` (CHECK 4).
