## apps/** bug-hunt sweep, round 2: twenty-nine programs divided by 1024 and called it KB (lane C)

**Filed:** 2026-08-21 by Lane C.
**Status:** FIXED — `textfmt::bytes` plus a 52-call-site migration across 45 files.
See design-decisions.md §489 for the units policy this settles.

### The defect

Forty-four functions in the tree turned a byte count into text for a person to
read: `format_size`, `format_bytes`, `human_size`, `human_file_size`. **Twenty-
seven of them divided by 1024 and labelled the result `KB`, `MB`, `GB`.** Three
more formatted bytes *per second* and split the same way, two of them wrong, so
the final tally is forty-seven hand-written implementations and twenty-nine
mislabellings.

That is not a style quibble. `KB` is 1000 bytes; `KiB` is 1024. The two differ
by 2.4% at kilobytes and by 10% at terabytes, so the number shown to the user
was simply not the quantity the label named:

| Real size | What it said | What that label means |
|---|---|---|
| 1 GiB file (`apps/explorer`) | `1.00 GB` | 1.07 GB |
| 4 TiB disk (`apps/diskimager`) | `4096.00 GB` | 4.40 TB, and not in gigabytes |
| 1 500 000 B of traffic (`network_settings`) | `1.4 MB` | 1.5 MB |

The last row is the one that proves it was drift rather than a considered
choice: **`gui/desktop` displayed the same network byte counters in two units
on one screen.** The tray indicator (`network_indicator.rs`) divided by 1000 and
said `MB`; the settings page (`network_settings.rs`) divided by 1024 and also
said `MB`. Same subsystem, same counter, two different numbers, and at most one
of them right.

The tally across all forty-four size formatters (the counts below are recomputed
from the migration diff itself, not from the first grep, which under-counted):

| | Count |
|---|---|
| base 1024, IEC names (`KiB`) — correct | 14 |
| base 1000, SI names (`KB`) — correct | 3 |
| **base 1024, SI names — wrong** | **27** |

### The second defect, present in forty-three of the forty-four

Every copy but one chose its unit from the *unrounded* byte count and rounded
afterwards, so the printed mantissa could reach the base while keeping the
smaller unit's name. 1 048 575 bytes is one byte under 1 MiB, so the `KiB`
branch was taken, and `1048575 / 1024 = 1023.999…` printed as **`1024.0 KiB`** —
a quantity that cannot exist, since the entire point of the unit is that there
are 1024 of them.

The exception is `apps/partmanager`, which was the most careful of the forty-four
in every respect: pure integer arithmetic, no floats, and it even suppressed a
trailing `.00`. It truncated rather than rounded, which is why it alone never
printed a full base.

A third, milder problem: several stopped at `GB` or `TB`, so large volumes were
described in thousands of the wrong unit.

### The third defect: the same mistake again, in rate form

Three more functions formatted bytes *per second*, and they reproduced the
split exactly:

| | Divides by | Prints |
|---|---|---|
| `gui/desktop/network_indicator.rs` `format_rate` | 1000 | `MB/s` — correct |
| `gui/desktop/resmon.rs` `format_bytes_per_sec` | 1024 | `MB/s` — wrong |
| `apps/diskimager` `speed_display` | 1024 | `MB/s` — wrong |

The first two are the *same quantity* — a network link's throughput — rendered
by the tray indicator and by the resource monitor's network graph, which a user
can have open side by side. They disagreed by 2.4% while claiming the same
unit. `resmon`'s copy is reached only from `ResourceType::Network`, so there
was never a domain difference to justify it; it was drift.

`textfmt::bytes` therefore also exports `iec_rate`/`si_rate`, which are `scale`
plus a `/s`. A rate now cannot disagree with the size it is a rate of, and the
suffix is spelled in one place rather than six.

One more instance was not a function at all: `apps/archivemanager` refused a
split-volume size below `65536` with the message *"Volume size must be at least
64 KB"* — a hand-written label with the same 1024-vs-1000 error, and a test
asserting it. Both now say `64 KiB`.

### The fourth defect: the scaling had moved upstream, into the interface

`apps/sysinfo` had no wrong formatter — it had wrong *data*. `DiskInfo` and
`PartitionInfo` stored `capacity_gb`/`used_gb`/`free_gb` as `f32`, already
divided by 1024³, and four display sites printed them with `format!("{:.1} GB")`.
So the "Samsung 990 Pro 2TB" read **`1863.0 GB`**, which is its capacity in
neither unit — 2000 GB, or 1863 GiB — and the disk's own figure did not equal
the sum of its partitions'.

The important part is where the division lived. `apps/sysinfo/src/hwquery.rs`
parses `/sys/hardware/block`, and the *node's own keys* were `capacity_gb`,
`used_gb`, `free_gb` — floats. A reader of that interface cannot tell whether
the producer divided by 1000 or 1024, and this one did not have to guess only
because the same crate wrote both halves. Linux publishes `/sys/block/*/size`
as a raw sector count for exactly this reason. The keys are now
`capacity_bytes`/`used_bytes`/`free_bytes`, integers, and nothing in the tree
produces the node yet, so the rename costs nothing today and stops a driver
from baking a divisor into an ABI tomorrow.

