## B-FOUR-COMPONENTS-DISAGREED-ON-THE-NAME-OF-THE-SYSTEM (lane B, 2026-08-22) — FIXED 2026-08-22

**In short:** Four different parts of this OS each kept their own private
answer to "what is this system called", and all four answers were different:
`Linux`, `CustomOS`, `MintOS` and `Slate OS`. Which one a program saw depended
on which one it happened to ask. On top of that, the `uname` command — the
standard way to ask — was missing two options that POSIX requires, so
`uname -n` (the portable way to ask for the machine's name) answered
`unknown option: -n` instead of answering; and the C library's `uname()`
reported a system version so low that a program built against glibc (the
standard C library that almost all ported Linux software uses) would have
refused to start at all.

**The four answers.** Each was a string literal compiled into a different
component, with nothing connecting them:

| Component | Said | Where |
|---|---|---|
| the kernel's `uname(2)` syscall, and `/proc/sys/kernel/ostype` | `Linux` | `kernel/src/syscall/linux.rs:45483`, `kernel/src/fs/procfs.rs:14356` |
| the C library's `uname()` | `CustomOS` | `posix/src/utsname.rs:73` |
| `/sys/kernel/ostype` | `MintOS` | `kernel/src/fs/sysfs.rs:396` |
| the `uname` command | `Slate OS` | `userspace/coreutils/src/bin/uname.rs` |

**`Linux` is the right answer, and it was already decided.** Not by me, and
not this week: `roadmap-detailed.md` §72 "Version-surface policy" records it as
user-directed on 2026-06-10, and `sys_uname` states the reasoning in the code:

> sysname / release are Linux-ABI-only surfaces: in our architecture native
> code uses native APIs, so the ONLY callers of uname(2) are Linux binaries
> that expect Linux values. Reporting "Linux" / "6.6.x" is therefore the
> faithful answer for this ABI, not a lie about what we are — it tells a Linux
> program exactly which Linux personality it is talking to.

So this was never an open question. The kernel had the settled answer and the
other three components had each independently invented one. (For the record: I
did initially take this for an operator question and was about to file it as
one. Reading `roadmap-detailed.md` and the kernel's own source first is what
stopped that — the answer was written down, which is exactly the case CLAUDE.md
means by "don't guess at requirements when the answer is written down.")

### What was wrong with the `uname` command

`userspace/coreutils/src/bin/uname.rs` was 205 lines against a 445-line
standalone twin in `userspace/uname/` — the size-gap signal that has now
found ten bugs for ten looks.

| Defect | Effect |
|---|---|
| `-n` absent | `uname -n`, POSIX-required, printed `unknown option: -n` and exited non-zero. This is *the* portable way to get the machine's name; scripts use it constantly. |
| `-v` absent | POSIX-required. Same failure. |
| `-p`, `-i`, `-o` absent | GNU options that portable scripts probe for. Same failure. |
| `--help`, `--version` absent | Every other utility has them. |
| long options absent entirely | `--kernel-name` etc. unrecognised. |
| `SYSNAME = "Slate OS"` | Contains a **space**. `uname -a` output is routinely split on whitespace; one field that is two words silently shifts every field after it, so a script reading field 5 as the machine type got the wrong one. |
| the values were compiled in | Four-way disagreement above; and no way for the kernel's answer ever to reach the command. |

### What was wrong with the C library's `uname()`

Worse, and independent of the command:

| Defect | Effect |
|---|---|
| `release` was `0.1.0` | glibc's start-up code (`__libc_start_main`) parses this and aborts the process with **"FATAL: kernel too old"** if the leading version is below its build-time minimum — typically 3.2. `0.1.0` is below every minimum ever shipped. Any glibc-linked program reaching this code died before `main`. Note the kernel deliberately reports `6.6.0-slateos` *for this exact reason*, with the reason written in a comment; the C library then reported something else. |
| `sysname` was `CustomOS` | See above. |
| `struct utsname` had **five** fields, 325 bytes | The real struct has six — `domainname` — and is 390 bytes, both in glibc and in our own kernel, whose `sys_uname` builds `[u8; 6 * 65]` and copies **all 390 bytes** to the caller. A caller that declared this struct and invoked `uname(2)` was handed 65 bytes past the end of its buffer. That is a memory-safety bug, not a cosmetic one. |
| the values were compiled in | The `nodename` field did track `sethostname()`, but only the process-local copy — see `B-POSIX-HOSTNAME-IS-PROCESS-LOCAL`. |

**The tree already contained the correct layout.**
`posix/src/linux_utsname_types.rs` has declared `UTSNAME_SIZE = 390`,
`UTSNAME_FIELD_COUNT = 6` and `UTSNAME_OFF_DOMAINNAME = 325` since it was
written, and `posix/src/linux_uts_user_types.rs` says the same and even names
`PROC_SYS_KERNEL_DOMAINNAME`. The constants were right and the actual `struct`
was wrong, and **nothing tied the two together**, so neither side could notice
the other. Meanwhile `linux_utsname.rs`'s own test asserted `325` — a test
that locked the bug in place rather than catching it.

### The fix

Neither the command nor the library keeps a name of its own any more. Both read
the kernel's `/proc/sys/kernel/{ostype,osrelease,version,hostname,domainname}`,
which `procfs.rs` generates with the comment "the uname(2) surface — must stay
byte-consistent with sys_uname". The compiled-in constants that remain are
**fallbacks for a `/proc`-less boot, set byte-identical to the kernel's own
values**, so even the degraded path cannot introduce a fifth answer.

- `userspace/coreutils/src/bin/uname.rs`: 205 → ~530 lines, 15 → 22 tests. All
  eight POSIX/GNU fields, short bundling (`-mrs`), long options, `--help`,
  `--version`, fixed print order independent of option order, `-a`'s
  omit-unknown-`-p`/`-i` rule (tested to match GNU's "was `-a` given" test
  rather than "was `-p` given"), byte-native argv via `args_os` — which also
  removes it from the `env::args()` panic sweep.
- `posix/src/utsname.rs`: the sixth field added, all values read from the
  kernel, `sysname` `Linux`, `release` `6.6.0-slateos`. 13 → 51 tests.
- `posix/src/linux_utsname.rs`: the stale `325` assertion corrected to 390 and
  expressed as `6 * (__NEW_UTS_LEN + 1)` so it cannot drift again.
- A new test binds the struct to the layout constants — size, field count and
  every field's offset — so the two can never again disagree silently.
- A `const _: () = assert!(size_of::<Utsname>() == UTSNAME_FIELDS * UTSNAME_LEN)`
  makes the ABI size a **compile** error to break. That matters specifically
  here: `cargo test` runs on the Windows build host, and the size of this
  struct is a contract with a kernel that host never runs, so a unit test alone
  would not protect it on the target we ship.

**Still outstanding, and not lane B's to fix:** `/sys/kernel/ostype` still says
`MintOS` (`kernel/src/fs/sysfs.rs:396`), contradicting procfs's `Linux` two
directories away — and `/sys/kernel/osrelease` says `0.1.0-dev`, which is the
value with teeth, since it is the one that fails glibc's start-up gate. Filed
as `requests/b-a-sysfs-says-mintos-while-procfs-says-linux.md`.

**Ten for ten.** Every shipped `coreutils` binary examined against a larger
standalone twin has had a silent-wrong-behaviour bug: `chmod`, `dd`, `tee`,
`tar`, `chown`, `stat`, `kill`, `env`, `hostname`, `uname`. The pattern holds
without exception — the standalone twin is bigger because it was written
against the real problem, and the shipped one is smaller because it was written
against a summary of it.

**Why it survived:** partly the usual reason (the host build compiles no
`cfg(target_os)` body and `cargo test` never runs the binaries), but this one
adds a distinct cause worth naming: **four components each had passing tests,
and each test asserted its own component's private answer.** `utsname.rs`
asserted `CustomOS`; `linux_utsname.rs` asserted `325`; the command's tests
asserted `Slate OS`. Every one of them was green. A test that checks a value
against itself confirms only that the value has not changed — it cannot notice
that three other components hold a different value. The cross-checking test
added above is the direct answer to that: it asserts one component's type
against *another* component's constants, which is the only kind of assertion
that could have caught this.
