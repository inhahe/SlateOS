### [A] `check-selftest-reinit`'s rule is right and one case short: "empty and live" is still broken for a table something registers into at boot -- 2026-09-17

**Status:** OPEN

**In short:** a self-test that wipes its module's table must switch the table
back on before it finishes, and a checker enforces that across 273 call
sites. But "switched back on and empty" is only harmless if the table fills
up through use. If something registered a row into it at boot, that row is
gone and nothing puts it back -- so the table is live, empty, and wrong, and
the checker is satisfied.

**The existing rule, which is a good one.** `scripts/check-selftest-reinit.py`:

> A `self_test` that clears a `Mutex<Option<_>>` state table must re-open it
> before returning. Clearing is right; stopping there is not.

and it names the distinction exactly -- `*STATE.lock() = None` leaves the
module *switched off* for the rest of boot, because every writer goes through
a `with_state` helper that returns `NotSupported` while it is `None`, whereas
`None; init_defaults()` leaves it *empty and live*. 146 modules had the first
shape; 18 were opened by nothing except their own self-test.

**Where it stops short.** `binfmt` complies with that rule -- its `self_test`
ends with `*STATE.lock() = None; init_defaults();`, and its own comment says
why: "no fixtures leak into the live format table afterwards". Correct for a
table whose contents accumulate through use.

But `binfmt`'s contents do not accumulate through use. `record_load` requires
the format to have been **registered** first, or it returns `NotFound`. So
when I wired the boot to register `Elf64` at step ~12, the self-test later in
the battery re-opened the table empty and the registration was gone. The
`[binfmt]` report read `0 format(s), 0 load(s)` on a kernel that had just
executed dozens of binaries, and `check-selftest-reinit` was green throughout
-- correctly, by its own rule.

**The refinement.** For a module with boot-time registration, "empty and
live" is a third broken state alongside "switched off". Two ways out, and
the first is what I did:

| fix | where |
|---|---|
| register **after** the self-test dispatch | `main.rs`, beside the `Binfmt` dispatch -- one line moved |
| have the self-test restore what it wiped | inside the module, but then the fixtures it was avoiding come back unless it re-registers exactly the boot set |

**Not proposing a gate for it.** The class is "modules whose table needs a
boot-time registration to be useful", and the only way I know to identify
them is to notice that some `record_*` returns `NotFound` without a prior
`register_*` -- which needs call-graph reasoning, not a grep, and would
produce the same false positives as the module-doc probe (2 real of 8).
Recorded so the next person wiring a stats module reads it before choosing a
call site, which is the cheapest place for this to be known.
