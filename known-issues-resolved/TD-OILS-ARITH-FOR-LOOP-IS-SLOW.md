### TD-OILS-ARITH-FOR-LOOP-IS-SLOW. a `for ((…))` loop costs ~9× what bash charges — ✅ CLOSED — 2026-08-01, closed 2026-08-02

**Where:** `userspace/oils/src/interp.rs` — the arithmetic-`for` execution path
and the per-item work it does each time round.

**The 9× was a debug artefact**, exactly as the original caveat suspected: the
figure came from an `opt-level=0` build. Measured properly, and then chased to
the end, the entry resolves into three findings — two real bugs (both fixed, both
with their own entries) and one build-configuration mistake (fixed) — leaving a
gap that is not a defect.

**What the hunt actually found.**

1. **A lone `[` was being globbed.** The one genuinely pathological figure, and
   it was not the loop at all: `[` is a builtin name, so every `[ … ]` test read
   the whole current directory to discover it matched nothing. 50 000
   `[ 1 -lt 2 ]` in the repo root went 10 996 ms → 679 ms. See
   TD-OILS-UNCLOSED-BRACKET-GLOBBED (fixed).
2. **Associative arrays were a linear scan**, making a fill quadratic — 40 000
   insertions 2 348 ms → 463 ms. See TD-OILS-ASSOC-ARRAY-IS-A-SCAN (fixed).
3. **osh was compiled `-Os`.** The workspace's userspace default is size-
   optimised, for a good reason that does not apply to the one binary that
   interprets every line of every script. `-O3` + `codegen-units = 1` is
   1.10×–1.37× faster across twelve constructs (geometric mean ~1.27×), nothing
   slower, for +884 KB. See design-decisions.md §100.

**Where it landed.** After all three, against bash 5.2.37 on the same host, best
of three, 100k–300k iteration loops:

| construct | osh | bash | ratio |
|---|---|---|---|
| `for ((i=0;i<N;i++)); do :; done` | 1 939 ms | 1 187 ms | 1.63× |
| `while ((i<N)); do ((i++)); done` | 1 271 ms | 997 ms | 1.27× |
| `while [ $i -lt N ]; do i=$((i+1)); done` | 3 496 ms | 2 475 ms | 1.41× |
| `for (( )); do s=$((s+i)); done` | 2 138 ms | 1 795 ms | 1.19× |
| `x=${v#h}` | 1 381 ms | 1 304 ms | 1.06× |
| `[[ abc123 =~ [0-9]+ ]]` | 859 ms | 924 ms | 0.93× |
| `case` | 1 035 ms | 999 ms | 1.04× |
| `echo hello` (to a file) | 1 998 ms | 1 640 ms | 1.22× |

**The two suspects this entry named were both wrong, and are recorded here so
nobody re-investigates them.**

- *`notify_signalled_jobs` per item.* It early-returns on
  `self.jobs.is_empty() && self.coproc_tracked.is_none()` — two field reads for
  a script with no jobs, which is every loop that matters.
- *The arithmetic evaluator re-parsing its expression each iteration.* It does,
  but caching the parse is both unsound and pointless. Unsound because
  `arith::parse` consults `vars.is_assoc(name)`, so the AST depends on shell
  state a `declare -A` mid-loop can change; pointless because the text handed to
  the parser is the *post-expansion* text, which a `$x` in the expression makes
  different every iteration anyway. bash re-parses too.
- A third suspect, checked and cleared by ablation: `$BASH_COMMAND`'s
  per-command `put_var` in `announce_debug`. Skipping it entirely is worth 0–8%,
  and was *negative* on one of the three cases.

**One measurement trap, for whoever benchmarks next.** Do not redirect to
`/dev/null` when comparing against Git-for-Windows bash. MSYS implements
`/dev/null` inside its DLL, so bash's write costs nothing while osh's is a real
`WriteFile` to `NUL`. That artefact alone made `echo`/`printf` look like a 1.85×
outlier; against a real file the same loop is 1.22×. Redirect to a file.

**Why this is closed rather than open.** What remains is a flat ~1.2–1.4×
constant across *every* construct, with no outlier and two constructs already
faster than bash. That is the signature of a tree-walking interpreter doing
somewhat more work per node than another tree-walking interpreter, not of a
defect with a location. There is nothing left in this entry to act on; a future
attempt should start from a profiler (ETW sampling needs an elevated shell,
which is why this hunt used ablation instead) and open a new entry against
whatever it names.

**Impact, retained.** A corpus case that spun 300 000 times as a builtin-only
delay blew through `osh-bash-diff.py`'s 20 s per-case timeout and failed the
whole corpus run. Cases wanting a busy-wait should still keep the count small or
use `sleep`.
