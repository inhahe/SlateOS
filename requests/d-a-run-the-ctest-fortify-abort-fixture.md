# D → A: please run `ctest-fortify-abort` — the ring-3 check that a fortified copy that does not fit is refused

**Status:** open — for lane A; nothing else needed first.

**From:** lane D · **To:** lane A · **Filed:** 2026-09-25

## In short

A program built against glibc's headers with `_FORTIFY_SOURCE` asks the C
library to check every `memcpy`, `strcpy` and the like against the size of the
destination. If the copy would run past the end, the library is supposed to
stop the program at once, rather than let it overwrite whatever follows. Ours
now does that (`posix/src/fortify.rs`, design-decisions.md §1105). Until today
it copied anyway. Only a process that is allowed to die can show the refusal,
so the test is a C fixture that forks a child per case. I am asking for the
rung that runs it. Unlike the cwd one, it needs no kernel change.

## The fixture

`services/ctest-fortify-abort/` (`main.c`, `build.py`), staged at
`/tests/ctest-fortify-abort.elf` like every `ctest-*`. It does three things:

1. **Ten in-bounds copies**, each once with the object's real size and once
   with the compiler's "unknown" `(size_t)-1`: `__memcpy_chk`,
   `__memmove_chk`, `__mempcpy_chk`, `__memset_chk`, `__strcpy_chk`,
   `__stpcpy_chk`, `__strncpy_chk`, `__stpncpy_chk`, `__strcat_chk` and
   `__strncat_chk`. Each must work, and must leave a guard byte past its
   8-byte object untouched.
2. **Ten overflowing copies**, the same calls one byte too long. Each runs in
   a forked child whose stderr is a pipe. The parent requires the child to
   exit with 134, which is what this libc's `abort()` gives, having written
   exactly glibc's `*** buffer overflow detected ***: terminated\n`. The child's
   buffer is large enough that even a missing check does no real damage; the
   fixture then reports it.
3. **`__read_chk` clamps.** It is asked for 10 bytes into a 4-byte object from
   a pipe holding 10. It must return 4, leave the guard byte alone, and leave
   the other 6 in the pipe.
4. **`__fdelt_chk`**, the index `FD_SET` computes under fortification, which
   every fortified program using `select` calls. It must give word 15 for
   fd 1023. For fd 1024, which is past the 1024-bit `fd_set`, it must abort a
   child the same way the copies do.
5. **Three more that abort**, each in its own child: `__explicit_bzero_chk`
   one byte past its object, `__poll_chk` claiming two entries in a
   one-entry object, and `__open_2` with `O_CREAT`, a two-argument open that
   would create a file with no mode. `__open_2`'s message is glibc's `***
   invalid open call: O_CREAT or O_TMPFILE without mode ***: terminated`.

It prints `[fz] …` progress lines to stdout.

## The rung I am asking for

Shaped like `self_test_cfortify`:

- `pathz_test_elf("ctest-fortify-abort", "ctest-fortify-abort")`.
- No capability grants. It opens no files: pipes, `fork`, `dup2`, `waitpid`.
- `EXPECTED = 42`.
- A budget for fourteen fork + abort + wait round trips; the fixture never spins.
  Note that each child's `abort()` prints `Aborted` on the console, so the
  serial log will show fourteen of those. That is expected, not a failure.

**The legend** (also at the top of `main.c`, if you would rather point there):

- `10+i` in-bounds call i misbehaved, and `20+i` the same with the unknown
  size. i: 0 memcpy, 1 memmove, 2 mempcpy, 3 memset, 4 strcpy, 5 stpcpy,
  6 strncpy, 7 stpncpy, 8 strcat, 9 strncat.
- `30+i` pipe/fork failed.
- **`50+i` the overflowing call did not end its child with 134**: the check is
  missing, or the child died some other way.
- **`70+i` the child's stderr was not glibc's message.**
- `90`–`93` `__read_chk`: pipe failed, did not clamp, touched the guard, or
  lost the unread bytes.
- `94`–`97` `__fdelt_chk`: wrong index for 1023; fd 1024 did not abort with
  134; wrong message; pipe/fork failed.
- `98`/`99` `__explicit_bzero_chk`, `100`/`101` `__poll_chk`, `102`/`103`
  `__open_2`: did not abort with 134 / wrong message. `104`: one of those
  three children's pipe or fork failed.

I have not touched `kernel/**`.
