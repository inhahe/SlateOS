## `A-FS-ACCOUNTING-TABLES-ARE-CLOSED-FOR-THE-WHOLE-BOOT` (lane A, 2026-08-26) — **fixed**, 0 of 146

**In short:** the kernel has ~430 little modules that each keep a table of
numbers for `/proc` to print. Each has an `init_defaults()` that opens its
table, and a `self_test()` that runs at boot to check the module works. The
self-test *closed the table again when it finished* and nobody opened it back
up — so for the rest of the boot the module refused every write and `/proc`
printed zeros. Reading `/proc/cpustat` on a busy machine got you the same
output as reading it on an idle one.

**Fixed 2026-08-26** in 117 files: the teardown now re-opens the table instead
of leaving it dead. Pinned by `scripts/check-selftest-reinit.py`. The
measurement table and module lists below are kept as the record of what the
defect was, and are no longer a description of the tree.

**The mechanism, exactly.** Each of these modules keeps its data in
`static STATE: Mutex<Option<State>>`. `init_defaults()` sets it to `Some(...)`;
every writer goes through a `with_state` helper that returns
`KernelError::NotSupported` when it is `None`. The self-tests end with:

```rust
    // Leave NO residue: reset to the uninitialised state ...
    *STATE.lock() = None;
```

which is the right *intent* — a diagnostic must not leave its fixtures in the
live table — but the wrong *end state*. It leaves the table dead rather than
empty. `init_defaults()` is not idempotent-and-re-called; it is called once, and
for most of these modules the once is *inside the self-test itself*.

**Measured** (both counts are from a scan of `kernel/src/fs/*.rs` against
`main.rs` and `kshell.rs`; re-run it before trusting them):

| | modules | what `/proc` shows after boot |
|---|---|---|
| null `STATE` at the end of `self_test` | 146 | — |
| …re-opened at boot in `main.rs` | **4** | live |
| …opened only by the kshell command's lazy init | **124** | zeros until someone types the command, then live |
| …opened by *nothing* but their own self-test | **18** | zeros forever; not even the shell can open them |

The 18 that nothing can open: `binfmt`, `blkqueue`, `cpucache`, `cpustat`,
`inodestat`, `iolatency`, `kprobes`, `mempress`, `migstat`, `netdev`,
`netsock`, `pidstat`, `pipestat`, `powerstat`, `schedlat`, `sockbuf`,
`taskstats`, `writeback`.

**Why this is the prerequisite for the other burn-down, not a separate chore.**
That list is almost exactly the set of category-3 modules named as the
high-value targets in
`A-FS-MODULES-EXPOSE-MUTATORS-NOTHING-CAN-REACH` — the accounting tables a real
subsystem ought to be feeding. **Wiring a recorder into any of them today would
be a no-op**: the call would run, `with_state` would return `NotSupported`, and
the caller (which must discard accounting errors, or statistics could fail a
real operation) would drop it. The number in `/proc` would not move, and nothing
would say why. Fix the table's lifetime first, or the recorder work is invisible
and looks like it did not work.

**How it stayed hidden.** For the same reason as everything else in this family:
nothing wrote to these tables, so a table that refused writes and a table that
had no writers produced identical output. The two defects concealed each other.
It surfaced only when `futexstat` got a real writer and the writes still did not
appear — see the fix in `fs/futexstat.rs` and `main.rs`.

**The fix that was applied.** Every `self_test` now ends with the clear **and**
a re-`init_defaults()`, so the table is left empty rather than dead:

```rust
    *STATE.lock() = None;
    init_defaults();
```

Two lines, identical in all 117 sites, and safe because `init_defaults()` opens
with `if guard.is_some() { return; }` — it is idempotent, so it can never
double-seed a table.

Do **not** fix it by deleting the reset: the fixtures genuinely must not survive
into the live table, and rungs like `pagecache`'s "empty after init" depend on
starting clean.

**Step 2 turned out to be unnecessary, which is the useful finding here.** The
original plan called for a second pass adding boot-time `init_defaults()` calls
to `main.rs` for everything `/proc` publishes, on the theory that the 18
never-opened modules had no boot-time opener. They do: **all 18 already call
`self_test()` from `main.rs` at boot** — and so do the other 128. The self-test
*was* the boot-time initialiser all along. It opened the table, exercised it,
and then switched it off on the way out. So repairing the teardown does the
whole job, and the entire 146-module family goes live at boot from this one
change. No `main.rs` edit was needed.

**What the seeded tables do now.** Five of the 117 `init_defaults()` bodies
genuinely populate rows rather than leaving empty `Vec`s, and each was checked
individually before trusting the re-call — because re-opening a table that seeds
*fabricated* rows would publish fake data to `/proc`, which is exactly what the
"leave no residue" comment was protecting against (that is the real defect in
`A-SIGNALQ-INIT-DEFAULTS-SEEDS-SEVEN-DELIVERIES-THAT-NEVER-HAPPENED`). None of
the five do: `cputopo` reads `smp::cpu_count()`/`cpu_topology::cpu_topo()`,
`devicemgr` reads `pci::scan_bus0()`, `monitors` reads
`console::framebuffer_info()`, and `pagestat`/`softirq` lay out per-order and
per-type rows with every counter at zero. All observed or structural; none
invented. (`battery`, `webcam` and `oomkiller` tripped the first scan only in
their *comments*, which explain at length why they deliberately seed nothing.)

**The gate:** `scripts/check-selftest-reinit.py`, wired into `boot-test.sh`'s
`check-*.py` glob. Its baseline is **empty** — the burn-down reached zero in one
pass, so there is no shrinking backlog to pin, unlike `check-option-refusal.py`.

**Scope it to `self_test`, or it fires on correct code.** The first draft of the
migration matched on the statement shape alone and would have rewritten
`viewstate.rs`, whose `*GLOBAL_DEFAULTS.lock() = None;` sits in
`pub fn clear_global_defaults()` — where `None` is a *meaningful value* ("revert
to built-in"), not a dead table. Injecting `init_defaults()` there would have
inverted what a public API does. `net/dhcp.rs` clearing `*PENDING_OFFER.lock()`
to return its state machine to Idle is the same shape and equally correct. The
gate therefore only looks inside functions whose name contains `self_test`, and
the migration script was rewritten to consume the gate's `violations()` rather
than re-derive the target list — two heuristics for "which resets are wrong" is
one too many.

**Not a regression.** True of each module since its self-test was written.
