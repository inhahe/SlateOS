## 624. `free` derives `SwapFree` from `SwapUsed` when the kernel omits it, and refuses a `-c` count that would wrap

**Date:** 2026-08-27
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** our `free` — the command that prints how much memory the machine
is using — is a line-by-line copy of the real `free` from procps-ng, so that
scripts which slice its columns keep working. Two places diverge on purpose,
and both are applications of the rule set in 623 ("copy a bug that only *looks*
wrong; fix one that *is* wrong"). First: the real `free` works out how much
swap is in use by subtracting the kernel's "SwapFree" figure from the total,
but this OS's kernel does not publish a "SwapFree" line at all — it publishes
"SwapUsed" instead — so a faithful copy would treat the missing number as zero
and announce that swap is **100% full** on every SlateOS machine that has any.
Second: the real `free -c <n>` (repeat n times) reads the count into a wide
integer and then stores it in a narrow one, so `free -c 4294967297` quietly
becomes `free -c 1`. Both produce a confidently wrong number, so both are
fixed.

### The swap substitution

`free.c`'s derivation is unconditional:

```c
if (mem.swap_free < mem.swap_total)
        mem.swap_used = mem.swap_total - mem.swap_free;
```

`swap_free` is whatever `meminfo.c` scraped out of `/proc/meminfo`, and a key
that is not in the file leaves the field at its zero-initialised value. So on
a kernel that omits `SwapFree`, `swap_used` becomes `swap_total` — the whole of
swap — and the `free` column reads `0`. `SwapUsed` is not in procps' parse
table at all, so publishing it instead does not help. All three rows below were
measured against procps-ng 4.0.4 with the file bind-mounted over
`/proc/meminfo` in a private mount namespace (`unshare -r -m`):

| `/proc/meminfo` | Upstream prints | True state |
|---|---|---|
| `SwapTotal 2000000`, no `SwapFree` | `Swap:  2000000  2000000  0` | unknown — the file does not say |
| `SwapTotal 2000000`, `SwapUsed 500000` | `Swap:  2000000  2000000  0` | 25% used |
| `SwapTotal 2000000`, `SwapFree 2000000` | `Swap:  2000000  0  2000000` | 0% used — correct |

Row 2 is this OS today: `kernel/src/fs/procfs.rs`'s `gen_meminfo` emits
`SwapUsed` and not `SwapFree` (see
`requests/b-a-proc-meminfo-omits-the-linux-keys-that-thirteen-tools-read.md`,
which asks lane A to add the Linux keys). "Swap is completely full" is the
single most alarming thing this program can say, and it would say it on every
machine, always, regardless of the truth. That is not a cosmetic
misalignment; it is a wrong answer, and an operator acting on it would go
looking for a memory leak that does not exist.

So `derive()` substitutes before running upstream's derivation unchanged:

```rust
if !raw.swap_free_seen && raw.swap_used_seen {
    m.swap_free = raw.swap_total.saturating_sub(raw.swap_used);
}
```

The condition is deliberately on the *key being absent*, not on the value
being zero. A Linux machine whose swap genuinely is 100% full publishes
`SwapFree: 0`, `swap_free_seen` is true, and nothing is substituted — it still
reads full, which is correct. The two tests
`swap_used_substitutes_for_an_absent_swap_free` and
`a_genuine_swap_free_of_zero_is_left_alone` pin exactly that distinction, and
`neither_key_reproduces_upstream` pins that a file with neither key still
produces upstream's output byte for byte.

This divergence is self-cancelling: when lane A adds `SwapFree` to
`gen_meminfo`, `swap_free_seen` becomes true everywhere and the branch stops
firing on its own. It does not need to be removed then, and removing it would
only re-break the case where a future kernel drops the key again.

> **Update, 2026-08-27 (same day):** lane A landed it — `gen_meminfo` now emits
> `SwapFree` along with the rest of the Linux key set (see 625). The
> substitution above has therefore stopped firing on SlateOS, exactly as
> predicted and without a line changing here, and it remains as the guard for a
> kernel that publishes `SwapUsed` alone. Lane A declined two keys on purpose
> (`Committed_AS` and `CommitLimit`) and invited this lane to push back:
> `requests/a-b-meminfo-linux-keys-landed-except-the-two-that-would-lie.md`.

### The `-c` truncation

`free.c` parses the count into a `long` and assigns it to `int repeat_counter`:

```c
args.repeat_counter = strtol_or_err(optarg, _("failed to parse count argument"));
if (args.repeat_counter < 1) ...
```

The range check runs on the *truncated* value, so which oversized counts are
caught is an accident of which bits survive. Measured on procps-ng 4.0.4:

| Command | Upstream | Why |
|---|---|---|
| `free -c 2147483648` | refused, `out of range` | truncates to `INT_MIN` |
| `free -c 4294967296` | refused, `out of range` | truncates to `0` |
| `free -c 4294967297` | **prints once and exits 0** | truncates to `1` |

Row 3 is the wrong answer: the user asked for four billion iterations and got
one, with no diagnostic. Rows 1 and 2 happen to be refused, so the visible
behaviour is "some huge counts are errors and some are silently a different
count" — which is worse than either consistent choice. Our `scan()` refuses
anything outside `i32` with upstream's own `out of range` sentence, which makes
all three rows agree and matches what upstream *meant* by its range check.
`count_diagnostics` is the pin.

### Why the other four divergences are not decisions

The module doc lists six differences from procps. The remaining four are not
judgement calls under 623 and are recorded there rather than here: re-reading
`/proc/meminfo` each iteration (procps' one-second library cache makes
`free -c 2 -s 0.1` print identical numbers twice, which defeats the point of a
repeat mode); the SlateOS `--version` string, which every tool here carries;
routing echoed argument bytes through `escape_unprintable`, which is a
house-wide terminal-safety rule; and `free -c ''` omitting a `strerror` suffix
that upstream produces only because it forgot to zero `errno`, so the suffix is
a different sentence on different machines and carries no information.

### How to reverse

For the swap substitution, delete the two-line `if` at the top of `derive()`
and the `swap_free_seen`/`swap_used_seen` fields on `Raw`, and delete the two
tests named above. For the count, replace the `i32` range check in `scan()`'s
`-c` arm with `value as i32` and adjust `count_diagnostics`. In both cases move
the module-doc bullet into a "reproduced upstream bugs" list, as 622 did for
`cal`.
