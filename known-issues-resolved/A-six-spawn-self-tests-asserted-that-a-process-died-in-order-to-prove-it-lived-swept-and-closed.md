### [A] Six spawn self-tests asserted that a process DIED in order to prove it LIVED -- swept and closed -- 2026-09-21
**Status:** FIXED (all six). Recorded for the shape, and to stop the next person re-running my scan and reading 16 as 16 defects.

**In short:** a group of kernel self-tests checked only that a test program
had finished, and concluded from that it had done its job. But a program
that crashes has also finished. So each of these tests printed a confident
success line for the one failure it was written to catch.

| test | claimed | why `Zombie` could not show it |
|---|---|---|
| Test 4 faulting process | a null write faulted and the kernel survived | if the fault never fired, the program reaches `SYS_EXIT(0)` -- also a zombie |
| Test 5 stack growth | growth past the initial allocation worked | if it failed, an unresolvable `#PF` kills it -- also a zombie |
| Test 6 exec | the image was replaced | a crashed caller is a zombie. **This one was live**: every exec was failing with -101 and the test was green |
| Test 6b exec-failure control | the failure probe fired | asserted termination, never that the probe logged |
| Test 8 SEH exit | the handler ran | its own doc says *"Without SEH, the page fault would kill the process"* -- four lines above *"becomes a zombie -- confirming the handler ran"* |
| Test 8b SEH resume | execution resumed past `ud2` | dying on the `ud2` is what happens if SEH does nothing |

**The class is generational, which is the useful part.** It is not scattered
at random: it is exactly the original numbered `Test 4..8` core spawn tests.
Everything written later -- `self_test_fastpy_slateos_forkexec` (which
discriminates exit codes 100/102/110/111 with a distinct message each),
`self_test_linux_execveat`, the tcc and make_cc harnesses, the minishell
suite -- already checks exit codes. So the convention improved and the first
generation was never revisited. That is worth knowing because it predicts
where else to look: the oldest tests in any file, not a uniform sample.

**The scan that found it has a high false-positive rate, and the number
moves with its window.** Do not re-run it and act on the count:

| window around the `Zombie` assertion | flagged |
|---|---|
| +-12 lines | 23 |
| +-20 lines | 17 |
| +-30 lines | 16 |

Of ~16 flagged, only **6** were real. The rest check the exit code somewhere
the regex cannot follow: `self_test_linux_execveat` tests
`exit_nf != Some(EXEC_FAIL)` thirteen lines down; the minishell suite passes
its code to a `diag(ec1)` helper, so the string `exit_code` never appears
near the assertion at all. A grep for `exit_code` cannot see a check made
through a variable or a function, and I twice concluded a test was broken
before reading it.

**The rule that would have prevented all six:** a test must assert an
outcome that its failure mode cannot also produce. "The process ended" is
almost never that, because ending is what both success and every crash have
in common. Where the expected value is known, assert it exactly; where it
is not -- Test 4's kill code is set by no constant this tree names -- assert
the negation of success rather than inventing a value.
