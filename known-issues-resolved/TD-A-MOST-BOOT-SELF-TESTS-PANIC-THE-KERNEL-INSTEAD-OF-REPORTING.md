## `TD-A-MOST-BOOT-SELF-TESTS-PANIC-THE-KERNEL-INSTEAD-OF-REPORTING` — resolved 2026-09-07, found 2026-08-22 (lane A)

**In short:** The kernel runs its own test suite on every single boot, and most
of those tests are written so that a failure **halts the machine** rather than
printing "this test failed" and carrying on. On a developer's boot test that is
fine and arguably better — you get an exact file and line. On a user's computer
it means that one wrong flag byte in, say, the terminal layer turns into a
machine that will not start. Nothing is broken today; this is about what happens
the first time one of these checks is wrong on real hardware.

### Scale

`567` files under `kernel/src/` contain both a `self_test` function and
`assert!`/`assert_eq!`/`assert_ne!`, totalling **12 674** assertion sites. The
largest concentrations:

| File | assert sites |
|---|---|
| `kernel/src/syscall/linux.rs` | 400 |
| `kernel/src/container.rs` | 334 |
| `kernel/src/oci.rs` | 217 |
| `kernel/src/net/dashboard.rs` | 151 |
| `kernel/src/net/httpd.rs` | 140 |
| `kernel/src/ipc/namespace.rs` | 116 |

Against roughly 299 sites that use the `KernelResult<()>` + `serial_println!`
FAIL form instead. So the panicking style is the *majority* convention, not the
exception, and the two have coexisted for a long time.

### Why this is being written down rather than fixed

Three separate questions are tangled here, and only the first is a bug:

1. **Should self-tests run on a production boot at all?** They currently run
   unconditionally from `kernel_main` — `tty::self_test()` at `main.rs:5972` is
   typical. If the answer is "no, gate them behind a boot flag", then the
   assertion style stops mattering for users and the whole issue shrinks to a
   one-line change. This is the question to settle first.
2. **Is `assert!` the wrong tool for a check that does run in production?** Yes,
   for the same reason `unwrap` is (`CLAUDE.md`: no panics in non-test code) —
   but only if (1) says they run.
3. **Is it worth converting 12 674 sites?** Almost certainly not as a bulk
   mechanical edit, and definitely not before (1) is answered. Converting them
   also *loses* something: an assertion carries file, line and the compared
   values for free, while the `KernelResult` form only reports what its author
   remembered to log.

Because (1) is a user-visible policy question with a real tradeoff — a
production boot that self-tests is slower but catches a corrupt build, one that
does not is faster but ships unverified — it is the operator's call rather than
mine.

### What was done instead

New self-test code written under `A-KERNEL-UNIT-TESTS-NEVER-RUN` uses the
`KernelResult<()>` form (`pathutil`, `net::raw`, `net::frag`), which is the safe
side of the question whichever way it is decided. Existing assertion-based
self-tests were left alone.

### Provenance

Noticed while converting `tty/mod.rs`, whose `self_test()` is a good example:
74 assert sites, called unconditionally at boot, covering things as ordinary as
whether `VERASE` is 127.
