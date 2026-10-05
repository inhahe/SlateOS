## D-POSIX-CONSTANTS-WERE-NOT-MUSLS — 83 constants a C caller passes or reads back had values of their own, or glibc's (lane D, 2026-09-27) — **Status: FIXED 2026-09-27**

**In short:** a C program here is compiled against musl's headers, so the
numbers it hands the C library -- flags, item numbers, error codes -- are
musl's. 83 of the library's disagreed, and each disagreement was a silent
wrong answer: `nl_langinfo(CODESET)` said "Sun", `getaddrinfo`'s errors
matched none of the names a program tests them against, and every `nftw`
callback misread the kind of every file. design-decisions.md §1119.

| Where | What was wrong | What a program saw |
|---|---|---|
| `langinfo.rs` | all 55 `nl_item`s numbered 0-55 in an order of the module's own; musl's (and glibc's) are `(category << 16) \| index` | `nl_langinfo(CODESET)` (14) answered ABDAY_1's "Sun": CPython's locale encoding, gnulib's `locale_charset` in every GNU port; every other item answered the wrong string or "" |
| `socket.rs` | `EAI_*` were 1 to 11; musl's are -1 to -11 | `rc == EAI_NONAME`, `rc == EAI_AGAIN` never true; `gai_strerror` agreed only with itself |
| `ftw.rs` | `FTW_F` .. `FTW_SLN` were glibc's 0-6; musl's are 1-7 | `typeflag == FTW_F` false for files, true for directories |
| `ioctl.rs` | `TCOOFF`/`TCOON`, `TCIOFF`/`TCION` swapped in pairs | nothing yet: `tcflow` accepts all four and does nothing |
| `unistd.rs` | `_SC_THREAD_KEYS_MAX` 76 and `_SC_THREAD_THREADS_MAX` 74, each the other's | `sysconf` answered the keys' question with the threads' limit and back |
| `linux_pthread_key_types.rs` | `PTHREAD_STACK_MIN` glibc's 16384 (musl 2048), `PTHREAD_KEYS_MAX` glibc's 1024 (the library allows musl's 128) | `pthread_attr_setstacksize(&a, PTHREAD_STACK_MIN + 4096)` was `EINVAL` |
| `sysv_shm.rs`, `locale.rs` | `SHM_NORESERVE` `0o10000000` (musl `0o10000`), `LC_ALL_MASK` 63 (musl `0x7fffffff`) | nothing: neither is read |

**How it was found.** Reviewing a facade module's tests turned up `tcflow`'s
swap; that prompted an audit of every constant of the live modules against
musl's headers -- a probe built with `zig cc --target=x86_64-linux-musl` and
run under WSL, 1,374 names compared. The tests that pinned the wrong values
(`tcflow`'s, `EAI_*`'s, `FTW_*`'s, `LC_ALL_MASK`'s, `PTHREAD_STACK_MIN`'s)
had each been written from the module rather than from a header.

**Worth knowing.** Two of the wrong values had passed a check the day before:
the *_types modules kept on 2026-09-26 (`TD-D-1700-CONSTANT-MODULES-NOTHING-USED`)
were compared with glibc's headers, not musl's, and `PTHREAD_STACK_MIN` and
`PTHREAD_KEYS_MAX` are two of the few numbers where the two differ. The
oracle has to be the header the callers include.

**Kept true since 2026-09-27** by the constants half of
`scripts/check-libc-abi.py` (design-decisions.md §1130), which compares every
public constant whose name a musl header defines with that header's value on
each push that touches `posix/src`.  Its first run found three the audit had
not reached, each fixed in the same change:

| Where | What was wrong | Why the audit missed it |
|---|---|---|
| `linux_perf_event.rs` | `PERF_EVENT_IOC_SET_BPF` was 0x2408, as if it were an `_IO`; the kernel's is `_IOW('$', 8, __u32)`, 0x40042408 | its header, `linux/perf_event.h`, was not among the audit's |
| `stdio.rs` | `TMP_MAX` was glibc's 238328; musl's is 10000.  `tmpnam` still tries glibc's 62 cubed names | it arrived with the stdio rewrite, nine hours after the audit |
| `perthread.rs` | `BLOCK_SIZE`, the per-thread block's size, was public under the name musl's `sys/mount.h` gives 1024 -- not a wrong number but an internal one wearing a C name; now crate-private | its value is computed with `size_of`, which the audit's evaluator could not compute |
