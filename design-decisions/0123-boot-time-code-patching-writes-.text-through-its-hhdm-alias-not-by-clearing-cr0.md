## §123 — Boot-time code patching writes `.text` through its HHDM alias, not by clearing `CR0.WP`

**Date:** 2026-08-13
**Decided by:** Claude (autonomous)

**Context.** `clac`/`stac` raise #UD on a CPU without CPUID.SMAP, so the `clac`
that `B-AC-INHERITED-AT-KERNEL-ENTRY` requires at the top of every ISR stub
cannot simply be assembled in unconditionally. The standard answer — Linux's,
and now ours in `kernel/src/alternatives.rs` — is to reserve a 3-byte NOP at
each site and overwrite it at boot iff CPUID says the feature exists. That
keeps the hot path branch-free, but it means the kernel must write to its own
`.text` exactly once, early.

The first implementation just stored to `entry.site` directly, reasoning that
`.text` is not made read-only until `mm::protect::harden_kernel_sections()`
runs, thousands of lines later in `kmain`. That was wrong, and the boot test
said so immediately:

```
EXCEPTION: Page Fault (#PF) at 0xffffffff81efcb87, address=0xffffffff8110aaad, error=0x3
```

`error=0x3` is present + write: a write to a read-only page. `.text` is
read-only from the *first instruction* — `linker.ld` gives it a `PT_LOAD` with
`FLAGS(R|X)` and Limine honours that. `harden_kernel_sections()` re-asserts
W^X; it does not establish it.

**Decision.** Patch through a *second, writable mapping* of the same physical
pages — the HHDM, which Limine already provides read-write over all of physical
memory. `boot::executable_address()` (a newly-added Limine
executable-address request) gives the kernel's physical load base, so a
`.text` virtual address converts to its writable alias by a single constant
offset, with no page-table walk. The executable mapping is never modified.

**Alternatives considered.**

1. **Clear `CR0.WP` around the patch loop.** The obvious trick, and the one
   most hobby kernels reach for.
   - *For:* three instructions, no dependency on the HHDM or on knowing where
     the kernel was loaded, works before any memory subsystem exists.
   - *Against:* `CR0.WP` is not scoped to the patch — it disables write
     protection for *this entire CPU* for the duration, so any unrelated stray
     write anywhere in that window silently corrupts read-only kernel memory
     instead of faulting. It also creates a genuine W^X hole (briefly, all of
     `.text` is writable *and* executable), which is exactly the property
     `audit_kernel_wx()` exists to prove we never have. Rejected.

2. **Temporarily `change_flags()` the site's page to writable, then restore.**
   - *For:* narrowly scoped to the pages actually being patched.
   - *Against:* still makes a page W+X for a window, still needs a TLB flush
     per site, and — decisively — needs `mm::page_table` initialized, which it
     is not this early (`page_table::init()` is ~100 lines further down
     `kmain` than `alternatives::apply()`). Moving the patcher later would
     mean every alternative stays at its default for that stretch of boot.
     Rejected.

3. **Write through the HHDM alias.** Chosen.
   - *For:* the executable mapping is never writable, not even transiently, so
     W^X holds throughout and the `CR0.WP` blast radius does not exist. Needs
     nothing but two Limine responses, so it works arbitrarily early — the
     patcher is free to run before *any* memory subsystem is up, which is what
     lets it precede `smep_smap::init()`. It is also what Linux's `text_poke`
     does, minus the temporary-mm machinery we do not need with one CPU
     running.
   - *Against:* depends on the bootloader answering two requests, and on the
     kernel image being physically contiguous (Limine guarantees this). Both
     are checked at runtime; failure logs and patches nothing.

**Consequence for the SMAP gate.** Because patching can now fail for reasons
other than "the CPU lacks the feature" (no HHDM, no executable-address
response, a malformed table), `has_run()` is too weak a signal to gate SMAP on:
the patcher could run to completion having installed nothing, and a caller
checking only "did it run?" would enable SMAP with no `clac` anywhere. So
`apply()` also exports `all_supported_sites_patched()`, set only if every site
whose feature is present was actually rewritten, and `smep_smap` gates on that.
This is the same fail-closed principle as §122.

**On x86 cache coherency.** Writing via one mapping and executing via another
needs no explicit flush: caches and the instruction-fetch unit are coherent
over *physical* addresses. The `cpuid` serialization after the patch loop is
still required, but for the separate reason that a stale prefetch of the
rewritten bytes may already be in flight (SDM Vol. 3A §8.1.3).

**How to reverse.** The alias arithmetic is confined to `TextAlias` in
`alternatives.rs`; swapping in the `CR0.WP` approach means replacing
`TextAlias::writable()` with the identity and bracketing the loop. Do not — it
would reintroduce the W^X window this decision exists to avoid.
