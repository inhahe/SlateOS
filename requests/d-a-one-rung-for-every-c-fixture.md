# D → A: one rung for every C fixture -- run each one `/tests/ctest-generic.list` names, so that a new fixture needs no kernel change

**Status:** open — for lane A; nothing else needed first.

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

(`ctest-cwd-umask` also waits on the kernel half of design-decisions.md
§960, as its request says; it goes on the list when that is in.)

The last six rows were added on 2026-10-06, which makes eighteen fixtures
waiting where the summary above says eleven. `ctest-llvm-tools` runs LLVM's
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
