## D-POSIX-BESSEL-HUGE-ORDERS-ARE-SLOW — `jnl` and `ynl` take time in proportion to the order where the answer is neither under- nor overflowing: seconds at an order of ten million, minutes at 2^31 (lane D, 2026-09-29) — **Status: FIXED 2026-09-29 -- from order 512, Debye's expansions away from the turning point, 12 to 20 microseconds a call in a release build; from order 2048 within 32 n^(1/3) of it too, a recurrence crossing from where they hold, at most about 6 ms (order 2^31); between, the recurrences, which cost no more there (under 0.2 ms); every value the correctly rounded one in all four directions at orders 600 to 2^31 - 1 (mpmath's recurrences at 80 digits, and Debye's sums beyond 2^16), and in agreement with the recurrences they replace and with the Wronskian at orders to 2^31**

**In short:** the `long double` Bessel functions of order `n` (`jnl(n, x)`
and `ynl(n, x)`, `posix/src/besl.rs`) compute their answer by stepping
through every order from 0 or 1 up to `n` (or down from just above it). For
the orders programs use -- up to a few hundred -- that is microseconds. For
a huge order with `x` near `n` or past it -- the only place such an answer
is neither zero nor infinite -- it is about a million steps a second: at
`n` = 10^6 a quarter of a second, at `n` = 2^31 some minutes. glibc steps
the same way (18 seconds for `jnl(2147483647, 1)`, in plain precision and
with the wrong sign), so nothing that worked before is slower; but a
function should not take minutes on any argument.

**Where:** `posix/src/besl.rs` -- `jn_scaled`'s forward recurrence and
`jn_miller`, and `yn_scaled`. Everywhere else the cost is bounded: an
answer Debye's estimate puts past the range returns at once.

**The proper fix:** Debye's asymptotic expansions for large orders, in
both of their forms -- `x = n sec(beta)` past the turning point, `x = n
sech(alpha)` before it -- whose Debye polynomials `u_k` a generator can
write as exact rationals; and across the turning point, where neither
converges, a short recurrence from a Debye value on the far side (upward
for `Y`, Miller's downward for `J`), about `n^(1/3)` steps. The same
double-long-double arithmetic and the same oracle (mpmath, which evaluates
large orders directly) test it.
