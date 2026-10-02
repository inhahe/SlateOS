## `B-FREE-DIVERGES-FROM-PROCPS-IN-SIX-MEASURED-PLACES` (lane B, 2026-08-27) -- **open**, deliberate divergence

**In short:** our `free` is a transcription of procps-ng 4.0.4's, and it was
checked the strongest way available: six fabricated `/proc/meminfo` files were
rendered under twelve option sets each, and **all 72 outputs are byte-identical
to the real binary**, trailing spaces included. Six things are on purpose
different. Two are upstream bugs we declined to copy; four are house rules.
Recorded here so a future reader who diffs the two does not "fix" them back.

| # | Upstream (measured) | Ours | Why |
|---|---|---|---|
| 1 | `swap_used = SwapTotal - SwapFree`, unconditionally. On a kernel that publishes `SwapUsed` and not `SwapFree` -- which is this one -- that reads **swap 100% full, always**. | When the `SwapFree` key is *absent* and `SwapUsed` is present, `SwapFree` is derived as `SwapTotal - SwapUsed`. | Upstream reports the most alarming thing this program can say, on every machine, regardless of the truth. A wrong answer, not a cosmetic one. See design decision 624. Keys on *absence*, so a Linux box whose swap really is full still reads full. |
| 2 | `-c` is parsed into a `long` and stored in an `int`, so which oversized counts are refused is an accident of truncation: `-c 2147483648` and `-c 4294967296` are `out of range`, but `-c 4294967297` **silently becomes `-c 1`** and prints once. | Anything outside `i32` is refused with upstream's own `out of range` sentence. | A count that wraps into a different count is a wrong answer. Makes all three cases agree, and matches what upstream's range check meant. See design decision 624. |
| 3 | `free -c ''` prints `failed to parse count argument: '': No such file or directory` -- `strtol_or_err` never zeroed `errno`, so the suffix is whatever the last unrelated library call left behind. | No suffix. | The suffix is a different sentence on a different machine and carries no information. (`-s ''` needs no divergence; `free.c` zeroes `errno` itself on that path.) |
| 4 | procps' library caches `/proc/meminfo` for one second, so `free -c 2 -s 0.1` prints **the same numbers twice**. | The file is re-read every iteration. | Repeating a stale reading is the one thing a repeat mode exists not to do. |
| 5 | `--version` prints `free from procps-ng 4.0.4`. | `free from SlateOS coreutils 0.1.0`. | Keeps the shape scripts grep for without claiming to be procps. Same call as `cal` and `renice`. |
| 6 | An argument echoed back in a diagnostic is printed as raw bytes. | Escaped through `quote::escape_unprintable`. | A diagnostic must not be a way to drive the terminal. Same call as every other bin in this sweep. |

**Everything else was measured and matched**, including the parts that look
like bugs and are not:

- **`used` can print negative.** With `MemTotal: 1`, `MemFree: 2`,
  `MemAvailable: 3`, the container guard resets `available` to `free`, the
  subtraction `1 - 2` underflows into an `unsigned long`, and `scale_size`
  multiplies by 1024 into a `long long` -- so the column reads `-1`, `-1024`
  under `-b`, `-1Ki` under `-h`. Reproduced exactly, wrap and all.
- **`-h` does not threshold.** It renders the value at each unit in turn and
  takes the first rendering that *fits the column* -- 4 characters for `--si`,
  5 for binary -- trying one decimal place first. That is why `10188 kB` is
  `9.9Mi` but `10240 kB` is `10Mi` (`10.0Mi` is six characters), and why
  `1023 kB` rounds *up* across a unit boundary to `1.0Mi`.
- **The column widths are a 9-character label field followed by `%11s`
  fields**, giving an 80-byte narrow header and a 92-byte wide one. Scripts
  slice this by offset, so the widths are pinned by `header_widths`.
- **Past `Pi` the column simply overflows** rather than growing a unit:
  `MemTotal: 9007199254740991 kB` prints `8191Pi`. Measured, not inferred.
- **Every command-line error is followed by the *whole* usage block on
  stderr**, never a `Try 'free --help'` referral -- and `free foo` prints that
  block with *no* message at all, status 1.
- **`-k -m` is fatal**: `Multiple unit options don't make sense.`, status 1,
  and no usage block.
- **`--k`, `--te`, `--s` and `--co` are all ambiguous prefixes**, and the
  `possibilities:` list is ordered by the long-option table, not
  alphabetically.
- **`cache` is `Cached + SReclaimable`**, and `-w` splits it back into
  `buffers` and `cache` as separate columns.

**Where it lives:** `userspace/coreutils/src/bin/free.rs` -- the "Measured
against procps-ng 4.0.4" table and the "Deliberate differences from procps-ng"
list in the module doc, and a test named for each behaviour. Divergence 1 is
pinned by `swap_used_substitutes_for_an_absent_swap_free`,
`a_genuine_swap_free_of_zero_is_left_alone` and
`neither_key_reproduces_upstream`; divergence 2 by `count_diagnostics`.

**What would change this:** #1 stops firing on its own the moment lane A adds
`SwapFree` to `gen_meminfo` (see the request file named in the entry below) --
it does not need removing then. #2--#6 are settled policy and should not be
revisited without changing the corresponding rule for every other bin in this
sweep.
