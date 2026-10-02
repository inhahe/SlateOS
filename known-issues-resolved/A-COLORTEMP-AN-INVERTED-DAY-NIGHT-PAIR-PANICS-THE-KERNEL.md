## `A-COLORTEMP-AN-INVERTED-DAY-NIGHT-PAIR-PANICS-THE-KERNEL` (lane A, 2026-08-26) — **fixed 2026-08-26**

**In short:** the night-light feature let you say "during the day make the
screen 3000K, at night make it 6500K" — the two numbers the wrong way round
from how people normally use it, but a setting it accepted without complaint.
The next time it recalculated the colour during the twilight fade, the
arithmetic subtracted the larger number from the smaller one on a type that
cannot go below zero, and **the whole kernel panicked.** Any user of the shell
could reach it with two ordinary commands.

**Where.** `kernel/src/fs/colortemp.rs`, `update_for_time`, the two transition
arms. The code read:

```rust
// Evening transition: day → night.
day_k - ((day_k - night_k) * elapsed / total.max(1))
// Morning transition: night → day.
night_k + ((day_k - night_k) * elapsed / total.max(1))
```

Both compute `day_k - night_k` on `u32`. Nothing establishes that `day_k >=
night_k`: `set_day_night` clamps its two arguments into `1000..=10000`
*independently* and never compares them, and the shell surface
(`colortemp daynight <id> <d> <n>`) passes whatever it is given. The kernel
builds under `[profile.dev]`, which does not turn `overflow-checks` off, so the
underflow is a panic rather than a wrapped value.

That distinction matters for severity: this is not "the screen goes the wrong
colour." It is an unprivileged shell command halting the kernel.

**Reproduce** (kernel shell), before the fix:

```
colortemp init
colortemp daynight 1 3000 6500     ← accepted; day cooler than night
colortemp mode 1 scheduled
colortemp schedule 1 1200 420 30   ← sunset 20:00, sunrise 07:00, 30m fade
colortemp update 1 1215            ← 15 min into the evening fade → PANIC
```

The window is narrow but not obscure: any `current_min` inside
`[sunset, sunset + trans)` or the morning equivalent lands in a transition arm.
Outside those windows the function returns `day_k` or `night_k` whole and never
subtracts, which is why the existing tests 4 and 5 — noon and 01:40 — never
touched it.

**Fix.** Replaced both arms with a shared `lerp_kelvin(from, to, elapsed,
total)` that subtracts only in the direction the values actually run:

```rust
if to >= from { from + (to - from) * elapsed / total }
else          { from - (from - to) * elapsed / total }
```

This removes the assumption rather than documenting it — the alternative,
making `set_day_night` reject or swap an inverted pair, would impose a policy
the feature does not otherwise have (there is no rule that a "day" temperature
must be cooler; a user working nights may genuinely want the inversion) and
would still leave the raw subtraction one careless caller away from the same
panic. `elapsed` is clamped to `total` so a caller cannot extrapolate past
`to`, and `total` is floored at 1 so a zero-length transition divides safely.

**Regression test.** `colortemp::self_test` test 9 sets day 3000 / night 6500
and asserts both transition directions return 4750K — the exact midpoint, 15
minutes into a 30-minute fade. The evening call is the one that used to panic.

**How it was found.** Reading the subsystem for severity evidence while
clearing `cmd_colortemp` for
`A-KSHELL-A-HUNDRED-AND-NINETEEN-FUNCTIONS-GUESS-A-VALUE-FOR-A-WORD-THEY-COULD-NOT-READ`.
Unrelated to the parsing defect, and not reachable through it — the guessed
values are all in range; it is the *stored* pair that has to be inverted.

**Not a regression.** True since `update_for_time` was written.