Both structs now hold `u64` byte counts and scale at the point of display. The
mock data was re-derived so the partitions sum exactly to the disk (2 TB =
2 000 398 934 016 B = EFI + root + home), which the pre-scaled gigabyte figures
did not.

This is the shape a `fn format_*` grep cannot find, and it is the most dangerous
of the four: the other three put the mistake in a function that could be
replaced, while this one put it in a field name and a wire format.

### Why this is finding 9 and not a style cleanup

It is the sweep's recurring shape in its clearest form yet. The two halves of
the answer — **what to divide by** and **what to call the result** — were
written as two *independent statements* in each of forty-seven places. Nothing
tied them together, so they could disagree; across enough copies they did, and
a majority of the copies ended up on the wrong side.

Note what could not have caught it. Every output was a plausible string. No
value was out of range, nothing overflowed, no test failed, and the number was
never absurd enough to look wrong — `1.00 GB` for a 1 GiB file is off by 7%,
which is invisible unless you already know the answer. As with the QR
codeword tables in round 1, **only the table compared against itself** found it.

### The fix

`textfmt::bytes`, a new module in the dependency-free crate:

- `iec(bytes)` → `1.5 MiB` (base 1024), `si(bytes)` → `1.5 MB` (base 1000),
  and `scale(bytes, Unit)` for callers choosing at run time.
- **The base and the unit names are one table**, chosen together by `Unit`. A
  caller selects a family, never a divisor, so 1024-with-`KB` is now unspellable.
- Rounds to tenths *first* and promotes afterwards, so the mantissa is always
  below the base — `iec(1_048_575)` is `1.0 MiB`, not `1024.0 KiB`.
- Runs to `EiB`/`EB`, past `u64::MAX`, so nothing saturates.
- All-integer `u128` arithmetic with a `NonZeroU128` divisor: no float rounding
  in the decision, and the division cannot be by zero.

Re-exported as `guitk::bytes` for the GUI callers. The three headless crates —
`apps/backup`, `apps/indexer`, `apps/installer` — depend on `textfmt`
directly, which is exactly the three the crate's own module doc predicted would
need it. That is the layering argument in
`requests/c-b-year-of-day-computes-the-month-and-day-and-throws-them-away.md`,
holding for a second primitive.

All 52 call sites across 45 files now delegate — 42 `iec`, 7 `si`, 1 `iec_rate`,
2 `si_rate`. That is five more sites than the forty-seven implementations
replaced, because `apps/remotedesktop`'s single formatter became two (see below)
and `apps/sysinfo`'s pre-scaled `capacity_gb` field became four display-site
calls.

### Which family, decided once instead of forty-seven times

The rule §489 records, and the reason the SI set grew from 4 to 9: **bytes
*moved over a link* are decimal; bytes *occupying storage* are binary.** It is
stated that way so it can be applied to the next call site without re-deriving
it from first principles, which is what forty-seven authors each failed to do.

The consequences worth knowing:

- `netmanager` and `vpnmanager` report interface/tunnel `rx_bytes`/`tx_bytes`
  counters — the same counters the tray indicator and the network settings page
  show — so they became SI. They had been base-1024-labelled-`KB`, i.e. wrong
  under either reading.
- `apps/remotedesktop` had one formatter serving both a *file transfer's* size
  and the *session's* link counters. It now has two named functions, because
  the answer genuinely differs: the file is the same file the explorer lists.
- `apps/torrent` stayed binary throughout, including `downloaded`/`uploaded`.
  Those count bytes of file content, not link traffic, and every torrent client
  in existence quotes piece lengths in MiB.

### What did not change

The **numbers**, almost everywhere. All 29 broken sites were already
*computing* base-1024 values, so correcting them to IEC names changed only
text. Numbers moved only where a display was reclassified as decimal:
`network_settings` (1.4 → 1.5 MB), `resmon`'s network graph, `netmanager` and
`vpnmanager`. In every one of those cases the number moved *into* agreement
with another window showing the same counter.

### The same conflation, in the inverse direction — not fixed

Two places *parse* a size suffix rather than print one, and both map `KB`/`MB`/
`GB` to binary multipliers:

- `apps/installer/src/lib.rs:1211` — the partition-size parser. A config saying
  `size = "500 GB"` requests 537 GB, so a partition table written from a drive's
  advertised capacity does not fit on that drive.
- `apps/archivemanager/src/main.rs:258` — the same table, in a test fixture.

This was deliberately **not** changed with the display side. Printing a number
under a label that means something else is unambiguously false; *reading* a
suffix is an input convention, and the surrounding ecosystem genuinely splits
(`fdisk` and `parted` treat every suffix as binary). Changing it would silently
resize partitions described by existing config files. Filed as
`open-questions.md` **Q55** with three options, because it is a user-visible
policy with no obviously-correct answer.

### Related

The formatter is the third primitive extracted by this sweep for the same
reason, after `guitk::grid::columns_across` (round 1, finding 7) and the
`tzrules::civil_from_days` request still open with lane B. In all three the
tree held one correct answer that callers could not reach, and grew wrong
copies of it. Whether that is worth generalising — an audit for *any* small
computation restated more than three times — is the natural round-3 question.
