## B-COREUTILS-STAT-QUOTES-THE-FILE-LINE — WITHDRAWN, IT IS DESIGN DECISION §371 (lane B, 2026-09-11)

**This entry was wrong and is kept as a correction rather than deleted.**

I filed it hours after writing `scripts/stat-diff.sh`, on the strength of 25
failing cases where our `File:` line reads `File: 'file.txt'` and GNU's reads
`File: file.txt`. I had measured GNU carefully — including that it ignores
`QUOTING_STYLE` for that line while honouring it for `-c %N` — and concluded
ours was over-eager.

**It is `design-decisions.md` §371, taken deliberately on 2026-08-23, and
argued better than my fix would have been:**

  * GNU's own behaviour there is a **`strstr` accident**. `stat -c '%N'` quotes
    and `stat -c '%.3N'` does not, because the substring search for the literal
    two characters `%N` misses the second. A directive that differs only by a
    precision gets a different quoting style, which nobody designed.
  * A file name is attacker-chosen input in every case that matters — a
    tarball, a download directory, a shared `/tmp` — and `design.txt` permits
    every byte but `/` and NUL. The human-readable block is the one a person
    reads line by line, so a name that can forge a line of `stat`'s own output
    is not recoverable by the reader.
  * `%n` is still raw, one character away, for anything machine-read.

**§371's own "Against" section names this harness's failure mode exactly**,
which is the part worth carrying forward:

> Reproducing upstream bug-for-bug has value of its own: it is the property
> that makes "measure GNU, assert the measurement" a usable method, and every
> deliberate exception weakens it.

That is what happened. **A harness whose null hypothesis is "GNU is right"
reports every deliberate divergence as a defect, in proportion to how thorough
it is** — the third time today, after `uname -o` printing `SlateOS` and
`chown`'s SlateOS-gated syscall. The remedy is not a threshold; it is checking
`design-decisions.md` before filing, which I did not do. The 19 affected cases
are now `xfail`s in the harness naming §371, so they are still run and an XPASS
would report the divergence disappearing.
