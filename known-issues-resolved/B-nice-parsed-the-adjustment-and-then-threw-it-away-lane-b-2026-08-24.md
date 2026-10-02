## `nice` parsed the adjustment and then threw it away (lane B, 2026-08-24)

**Status: FIXED.** `userspace/coreutils/src/bin/nice.rs` is a port of GNU
coreutils 9.4's `src/nice.c`, verified case-by-case against the real binary by
`scripts/nice-diff.sh` — **127 cases, 125 passed, 0 differed, 2 differ on
purpose**, plus 25 unit tests.

> **Amended 2026-08-30.** Those figures were first taken in an environment
> that *permitted* lowering the niceness, which hid a tenth defect: we called
> `nice(2)` where GNU calls `setpriority(2)`, so a *refusal* was reported with
> the wrong errno string. It stayed invisible until the harness ran somewhere
> `RLIMIT_NICE` is 0, where it became 9 differences at once. Now fixed — see
> `## nice reported a refused niceness with the wrong errno, because nice.c's
> nice(2) branch is dead code` below — and the counts above are accurate again.

### What the old version did

It accepted `-n N`, computed nothing from it, and ran the command at whatever
niceness it had inherited. Eight separate defects, each of which a shipped
`nice` would have been wrong about:

| # | The old behaviour | GNU |
|---|---|---|
| 1 | `-n N` parsed, then discarded; the command ran unchanged | the niceness is actually set |
| 2 | no default: `nice cmd` did nothing | `nice cmd` is `nice -n 10 cmd` |
| 3 | the obsolete `-NUM` form was an unknown option | `nice -5 cmd` is an adjustment |
| 4 | `argv` held as `Vec<String>` — a non-UTF-8 byte panicked | passed through untouched |
| 5 | a failed `exec` exited 1 | 127 not found, 126 found-but-not-runnable |
| 6 | a refused adjustment was fatal | a warning; the command still runs |
| 7 | no query form: `nice` alone printed usage | prints the current niceness |
| 8 | no `--adjustment`, no `--help`, no `--version` | all three |

Defect 1 is the one that makes the rest secondary: a batch script asking for
`nice -n 19 ./rebuild` got a full-priority rebuild, and nothing anywhere said
so. Text and exit status were *correct* throughout — which is why the harness
runs `nice` itself as the command under test in most cases, so that stdout
carries the scheduling parameter and a comparison of text becomes a
comparison of behaviour.

### Why the scan is interleaved rather than a pre-pass

The obsolete `-NUM` syntax cannot be described to getopt: `-5` is neither a
short option cluster nor an operand. The tempting shape — walk the words once
pulling out anything matching `-NUM`, then hand the rest to getopt — is wrong,
and wrong in a way that only shows up on a valid command line: `nice -n -5
cmd` would lose its `-5` to the pre-pass, and the `-n` would then report a
missing argument. Upstream alternates instead, and so does this port: test the
next word for the digit form, otherwise take **exactly one** item from getopt
and resume at `optind`. That is what `Parser::optind()` was added to
`src/getopt.rs` for.

The digit test is `s[0] == '-' && ISDIGIT (s[1 + (s[1] == '-' || s[1] ==
'+')])`, and the adjustment is `s + 1` — so `--5` is −5, `-+5` is +5, and
`--` is not caught by it. `-5x` *is* caught, which is why it produces
`invalid adjustment '5x'` rather than an unknown-option error.

### A refused niceness is a warning; an undeliverable warning is fatal

Unprivileged, a negative adjustment is refused by the kernel. GNU prints
`nice: cannot set niceness: Permission denied` and **runs the command anyway**,
returning the command's own status — treating it as fatal would break every
`nice -n -5` in every script run by a non-root user. But upstream then checks
`ferror (stderr)`, and if the warning could not be delivered it returns
`EXIT_CANCELED` after all. Both halves are reproduced, and both are covered:
`nice -n -5 true` is 0, `nice -n -5 true 2>&-` is 125.

The same rule produces the finding most likely to trip a re-implementation:
**`nice /nope 2>&-` exits 125, not 127.** The command was not found, but the
report of that could not be delivered, and the undeliverable report wins.
