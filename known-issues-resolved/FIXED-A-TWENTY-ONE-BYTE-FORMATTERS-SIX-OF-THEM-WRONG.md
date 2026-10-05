### FIXED-A-TWENTY-ONE-BYTE-FORMATTERS-SIX-OF-THEM-WRONG

**Status:** fixed. `kernel/src/bytesize.rs` is now the only byte formatter in
the kernel; all twenty-one private copies delegate to it.

**Symptom.** `TD-A-FORMAT-SIZE-PRINTED-A-TWO-DIGIT-TENTHS` — a size just under
2 KiB printed as `1.10 KiB`: two digits after the point, reading as *larger*
than the `1.9 KiB` below it. Found by a self-test that finally ran at boot.

**Cause, and why the fix could not be local.** The tenths digit was computed as
`remainder / (unit / 10)`. That is the obvious thing to write and it is off by
one whole digit at the top of every unit — `1023 / 100` is `10`, not a digit.
The same divisor appeared in **six** files, so fixing the one that had a
self-test would have left five to be rediscovered one at a time, each by
whoever next happened to look at a size near the top of a unit.

Surveying for the divisor turned up twenty-one private byte formatters in all,
disagreeing on more than arithmetic:

| Fault | Copies | What a user saw |
|---|---|---|
| Tenths bug | 6 | `1.10 KiB` — two digits, sorts above `1.9 KiB` |
| `KB`/`MB`/`GB` on 1024-based units | 6 | ~7% overstatement per unit |
| `f64` division in kernel code | 7 | silently lossy above 2^53 bytes |
| Stopped at GiB | 9 | `4096.0 GiB` instead of `4.0 TiB` |
| Truncated to whole units | 3 | 1 GiB and 1.9 GiB of RAM read alike |
| No KiB branch at all | 1 | `1048575 B` |

`net/iperf.rs` had the tenths bug *squared*: `(bytes % 1024) / 10` reaches
**102**, and with `{:02}` that printed `1.102 KB` — three digits in a
two-digit field.

**Fix.** One module, three entry points, integer arithmetic throughout:
`iec` (`1.5 MiB`, the prose form, what almost everything uses), `iec2`
(two digits, for throughput totals where a tenth of a GiB is 107 MB of slack),
and `compact` (`1.5M`, the fixed-width column form — so `ls -h`, `df` and `du`
finally agree, which they did not: `du` truncated to whole units and reported a
1.9 MiB directory as `1M` while `ls -h` called the file inside it `1.9M`).
`remainder * scale / unit` scales before dividing, so the fraction is in
`0..scale` by construction rather than by choosing a divisor carefully. The
hundredths path widens to `u128` because `(2^60 - 1) * 100` needs 67 bits.

**The lesson, and what it cost.** Two of the copies were wrong in the same way
and only one had a test. A formatter is exactly the kind of code that looks too
small to share — and so gets rewritten from memory, with the same off-by-one
each time. `bytesize`'s module doc argues explicitly that a *fourth* entry point
must justify itself, because every difference between the twenty-one beyond the
three real ones was accident.

Two self-tests had been asserting the *wrong* behaviour and had to be corrected
with the migration: `datausage` asserted `contains("MB")` for 1_500_000 bytes
(which is 1.4 MiB), and `iperf` asserted `contains("MB")`/`contains("GB")`.
Both passed only because the code they tested was mislabelling the unit. Both
now assert the whole string, since `contains` would not have caught a wrong
number either.
