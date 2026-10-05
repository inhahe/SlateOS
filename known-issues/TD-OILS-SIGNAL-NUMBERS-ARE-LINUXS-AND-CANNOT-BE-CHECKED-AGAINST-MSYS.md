### TD-OILS-SIGNAL-NUMBERS-ARE-LINUXS-AND-CANNOT-BE-CHECKED-AGAINST-MSYS. `kill -l` and `trap` model Linux's signal table; the development host's bash uses a BSD-ish MSYS one — 2026-08-04 — **VERIFIED CORRECT 2026-08-25**; the premise "unverifiable on this host" is retired

> **Measured against glibc bash, 2026-08-25.** `scripts/osh-diff.sh` compares
> osh against `/usr/bin/bash` inside WSL, so the table below is now checkable
> and has been checked. **osh's numbering for signals 1–31 is identical to
> glibc bash's, name for name and number for number** — including the three
> this entry called out as differing under MSYS:
>
> | signal | osh | glibc bash | MSYS bash (the old reference) |
> |---|---|---|---|
> | `SIGBUS` | 7 | **7** | 10 |
> | `SIGUSR1` | 10 | **10** | — |
> | `SIGUSR2` | 12 | **12** | — |
> | `SIGSYS` | 31 | **31** | 12 |
> | `SIGEMT` | rejected | **rejected** | 7 |
>
> Both shells reject `SIGEMT` with the same wording (`kill: EMT: invalid signal
> specification`), so the agreement extends to the error path. Nothing here was
> ever an osh defect, and it is no longer unverifiable — it is verified.
>
> **One real gap did surface**, which is what a correct reference is for:
> osh's table stops at 31, while glibc bash exposes the 31 real-time signals
> 34–64. `kill -l RTMIN` answers `34` in bash and `invalid signal
> specification` in osh. That is an osh limitation rather than a reference
> artifact, and it is tracked separately as
> `TD-OILS-NO-REAL-TIME-SIGNALS` below.

**Where:** `userspace/oils/src/interp.rs` — the signal name/number table behind
`kill`, `trap` and `$?`'s 128+n encoding.

**What:** osh numbers signals the way Linux does (SIGBUS 7, SIGUSR1 10,
SIGUSR2 12) and stops at 31. MSYS bash numbers them the BSD way (SIGEMT 7,
SIGBUS 10, SIGSYS 12) and exposes 64 including the real-time range. Every
*behavioural* section — what `kill` accepts, how `trap` lists, what a killed
job reports — matches; only the table differs.

**Why deferred:** it is not a bug. osh targets SlateOS, whose signal numbering
is the Linux one by design; the divergence is the reference shell's host libc,
not osh. It is logged so a corpus case is never written against `kill -l`
output, and so a future session does not mistake the difference for a defect.

**Proper fix:** none needed for the numbering. If the signal *table* ever needs
testing, it has to be against a glibc bash, not MSYS — which is also why the
existing `kill` corpus cases test behaviour and never the numbers.
