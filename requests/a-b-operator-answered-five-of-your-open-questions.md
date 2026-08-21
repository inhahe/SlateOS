# Lane A → Lane B: the operator answered five of your open questions on 2026-08-21

**Status:** informational — nothing for lane A to do, everything for lane B.

**Why you are hearing this from lane A and not from the operator.** The answers
arrived in a single batch appended to a lane-A autonomous-loop tick, covering
all three lanes' questions at once. Lane A has relayed them into
`open-questions.md` (each question's `Status:` line now reads **ANSWERED
2026-08-21 by the operator**, with a quote block naming the choice), but has
deliberately **not** written the `design-decisions.md` entries for them —
lane B owns `design-decisions.md` §300–399 and owns these subsystems.

## The answers, verbatim

| Question | Operator's answer |
|---|---|
| **Q48** — kernel objects for "set the clock" / "bind port 80" / "raise your own rlimit", or leave them denied? | `q48: b` |
| **B-Q2** — GNU's curly quotes `‘zzz’` in error messages, or keep straight ones? | `b-q2: b` |
| **B-Q3** — pre-existing password hashes that can no longer be checked: accept fail-closed, or let those users in once more? | `b-q3: a` |
| **B-Q4** — two user databases that drift apart: which one is real? | `b-q4: c` |
| **B-Q6** — should the console login prompt obey the system-wide failed-guess delay? | *"i'll go with your recommendation"* (this phrase covered Q52, Q53, Q54 and B-Q6 together) |

For **B-Q6**, "your recommendation" means the one recorded under
`### My recommendation` in that entry — read it there rather than trusting this
summary, since lane A has not re-derived it.

## B-Q5 is *not* in the table, and the reason is worth your attention

The operator did not choose an option for B-Q5. They asked a question back:

> *"if libc.a builds byte-reproducibly from the same source, would c be the best
> option? because if so, maybe you should test if it does and then update the
> question?"*

**Lane A ran that test. `libc.a` is byte-reproducible.** Two independent checks:

1. Lane B built it in `os-lane-b`; lane A rebuilt it in `os-lane-a`. The
   archives are **byte-identical** (`5e252d0d…`), so no build path — not the
   worktree name, not an absolute path, not a timestamp — reaches the archive
   bytes.
2. `cargo clean -p posix` removed 0 files, which would have made check 1 a cache
   hit and therefore weak evidence. `touch posix/src/lib.rs` forced a genuine
   full recompile (verified: `grep -c "Compiling posix"` = 1) and the hash was
   **the same again**.

So option C's one unverified premise now holds. The full write-up, including the
honest limits (same machine, same toolchain — a toolchain upgrade churning the
file once is arguably *correct* behaviour), is in `open-questions.md` under
`### UPDATE 2026-08-21 (lane A)`.

**A second, independent argument for C surfaced in the same session, and lane A
thinks it is worth more than the reproducibility answer.** `scripts/ctest-fixtures.py check`
reported all nine ctest fixtures STALE and advised **"rebuild the fixtures"** —
when the side that had actually moved was `libc.a` (the sysroot was two `posix/`
commits behind). Following that advice would have relinked all nine against a
stale libc and reproduced the 2026-08-16 incident by hand. The gate holds one
hash and structurally cannot tell which side moved. What saved it here was
`create-ext4-rootfs.sh`'s **mtime** gate — but that gate is documented as silent
in a fresh clone, so in CI only the wrong advice survives.

A cheap fix within option A, if you keep A: the gate could compare `libc.a`
against a committed identity and split the diagnosis —

| `libc.a` vs committed id | ELF vs stamp | Remedy to print |
|---|---|---|
| differs | — | **rebuild the sysroot** |
| matches | differs | **rebuild the fixture** |

**The call is yours.** Lane A supplied the measurement the operator asked for and
is not deciding B-Q5.

## One more datum for whichever option you pick

Rebuilding `libc.a` to *byte-identical* content still moves its mtime, and
`create-ext4-rootfs.sh` then emits nine `WARNING: ctest-*.elf is OLDER than the
sysroot libc.a` lines that are pure noise — the content stamps all match. The
mtime gate cannot see that the rebuild was a no-op. That is a small, concrete
case of the same "mtime is the wrong oracle" argument the stamp system was
introduced for.

---
Filed by lane A, 2026-08-21.
