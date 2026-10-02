### [A] RESOLVED — a KASAN self-test left a freed heap slot permanently poisoned, so 62 of the instrumented build's 64 report slots were spent on false use-after-frees and the real report budget was exhausted two thirds of the way through the boot — 2026-08-19

**Id:** `B-KASAN-STALE-POISON-ON-LIVE-SLOT`

**In short:** KASAN keeps a side table ("shadow") with one byte per 8 bytes of
memory, marking each 8-byte chunk as usable or not. Freeing marks a chunk
"freed"; allocating marks it "usable" again. Both of those updates were skipped
whenever KASAN was switched off — but the *switching off* did not clean up the
marks already written. A boot self-test switched KASAN on, freed a 64-byte block
(marking it "freed"), and switched KASAN off again, leaving that block sitting
on the allocator's free list still wearing the "freed" mark. Whatever allocated
it next got no mark update, because KASAN was off. From then on the block was in
active use while permanently labelled freed, and every ordinary read or write to
it was reported as a use-after-free. There were 64 such reports on one
instrumented boot, and after 64 the kernel stops reporting — so every *genuine*
memory-corruption report in the last 1600 lines of a 27701-line boot was
silently dropped, in the one build profile that exists to catch them.

#### How it presented

The instrumented boot that closed
`B-KASAN-INSTRUMENTED-BOOT-WEDGES-MID-PRINT-ON-A-PAGE-FAULT` reached `BOOT_OK`
in 951 s and looked clean. It was not clean:

| Observation | Value |
|---|---|
| `[kasan] CRITICAL: use-after-free` reports | 64 |
| ...fired *after* the KASAN self-tests ended (line 25623) | 62 |
| Distinct addresses across all 64 reports | one 20-byte span, `0xffff80007eb072c0`-`...72d3` |
| Report budget exhausted at | line 26035 of 27701 |
| Boot lines left unprotected after that | ~1666 |

Reports on a *single* span, interleaved with unrelated `[quarantine]`,
`[watermark]` and `[ipv6]` self-tests, is not the signature of a corruption bug;
it is the signature of one piece of memory that the checker is permanently wrong
about.

#### The mechanism

`on_alloc` and `on_free` (`kernel/src/mm/kasan.rs`) both open with
`if !is_enabled() { return; }`. That pair of gates is only sound while the shadow
is guaranteed clean whenever KASAN is off, and the two directions do not fail the
same way:

| Sequence | `on_free` poisons? | `on_alloc` unpoisons? | Result |
|---|---|---|---|
| freed off, reallocated on | no | yes | clean |
| freed on, reallocated on | yes | yes | correct |
| **freed on, reallocated off** | **yes** | **no** | **`0xFA` over live memory, forever** |

`kasan::self_test_freed_address()` performed the third row *by design*: it
enabled KASAN, allocated 64 bytes, freed them (so the shadow read `KASAN_FREE`),
and returned the address — because its caller, `kasan_rt::self_test`, needed a
known-bad address to drive the report path with. It then restored KASAN to
disabled and returned, leaving the poisoned slot on the free list.

The instrumented build is what turns that into visible damage.
`kasan::shadow_allows` is deliberately **not** gated on `is_enabled()` — it backs
the compiler-emitted `__asan_load*`/`__asan_store*` calls, which LLVM emits
unconditionally — so the poison is consulted on every access to that memory even
though KASAN is nominally off.

#### Why nothing caught it

- **The self-tests all passed**, because each one asserts about the shadow state
  it just created. None asserts anything about the state it *leaves behind*.
- **The uninstrumented build is immune in practice.** With KASAN off nothing
  calls `check()`, so the stale poison sits there inert. The bug is only
  observable in the profile that is run rarely and read least carefully.
- **The verification that declared the boot clean could not have failed.** It
  grepped for `BUG: KASAN` and `__asan_report`, neither of which appears
  anywhere in `kernel/src/`. The kernel prints `[kasan] CRITICAL:`
  (`kasan.rs`). This is the same "matcher that can never fire" hazard
  `scripts/boot-history.py`'s own docstring warns about; see the correction
  logged below.

#### The fix

Three changes in `kernel/src/mm/kasan.rs` and one in `kernel/src/mm/kasan_rt.rs`:

1. **`disable()` now wipes.** It clears `ENABLED` first (so no new poison can
   start), then resets every *mapped* shadow frame to `KASAN_ADDRESSABLE` via a
   new `clear_all_poison()`, iterating the existing `SHADOW_MAPPED` bitmap so the
   cost is proportional to the heap actually touched, not to the 8 GiB shadow
   reservation. This restores the invariant the two gates depend on — *poison
   exists only while enabled* — **by construction**, rather than requiring every
   present and future enable/disable window to remember to clean up. The
   direction is the safe one: a wiped shadow reads addressable everywhere, so the
   worst case is a missed report, never a false one.
   - The fill uses `mm::rawmem::fill_u8`, not `core::ptr::write_bytes`: the
     `core` intrinsic is subject to LLVM's memory-intrinsic instrumentation, and
     a shadow write that itself takes a shadow check underneath the sanitizer is
     the recursion this module exists outside of.
   - Unmapped shadow is left alone. It resolves to the single read-only zero page
     aliased across the whole reservation; one store there would appear as poison
     at millions of unrelated addresses, which is exactly why it is read-only.
2. **`self_test_freed_address()` becomes `with_self_test_freed_address(|addr| ...)`.**
   The old signature *was* the bug: it handed out an address naming a slot that
   was back on the free list while still poisoned. Scoping it to a closure makes
   the cleanup structural, and the cleanup is the state restore that already
   existed — if KASAN was on, the poison is correct and `on_alloc` will clear it
   at reuse; if KASAN was off, `disable()` wipes on the way out. No future caller
   can reintroduce the leak by forgetting a step that no longer exists.
3. **`with_map_lock`'s doc comment corrected.** It claimed a lock give-up "is
   never a correctness problem — an unpoisoned shadow byte reads as
   'addressable' and fails open." True of the poison direction; **false** of the
   unpoison direction, which fails *closed* and is precisely how a live slot ends
   up permanently marked freed.
4. **Regression test** — `kasan::self_test` Test 6 performs the exact failing
   sequence (KASAN on, alloc, free, assert poisoned, `disable()`) and asserts
   the shadow is addressable afterwards, plus `shadow_allows(a, 64)` so a wipe
   that cleared one byte but missed the rest of the slot still fails. It asserts
   about the *shadow byte*, not about "did a report fire", so it holds in the
   ordinary build too — the uninstrumented build is where the poison is planted
   and where the wipe either happens or does not.

#### The lesson that generalises

**An enable/disable flag that gates a state *mutation* must also own the state's
teardown.** Gating both halves of a poison/unpoison pair on the same flag looks
symmetric and is not: the flag can be cleared between the two halves, and only
one of the two failure directions is safe. Any time a flag gates "write X" and
"undo X" separately, ask what happens when it flips in between — and if the
answer is bad in one direction only, the flag's `off` transition, not the
individual hooks, is where the fix belongs.
