## 1439. A parse is given up for the work it has done, not the time it has taken

**Date:** 2026-09-28 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C

**In short:** The code editor colours a file by parsing it a few milliseconds
per frame. A grammar with a mistake in it could keep that going forever, so
a parse that has gone on far too long is given up and the file stays in the
colours it had. "Too long" was five seconds of wall-clock time -- and on a
busy machine a perfectly ordinary file took that long, because the clock kept
running while the editor waited its turn for the processor. Two of the
highlighter's own tests failed that way while a boot test ran beside them.
Now "too long" is counted in the parser's own work -- its steps and the
characters its lexers read -- which a busy machine cannot inflate. Ordinary
files never come near the limit; a runaway grammar still hits it.

**What was measured** (`cargo test`, debug build, 2026-09-28): the
runtime's steps (its progress callback fires once per hundred) and the
characters every lexer stepped over (counted in `ffi::Lexer::advance_with`,
which generated lexers and hand-ported scanners both go through), per byte of
input:

| Input | Steps / byte | Characters / byte |
|---|---|---|
| Real files: Rust (212 KB), Python (937 KB), C, TOML (`Cargo.lock`), YAML, CSS, JSON (199 KB), Markdown (`roadmap.md`, 2 MB) | 0.2 -- 1.4 | 1.0 -- 4.3 |
| Each language fed another's file (Markdown as Rust, C, Python, CSS, TOML, YAML, JSON; Rust as Python, YAML, TOML, JSON, CSS, Markdown; ...) and 100 KB of random printable bytes | 0 -- 2.1 | 0 -- 3.4 |
| 50,000 `(` as Rust, `{` as C, `"` as Python | 1.0 | 1.0 |
| **10,000 `*` as Markdown** | 3.0 | **5,001** |

The last is a scanner that reads the rest of the line again for every token
of it: quadratic, 22 seconds in a debug build for 10 KB, and invisible to the
runtime's step count. The limit is two million units, plus two hundred a
byte -- more than twenty-five times the most any ordinary input needed -- so
that line is given up after a fraction of a second, and every file above
finishes.

**The alternatives:**

| Option | For | Against |
|---|---|---|
| **Work: runtime steps + characters lexed** (chosen) | the same on any machine, however busy; tests of it are exact | counts what the runtime reports and what our lexers do, not everything the runtime does: error recovery spends time it does not count (TOML fed 200 KB of Markdown: 33 s in a debug build on 118,000 steps). That time is finite, so it ends -- it is only never *given up* |
| Wall-clock time, as before, but longer | one number; bounds everything | still wrong under load, only less often; a long enough limit to be safe lets a quadratic scanner run for minutes |
| The thread's CPU time | not stretched by waiting | no portable way to read it (`GetThreadTimes` ticks at 15.6 ms, `CLOCK_THREAD_CPUTIME_ID` is POSIX, SlateOS's is unknown); per-slice sums of coarse ticks drift |
| Both work and a wall-clock backstop | bounds the uncounted too | the backstop brings the load problem back, for a case -- slow but finite error recovery -- where finishing is better than giving up |

**Why no time limit at all is safe.** A loop that goes through the parser's
steps reaches the progress callback, and is counted. A loop that never does
-- inside one scanner call, or in runtime code between checks -- cannot be
stopped by any limit checked in that callback, the clock included; the
frame budget cannot stop it either. So a time limit adds nothing against a
true runaway; it only gives up on slow finite parses, which are better
finished.

**Revisit if** a grammar is found whose ordinary files need more than two
hundred units a byte (raise the rate, with the file as a test), or the
runtime starts reporting more of its work (then count that too).
