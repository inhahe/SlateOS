## 1162. GNU obstacks are glibc's: its older interface, its chunk sizes, and an `obstack_printf` that reaches the chunks only by name

**Date:** 2026-09-30
**Decided by:** Claude (autonomous)
**Lane:** D

**In short:** an obstack (GNU's "object stack") is a heap a program keeps
for itself. Objects go one after another into large blocks ("chunks") that
the program's own allocation function supplies; one object at a time can
grow at the end until it is finished; and freeing one object frees
everything made after it. elfutils, among others, uses the C library's.
Almost all of it is macros in `<obstack.h>`, which the program compiles
into itself; the library has only the functions those macros call when a
chunk runs out. This library had none of it. It now has glibc 2.39's: the
same structure field for field, the macros doing what glibc's do, and the
functions asking the program's allocation function for exactly the sizes
glibc's ask for -- which the program can see.

| | glibc 2.39 | gnulib's own | here |
|---|---|---|---|
| lengths, `_obstack_memory_used` | `int` | `size_t` | `int` |
| the chunk size, the allocation function's argument | `long` | `size_t` | `long` |
| `alignment_mask` | `int` | `size_t` | `int` |
| `struct obstack` on x86-64 | 88 bytes | 88 bytes, each field at glibc's offset | glibc's |

**Why glibc's older interface.** glibc has kept the interface obstacks had
in the 1990s, `int` lengths and all, for its binaries' sake. gnulib's
obstack module has a newer one, with `size_t`, and compiles it into the
program whenever the C library's `_obstack_memory_used` returns an `int` --
on glibc, that is, and so here. Programs are therefore of two kinds: those
written against glibc's header, which want glibc's interface, and those
that bring gnulib's and never call the library's functions at all, which
want only that the library's stay out of their way (below). So the
interface is glibc's (D-Q6's rule: glibc 2.39 is the oracle). Its limit is
glibc's too: an object of 2 GiB or more cannot be made.

**As glibc does** (`posix/tools/oracle/obstack_harness.py`: twenty-five
scenarios, each operation's resulting state replayed against the library
by `posix/src/obstack.rs`'s tests). A first chunk of 4064 bytes and an
alignment of 16 when the program asks for neither; each new chunk asked
for at the object's size plus the length wanted, plus an eighth of the
object, plus the alignment mask, plus 100 bytes -- never less than the
chunk size; the chunk an object leaves freed when the object was all it
held, unless it may hold an empty object as well; an object's start
aligned as an address, not as an offset in its chunk; `obstack_free` back
to an object in any chunk, and an abort for an address in none; the
default failure handler printing "memory exhausted" and exiting with
`obstack_exit_failure`, which is 1. `obstack_printf` and `obstack_vprintf`
add their output to the growing object with no NUL after it, as glibc's
do, and so do their fortified `__obstack_printf_chk` and
`__obstack_vprintf_chk`.

**The header's macros.** `posix/include/obstack.h` follows glibc's
installed header macro for macro (D-Q6 lists it): the GNU C forms, which
evaluate each argument once, and the portable forms, which go through the
obstack's `temp` field. Both are held to glibc: the harness builds its own
program against this header and glibc's functions, in each form, and must
print glibc's answers line for line (`obstack_harness.py --header`); and
`services/ctest-obstack` is the same program built for SlateOS, against
this header and this library.

**`obstack_printf` reaches the chunks only by name.** Each new chunk
`obstack_printf` needs is had through `_obstack_newchunk`, called by its C
name, as glibc's is. A program with gnulib's obstacks has its own
`_obstack_newchunk`, and the call is to that; were it to the library's own,
linking would bring the library's obstack functions in beside the
program's, and two definitions of each would refuse to link. For the same
reason printf's core holds no reference to obstacks at all -- its
destination is a function and a context, which `obstack_vprintf` passes --
and `scripts/check-libc-shape.py` makes obstack a strict family: its
functions in one archive member with nothing else, and nothing outside it
referring to anything in it but their names.

**Where not.** A failure handler that returns -- glibc's manual says it
must exit or longjmp -- leaves glibc's `_obstack_newchunk` going on through
the null chunk it could not have, and it faults. This library aborts at
that point instead: the result is the same, the process ends, but it ends
before the macro that asked for room can write past the chunk there is.
`services/ctest-obstack` checks it (its exit 2).

**The alternatives.**

- *gnulib's newer interface, `size_t` throughout*: objects past 2 GiB, but
  not glibc's interface -- a program written for glibc's, printing
  `obstack_object_size` with `%u` as its `unsigned` asks, or declaring
  `_obstack_memory_used` itself, is wrong against it; and gnulib's programs
  gain nothing, since they bring their own.
- *No obstacks*: gnulib's programs still build, with their own copies;
  those that call the C library's, elfutils among them, do not.
- *A returning handler answered by returning*: the obstack left as it was,
  as a longjmp leaves it -- but the macro that called would then write past
  its chunk, silently, where glibc's faults.

**Where:** `posix/src/obstack.rs`; `posix/include/obstack.h`;
`posix/src/printf.rs` (`obstack_printf`, `obstack_vprintf`, printf's
destinations), `posix/src/fortify_printf.rs` (their `__*_chk` forms);
`scripts/check-libc-shape.py`; `services/ctest-obstack/`.
