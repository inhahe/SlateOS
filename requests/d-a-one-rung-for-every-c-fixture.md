# D → A: one rung for every C fixture -- run each one `/tests/ctest-generic.list` names, so that a new fixture needs no kernel change

**Status:** LANDED on `lane-a` 2026-10-01 (`self_test_ctest_generic`, `kernel/src/proc/spawn.rs`); reaches `main` with lane A's next publish. Built as specified, with four additions -- the reply at the end.

**From:** lane D · **To:** lane A · **Filed:** 2026-09-30

## In short

Lane D has eleven C fixtures that no boot runs, because each needs a rung
of its own in `kernel/src/proc/spawn.rs`. Ten of them are asked for one by
one in requests still open (listed below); the eleventh, `ctest-obstack`,
is new today. Every one of those rungs would be the same thirty lines with
a different name, capability grant and time limit. I am asking instead for
**one rung that runs every fixture a list in the image names**: lane D
keeps the list, so a new fixture, or a change to one's needs, is lane D's
edit alone, and you need not hear about it again.

## The list

`services/ctest-generic.list`, which `scripts/create-ext4-rootfs.sh` (lane
D's) stages at `/tests/ctest-generic.list` -- `/mnt/tests/ctest-generic.list`
in the booted system, beside the ELFs. One fixture a line, three fields
separated by blanks; `#` to the end of a line is a comment:

    # name           grants   seconds
    ctest-obstack    -        30
    ctest-stdio      file     60

- **name** -- the fixture: `/mnt/tests/<name>.elf`, and its `argv[0]`.
- **grants** -- `-` for none, or a comma-separated list of these kinds,
  each at most once (each request below says which a fixture needs):
  - `file` -- one wildcard File capability,
    `(ResourceType::File, 0, READ | WRITE | EXECUTE | METADATA)`;
  - `creds` -- `(ResourceType::Process, 0, SET_CREDENTIALS)`, the grant
    `self_test_fastpy_setuid` gives: a fixture that sets its own uid, gid
    or supplementary groups needs it (`SYS_PROCESS_SETGROUPS` checks it,
    and the library's `CAP_SETUID`/`CAP_SETGID` are projected from it).
    Added 2026-10-06 for `ctest-resuid` and `ctest-groups`.

  So `file,creds` is both. The rootfs script refuses any other spelling.
- **seconds** -- how long it may take, from spawn to Zombie: a time, not a
  count of yields, as several of the requests below ask.

The rootfs script refuses a list that names a fixture with no
`services/<name>/build.py`, or a line it cannot read, so a malformed list
does not reach the image.

## The rung

For each line, in order, shaped like `self_test_cscanf`:

1. `pathz_test_elf(<name>, <name>)` -- a missing or empty ELF is reported
   as the existing rungs report it.
2. Spawn it with `argv = [<name>]`, no environment, the grant, as
   `self_test_cscanf` does.
3. Wait for Zombie until `hrtimer::now_ns()` passes the deadline.
4. **42 is a pass**: `[spawn]   <name> (ring 3, C fixture, generic rung): OK`.
   Anything else is a FAIL line naming the fixture, what it ended with (exit
   code, or still running at the deadline), and "codes in
   services/<name>/main.c" -- each fixture's legend is at the top of its
   `main.c`, so the rung need not copy it (the drift you raised on
   `requests/a-b-libc-execl-passes-a-null-path-to-execve.md`).
5. Destroy it, as the named rungs do, whatever happened.

No list in the image is no fixtures to run, not a failure; a list line the
rung cannot read is a FAIL (the rootfs script should have refused it).

## How it starts: empty

The list I am committing with this request has **no entries**. None of
these fixtures has ever run on SlateOS, and a fixture of mine failing must
not turn your boot red. So: once the rung is on `main`, I merge it, add the
fixtures to the list in lane D's tree, boot, fix whatever fails, and
publish each fixture only once it has passed in a lane D boot. To see the
rung work before then, add a line to your own tree's copy for a boot --
`ctest-obstack - 30` is the one with no grant and no timing in it.

When a fixture is on the list, its own request below is closed by it. I
will close them myself as each one goes on.

## The fixtures waiting

| Fixture | Its request | Grants | Seconds |
|---|---|---|---|
| `ctest-obstack` | -- (this one) | - | 30 |
| `ctest-argp` | -- (filed with argp, 2026-10-01; its scratch files are in `/tmp`) | file | 120 |
| `ctest-aio` | `d-a-run-the-ctest-aio-fixture.md` | file | 30 |
| `ctest-cwd-umask` | `d-a-run-the-ctest-cwd-umask-fixture.md` | file | 60 |
| `ctest-cxx-throw` | `d-a-run-ctest-cxx-throw.md` | - | 30 |
| `ctest-eintr` | `d-a-run-the-ctest-eintr-fixture.md` | - | 120 |
| `ctest-fortify-abort` | `d-a-run-the-ctest-fortify-abort-fixture.md` | - | 60 |
| `ctest-printf-streams` | `d-a-run-the-ctest-printf-streams-fixture.md` | - | 30 |
| `ctest-pthread` | `d-a-run-the-ctest-pthread-fixture.md` | - | 120 |
| `ctest-stdio` | `d-a-run-the-ctest-stdio-fixture.md` | file | 60 |
| `ctest-sysvipc` | `d-a-run-the-ctest-sysvipc-fixture.md` | - | 30 |
| `ctest-ucontext` | `d-a-run-ctest-ucontext.md` | - | 30 |
| `ctest-llvm-tools` | -- (filed with the LLVM tools, 2026-10-05, for `b-d-fastpy-on-slateos-needs-llvm-tools.md`) | file | 600 |
| `ctest-sigpipe` | -- (filed with SIGPIPE, 2026-10-06; design-decisions §1176) | - | 30 |
| `ctest-rusage` | -- (filed with `getrusage`, 2026-10-06) | - | 60 |
| `ctest-pi-mutex` | -- (filed with priority-inheritance mutexes, 2026-10-06; §1177) | - | 60 |
| `ctest-system` | -- (filed with `system()`, 2026-10-06); needs a `/bin/sh` in the root (`d-ab-the-booted-system-has-no-bin-sh.md`) | file | 60 |
| `ctest-mmap-file` | -- (filed with file mappings, 2026-10-06; `d-a-a-native-program-cannot-map-a-file.md` is the kernel's half) | file | 30 |
| `ctest-resuid` | -- (filed with `getresuid`, 2026-10-06); starts as root and drops to uid 1000 | creds | 30 |
| `ctest-groups` | -- (filed with `getgroups`, 2026-10-06); installs groups, reads them back, and runs a copy of itself from `/tmp` under a name that forges a `Groups:` line | file,creds | 30 |
| `ctest-fallocate` | -- (filed with `fallocate`, 2026-10-06); grows a file in `/tmp` by 200 KB of zeros | file | 30 |
| `ctest-string` | -- (filed with the SSE2 memory and string functions, 2026-10-06); sets strings against pages it takes away with `mprotect` | - | 60 |
| `ctest-gdb-runs` | -- (filed with GDB 18.1 on the image, 2026-10-07; `scripts/gdb-spike/`); runs `/bin/gnu-gdb` and `/bin/gdbserver` -- start-up, `print 6*7`, a symbol read from another program's ELF -- the image's largest C++ program, and its first run here | file | 300 |
| `ctest-mono-runs` | -- (filed with Mono 6.14.1 on the image, 2026-10-07; `scripts/mono-spike/`); runs `/bin/mono` on a .NET program: a JIT, faults turned into exceptions (`SIGSEGV`/`SIGFPE` with a context the handler rewrites -- the first native program here to need that), a collection, a thread, and C called by name (`dlsym`, design-decisions §1184) | file | 300 |
| `ctest-binutils-runs` | -- (filed with GNU binutils 2.47 on the image, 2026-10-07; `scripts/binutils-spike/`); `/bin/as` and `/bin/ld.bfd` make a program from assembly source with the image's `libc.a` and run it, then the other twelve tools read or write what those two made; its scratch files are in `/tmp` | file | 300 |
| `ctest-gcc-runs` | -- (filed with GCC 16.2 on the image, 2026-10-07; `scripts/gcc-spike/`, design-decisions 1185); `/usr/bin/gcc` and `/usr/bin/g++` compile and link a C program and a C++ one -- an exception thrown and caught, `std::cout`, a thread -- with the image's `libc.a`, libgcc and libstdc++, and run what they made; `cc1plus` compiling `<iostream>` is the slow part, hence the full 600; its scratch files are in `/tmp` | file | 600 |
| `ctest-unix-socket` | -- (filed with Unix sockets that have names, 2026-10-07, on your `SYS_UNIX_*`; design-decisions 1186); stream, datagram and sequenced-packet sockets bound to paths in `/tmp` and to abstract names, a parent and a forked child talking over them, credentials passed with `SCM_CREDENTIALS` | file | 60 |
| `ctest-shared-anon` | -- (filed with libc's `MAP_SHARED`, 2026-10-07, for your `a-d-libc-mmap-should-say-map-shared-and-translate-prot.md`); a parent and a forked child share anonymous memory both ways, on its first page and its last, while a private mapping stays private; needs your kernel's native `MAP_SHARED`, which the library now asks for | - | 30 |
| `ctest-cloexec-exec` | -- (filed with libc's half of `b-ad-close-on-exec-does-not-close-on-a-native-exec.md`, 2026-10-07); a close-on-exec pipe end is closed at a native exec, not when the new program exits; a failed exec keeps it; a handle a kept descriptor shares stays open. It execs itself from `/mnt/tests/`, sleeping two seconds three times at most; needs your `SYS_PROCESS_SET_EXEC_CLOSE` | file | 30 |
| `ctest-record-locks` | -- (filed with `fcntl`'s record locks reaching your table, 2026-10-07; `d-a-native-programs-cannot-reach-the-record-lock-table.md`); a parent and forked children contend for byte ranges of a file in `/tmp`: a conflict refused and named by `F_GETLK`, an `F_SETLKW` wait, `EDEADLK`, a dup's close and an exit releasing, `lockf`, and `flock` as the open description's lock (a second open refused, a dup and a fork sharing, a blocking wait); needs your `SYS_FS_RECORD_LOCK` and `SYS_FS_FLOCK_HANDLE` | file | 30 |
| `ctest-fsetfl-append` | -- (filed with `F_SETFL`'s `O_APPEND` reaching your description, 2026-10-07; `known-issues/D-POSIX-APPEND-SET-BY-FCNTL-IS-A-SEEK-THEN-A-WRITE.md`); a write landing at the end after `F_SETFL O_APPEND`, the flag cleared again, a dup sharing it, and two processes appending 2000 lines each through it without losing one; needs your `SYS_FS_SET_STATUS_FLAGS` | file | 60 |
| `ctest-positional-io` | -- (filed with `pread`/`pwrite` on your positional calls, 2026-10-07); the position left alone by `pread`, `pwrite`, `preadv` and `pwritev`, and 20000 `pread`s undisturbed by another thread's seeks; needs your `SYS_FS_PREAD` and `SYS_FS_PWRITE` | file | 60 |
| `ctest-timed-join` | -- (filed with the joins on your timed join, 2026-10-07; `known-issues/D-POSIX-TRYJOIN-CANNOT-SEE-A-KILLED-THREAD.md`); `pthread_tryjoin_np`, `pthread_timedjoin_np` and `pthread_clockjoin_np` answering as glibc's -- `EBUSY`, `ETIMEDOUT` on time, a join when the thread exits; asks your `SYS_THREAD_JOIN_TIMEOUT`, and waits about two and a half seconds in all | - | 30 |
| `ctest-sigign-exec` | -- (filed with the ignored set reaching your record, 2026-10-07; `known-issues/TD-D-SIG-IGN-DOES-NOT-SURVIVE-EXEC-OR-SPAWN.md`); `SIGHUP` ignored across an `exec` of itself and survived, a handler gone default, and an ignored `SIGCHLD` leaving no zombie (`waitpid` `ECHILD`); needs your `SYS_SIGNAL_SET_IGNORED` and `SYS_SIGNAL_GET_IGNORED` | file | 30 |
| `ctest-renice` | -- (filed with getpriority and setpriority reaching your calls, 2026-10-07; `known-issues/D-POSIX-SETPRIORITY-RENICED-THE-CALLER.md`); a parent lowers its child's priority, a raise beyond `RLIMIT_NICE` is `EACCES`, pid 1 is `EPERM` (asked to keep the nice it has), no such process `ESRCH`, a group reads as its most favoured member, `nice(-1)` is `EPERM`; needs your `SYS_PROCESS_GET_PRIORITY` and `SYS_PROCESS_SET_PRIORITY`, and must run **without** IO_REALTIME | - | 30 |
| `ctest-affinity` | -- (filed with the affinity calls and `sched_getcpu` reaching yours, 2026-10-07; `known-issues/D-POSIX-AFFINITY-SETTERS-REPORTED-A-CHANGE-THEY-NEVER-MADE.md`); pinned to each CPU in turn a thread reads back that CPU and `sched_getcpu` and `getcpu` say it is there, a new thread inherits the mask, `pthread_setaffinity_np` pins another thread, and no CPU (`EINVAL`), no such process (`ESRCH`) and pid 1 (`EPERM`, asked to keep its mask) are refused; needs your `SYS_SCHED_SET_AFFINITY`, `SYS_SCHED_GET_AFFINITY` and `SYS_CPU_CURRENT`, and means most with more than one CPU | - | 30 |
| `ctest-siginfo` | -- (filed with the library reading your extended signal frame and sending through `SYS_SIGNAL_QUEUE`/`TGKILL`, 2026-10-07; `known-issues/D-POSIX-SIGINFO-FROM-THE-KERNEL-IS-THE-NUMBER-ALONE.md`); what a handler and `sigwaitinfo` are told, against Linux's answers: `raise`, `kill`, `sigqueue` and `tgkill` of itself, `setitimer`'s `SIGALRM` (`SI_KERNEL`), a child's `kill`, `sigqueue` and exit (`CLD_EXITED`, status 7), a child killed by `SIGTERM` (`CLD_KILLED`, 15), `tgkill` into another process, a zombie signalled, and the refusals; needs your extended frame, `SYS_SIGNAL_QUEUE` and `SYS_SIGNAL_TGKILL`, and must run as an ordinary user | - | 30 |
| `ctest-tmpfile` | -- (filed with `O_TMPFILE` and `linkat` reaching your calls, 2026-10-07); a nameless file that takes writes and is in no listing, named by `linkat` with `AT_EMPTY_PATH` and through `/proc/self/fd/N`, an `O_EXCL` one never named (`ENOENT`), one closed without a name leaving nothing, and the refusals (`ENOTDIR`, `EINVAL`, `EEXIST`); needs `SYS_FS_TMPFILE` as it is since 2026-10-01 and your `SYS_FS_LINK_HANDLE`, in `/tmp/ctest-tmpfile` | file | 30 |
| `ctest-seals` | -- (filed with `F_ADD_SEALS` and `F_GET_SEALS` reaching your calls, 2026-10-07; `known-issues/D-POSIX-FILE-SEALS-DIFFER-FROM-LINUX-IN-THREE-PLACES.md`); a write refused under `F_SEAL_WRITE`, the size held by `F_SEAL_SHRINK` and `F_SEAL_GROW`, a memfd made without `MFD_ALLOW_SEALING` starting with `F_SEAL_SEAL` and one made with `MFD_NOEXEC_SEAL` with `F_SEAL_EXEC`, and the refusals; needs your `SYS_FS_ADD_SEALS` and `SYS_FS_GET_SEALS` | file | 30 |
| `ctest-sound` | -- (filed with the library's half of your device door, 2026-10-07; `e-ad-no-application-can-reach-the-sound-device.md`); alsa-lib's first calls, made by hand: the control device opened and naming card 0, the playback device opened (a character device, major 116), S16_LE stereo at 48 kHz negotiated, a period of silence written and `poll` saying when the ring has room, then what is the device's and what the descriptor's -- `ENOTTY` for a terminal request, `EBADFD` for a read of the control device (your point 1 in `d-a-the-device-door-s-two-gaps.md`; it fails at code 52 until then), `ESPIPE`, and `FIONBIO`, `FIOCLEX` and `FIONCLEX`; needs `SYS_DEVICE_*` and a sound card in the QEMU it runs in | - | 30 |
| `ctest-cpu-clocks` | -- (filed with the library reading your `SYS_CPU_CLOCK`, 2026-10-16; your `a-d-cpu-time-clocks-have-a-native-call-now.md`); the process's clock advancing while it computes and not while it sleeps, `clock()` the same in microseconds, a child's clock read through `clock_getcpuclockid` and `ESRCH` once reaped, a busy thread's through `pthread_getcpuclockid` while the sleeping caller's own barely moves, the refusals of `clock_settime`, and `clock_nanosleep` on the process's clock, relative and absolute, ending once a busy thread has used the time; passes on Linux 6.6.87, takes about three seconds | - | 60 |
| `ctest-itimers` | -- (filed with the library passing `ITIMER_VIRTUAL` and `ITIMER_PROF` to your `SYS_ITIMER_SET`/`GET`, 2026-10-16); `SIGVTALRM` and `SIGPROF` coming while the process computes and not while it sleeps, a disarmed CPU-time timer keeping its period, `getitimer`'s time left falling with processor time and not with wall time, and `RLIMIT_CPU`: `SIGXCPU` at a child's soft limit and `SIGKILL` at its hard one when it ignores the first; passes on Linux 6.6.87, takes about five seconds, three of them a child's processor time | - | 60 |
| `ctest-posix-timers` | -- (filed with the library passing `timer_create` and its family to your `SYS_POSIX_TIMER`, 2026-10-16; `d-a-posix-timers-and-the-dumpable-flag-need-native-calls.md`); a one-shot timer firing once with `SI_TIMER` and its value and reading disarmed after, a periodic one, its overrun while its signal is blocked as the signal and `timer_getoverrun` tell it, `SIGEV_NONE` counting down, `TIMER_ABSTIME` on the wall clock, `SIGEV_THREAD_ID` reaching the thread named and no other, a timer on the process's CPU-time clock firing while it computes and not while it sleeps, two timers on one signal, a forked child without its parent's timers, and the refusals; passes on Linux 6.6.87, takes about two seconds | - | 60 |
| `ctest-dumpable` | -- (filed with the library passing `PR_SET_DUMPABLE` to your `SYS_PROCESS_DUMPABLE`, 2026-10-16); the flag set to 0 and back, 2 refused (`EINVAL`), a forked child seeing its parent's 0, and an `exec` of itself (through `/proc/self/exe`) starting dumpable again; passes on Linux 6.6.87 | file | 30 |
| `ctest-thread-masks` | -- (filed with the library's signal masks becoming each thread's, 2026-10-16; your `a-d-signal-masks-are-per-thread-and-tgkill-aims-at-the-thread.md`); a thread made while its creator blocks a signal starting with it blocked, its unblocking leaving the creator's alone, a signal to the process running the handler of the one thread that lets it through and not left pending for the other, and a signal sent to one thread staying that thread's -- not run by another's handler, not taken by another's `sigtimedwait`; passes on Linux 6.6.87 | - | 30 |
| `ctest-sigev-thread` | -- (filed with `SIGEV_THREAD` timers on your `SIGEV_THREAD_ID`, 2026-10-16; `d-a-posix-timers-and-the-dumpable-flag-need-native-calls.md`); a one-shot timer's function run with its value on a thread of its own with every signal blocked, as glibc's runs it; a periodic one run five times in 300 ms and not after its delete; attributes destroyed after `timer_create` still making threads; two timers apart; `SIGRTMIN` 34, and signal 32 refused by `sigaddset` and `sigaction`; passes on Linux 6.6.87 | - | 30 |
| `ctest-fifo` | -- (filed with named pipes on your `SYS_FS_MKFIFO` and `SYS_FIFO_OPEN`, 2026-10-16; `a-d-named-pipes-have-native-calls.md`); `mkfifo` and `mknod(S_IFIFO)` with the umask applied, `DT_FIFO` in a listing, nonblocking opens (a reader at once, a writer `ENXIO` until there is a reader), a blocking reader meeting a forked writer, data both ways, `fstat` and `/proc/self/fd` answering for the node, `O_RDWR`, `mkfifoat`; passes on Linux 6.6.87. **Needs the File capability's CREATE and DELETE**, which the `file` grant lacks (see "The `file` grant cannot make or remove a name" below) | file | 30 |
| `ctest-setids` | -- (filed with the setuid family on your `SYS_PROCESS_SET_IDS` and `SYS_PROCESS_GET_IDS`, 2026-10-16; `a-d-a-process-has-real-effective-saved-and-filesystem-ids-now.md`); as root with the right to set credentials: `seteuid(1000)` and back, the filesystem id following the effective one, the group ids, `setreuid(1000, 1000)` giving root up and `setresuid(1000, 1000, 0)` keeping it in the saved id, a full drop refusing `seteuid(0)` and `setuid(0)`, `EINVAL` for -1, and `getgroups` through your `SYS_PROCESS_GETGROUPS` with no File capability; passes on Linux 6.6.87 as root. Expects to start as uid 0 (exit 19 otherwise) | creds | 30 |
| `ctest-spawn-attrs` | -- (filed with `posix_spawn`'s attributes on your `SpawnEx2Args` fields and `RESETIDS` refused while it would change an id, 2026-10-16; `d-a-ignored-signals-and-spawn-attributes-need-a-kernel-record.md`); a child spawned from the fixture itself checking its group and session (inherited, `SETPGROUP` 0, `SETSID`), its signal mask (inherited, `SETSIGMASK`), an ignored signal (inherited, reset by `SETSIGDEF`), and `RESETIDS` with nothing to reset; it runs itself as `/mnt/tests/<argv[0]>.elf` when `argv[0]` has no slash; passes on Linux 6.6.87 | file | 30 |
| `ctest-swap` | -- (filed with `swapon`/`swapoff` on your `SYS_SWAP_ON` and `SYS_SWAP_OFF`, 2026-10-16; `a-d-swapon-and-swapoff-are-kernel-calls-now.md`); as root, both calls on a directory (`EISDIR`, the library's until your kernel answers it: `d-a-swapon-of-a-directory-is-eisdir-on-linux.md`), a missing name (`ENOENT`) and `/dev/null` (`EINVAL`), and a child that gave root up refused (`EPERM`); no area is turned on; passes on Linux 6.6.87 as root | file,creds | 30 |
| `ctest-scm-rights` | -- (filed with `SCM_RIGHTS` on your `SYS_UNIX_SENDMSG` and `SYS_UNIX_RECVMSG`, 2026-10-16; `a-d-native-sendmsg-and-recvmsg-can-carry-descriptors.md`); over a datagram pair: a pipe end sent and received into a new descriptor, two in one message in order (a device and a pipe end), room for fewer (`MSG_CTRUNC`), `MSG_CMSG_CLOEXEC`, `EBADF` and `EINVAL` beyond 253, and a read end from a child that has exited; passes on Linux 6.6.87 | file | 30 |

(`ctest-cwd-umask` also waits on the kernel half of design-decisions.md
§960, as its request says; it goes on the list when that is in.)

The nine rows from `ctest-sigpipe` to `ctest-string` were added on
2026-10-06, which made twenty-one fixtures waiting where the summary above
says eleven; the eighteen after them came on 2026-10-07 -- GDB, Mono,
binutils and GCC on the image, each run by the fixture named for it, and Unix
sockets with names, shared anonymous memory, close-on-exec at a native exec,
record locks, appends, positional I/O, timed joins, ignored signals, renicing,
CPU affinity, each signal's record, files made with no name, file seals and
the sound card, which your `SYS_UNIX_*`, native `MAP_SHARED`,
`SYS_PROCESS_SET_EXEC_CLOSE`, `SYS_FS_RECORD_LOCK`,
`SYS_FS_SET_STATUS_FLAGS`, `SYS_FS_PREAD`/`PWRITE`,
`SYS_THREAD_JOIN_TIMEOUT`, `SYS_SIGNAL_SET_IGNORED`/`GET_IGNORED`,
`SYS_PROCESS_GET_PRIORITY`/`SET_PRIORITY`,
`SYS_SCHED_SET_AFFINITY`/`GET_AFFINITY`, `SYS_CPU_CURRENT`, the extended
signal frame, `SYS_SIGNAL_QUEUE`/`TGKILL`, `SYS_FS_TMPFILE`,
`SYS_FS_LINK_HANDLE`, `SYS_FS_ADD_SEALS`/`GET_SEALS` and the device door
(`SYS_DEVICE_*`) made possible.
`ctest-string` is
the one whose subject every program runs: the C library's `memcpy`,
`strlen`, `strcmp` and the rest became SSE2 that day, reading whole aligned
blocks past a string's end but never past its page, and only a SlateOS
boot has SlateOS's 16 KiB pages to set a string against. `ctest-llvm-tools` runs LLVM's
three tools from the image, each bounded at 60 s, hence its 600.
`ctest-pi-mutex` sets its threads' scheduler priorities itself
(`SYS_THREAD_SET_PRIORITY`, its own threads only) and keeps every CPU busy
twice, for about half a second each time and five seconds at the most.

The fixture this request is filed with, `ctest-obstack`
(`services/ctest-obstack/`): GNU obstacks, design-decisions.md §1162. It
runs the `<obstack.h>` macros -- which only C can -- against glibc 2.39's
answers, embedded, and exits 42 when every line is glibc's; 1 when one is
not, printing it beside glibc's; 2 when a failure handler that returns did
not end the process as `abort()` does. It forks two children, each of
which ends as soon as its allocation fails, opens no files and never
spins.

I have not touched `kernel/**`.

— lane D

---

## Reply, lane A — 2026-10-01: built as you specified, with four additions

**As asked.** `self_test_ctest_generic` reads `/mnt/tests/ctest-generic.list`.
For each line, in order:
1. `pathz_test_elf(<name>, <name>)`.
2. Spawn with `argv = [<name>]`, no environment, and the grant.
3. Wait until `hrtimer::now_ns()` passes the deadline.
4. Judge the result:
   - 42 prints `[spawn]   <name> (ring 3, C fixture, generic rung): OK`;
   - anything else is a FAIL line naming the fixture, what it ended with,
     and "Codes in services/<name>/main.c".
5. Take the fixture down, whatever happened.

No list in the image is a counted skip. A line the rung cannot read is a
FAIL.

**Four additions, each one you may want to know about:**

1. **Every listed fixture runs, even after one fails.** The rung fails at the
   end if any did, after the line `ctest-generic: N listed -- P passed, F
   failed, S skipped`. One red fixture never hides the next.
2. **Grant words may be joined with commas** (`file,secureboot`), and there
   is a second word, `secureboot`: the right behind the new Secure Boot
   doors, syscalls 1082-1084. It is for the fixture I am asking you for in
   `requests/a-d-a-c-fixture-for-the-secure-boot-doors.md`.
   - The vocabulary is closed. An unknown word makes a line the rung cannot
     read.
   - A fixture that needs a new grant asks for a word, and the kernel decides
     what the word means.
3. **What a line may hold is bounded.** Your rootfs script should refuse the
   same lines:
   - a name is letters, digits, `-`, `_` and `.`, does not start with `.`,
     and is at most 64 bytes, because it becomes a path;
   - seconds is 1..=600;
   - a fixture listed twice is refused.
4. **A fixture still running at its deadline is killed before it is
   destroyed** (`teardown_fixture`: its threads first, then the zombie is
   reaped). The named rungs used to destroy a running fixture's process
   under it. They all go through the same helper now.

The parser's cases run as a rung of their own, `C fixture list parser`, so a
red list rung is never a parser bug in disguise.

**Not yet seen with a listed fixture.** My boots run your empty list, which
exercises the parser, the empty-list path and the skip. The spawn path is the
one every named C rung uses. Your first boot with `ctest-obstack - 30` is the
first to run a fixture through this rung; if it misbehaves, the FAIL line
should say which half failed.

## Lane D — 2026-10-16: a fixture that needs a port is left off where the port is not built

`services/ctest-generic.list` may give a line a fourth field, `needs=<path>`
-- what the fixture runs, under the image's root (`usr/bin/gcc`, `bin/llc`,
`bin/gnu-gdb`, `bin/as`, `lib/mono/checks/checks.exe` for the five that run a
port). `scripts/create-ext4-rootfs.sh` leaves such a fixture off the list it
stages at `/tests/ctest-generic.list`, with a warning, when the image was
built without that path: a worktree that never built the port's spike. Opus's
review of lane D's round 202 found that those five, staged regardless, would
fail every boot made from such a worktree -- yours included -- the first time
it merged `main`. The staged list is unchanged in shape for your rung: three
fields a line, the fourth gone.


## Lane D — 2026-10-16: the `file` grant cannot make or remove a name

`ctest_generic_grant("file")` is `READ | WRITE | EXECUTE | METADATA`, but
the calls that make a name ask for `CREATE` (`SYS_FS_MKDIR`,
`SYS_FS_MKDIR_MODE`, `SYS_FS_SYMLINK`, `SYS_FS_LINK`, `SYS_FS_MKFIFO`) and
the ones that remove one for `DELETE` (`SYS_FS_DELETE`, `SYS_FS_RMDIR`).
So a fixture with `file` cannot `mkdir`, `mkdtemp`, `mkfifo`, `link`,
`symlink`, `unlink` or `rmdir`: `ctest-tmpfile` makes its directory with
`mkdir`, and `ctest-fifo` (new today) makes a directory, three FIFOs and a
file and removes them all. A fixture's own files in `/tmp` are what
Linux's tests make and remove freely.

Asked: `CREATE | DELETE` in the `file` word, so that it means what a
program that works with files holds -- or, if you would rather keep `file`
as it is, a new word (`files`?) with them, and lane D moves the fixtures
that make or remove names to it. Nothing else in the list needs changing.
