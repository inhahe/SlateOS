## Verdict (2026-08-16): code layout under TCG. Not a regression.

The straddle check predicted above was run, and it lands exactly on the two
functions that drive the three benchmarks. `--compare` between the `86a923fe1`
and post-`9ecef3188` kernel ELFs reports **`recompiled: 0`** — the machine code
of these functions is byte-identical and only their *addresses* moved:

```
STRADDLE GAINED (expect these to be SLOWER): 4
  kernel::fs::encrypt::chacha20_crypt   760 B  ffffffff804fb860 -> ffffffff80507fe0
  kernel::fs::encrypt::chacha20_crypt   712 B  ffffffff804fb860 -> ffffffff80507fe0
  kernel::crypto::poly1305              616 B  ffffffff80a07770 -> ffffffff80a13ef0
  kernel::crypto::chacha20_block        344 B  ffffffff809fd7c0 -> ffffffff80a09f40
STRADDLE LOST (expect these to be FASTER): 1
  kernel::rng::ChaCha20State::generate_block  335 B  ffffffff808a2fb0 -> ffffffff808af730
```

**Five independent lines of evidence agree:**

1. **Source is byte-identical** — `git diff 86a923fe1..HEAD` on `crypto.rs` and
   `bench.rs` is empty, and the tool independently confirms it (`recompiled: 0`).
2. **The mapping is exact.** `chacha20_block` drives `crypto_chacha20_1KiB`;
   `poly1305` drives `crypto_poly1305_1KiB`; **both** gained a page crossing
   (0 → 1). `crypto_aead_1KiB` is those two composed, which is why the third
   benchmark moved and why it moved by the composed amount.
3. **The magnitude matches the documented penalty.** Measured ratios
   1.590 / 1.573 / 1.569 against the ~1.7x per-iteration cost recorded in
   `B-BENCH-TCP-CHECKSUM-PAIR-BIMODAL-1.7x`.
4. **It is binary-dependent, not host-dependent.** `86a923fe1` was rebuilt and
   re-benched on the same host *after* the step was observed and returned to
   baseline in all three: chacha20 **12045 ns**, poly1305 **5098 ns**, aead
   **19193 ns** — every one inside its historical band. So the host is not the
   variable; the image is.
5. **Benchmarks moved in both directions**, which is a layout re-roll's
   signature and not a regression's: `net_veth_recv` −25.1%, `net_arp_lookup`
   −17.3%, `crypto_ed25519_sign` −17.1%, `vfs_stat_root` −11.3%.

**A second finding, same cause.** `page_alloc_free` (+44.9%, 457 → 662 ns) is
the same artifact: `mm::frame::alloc_frame` gained a straddle on all three of
its loops (878 B, 491 B, 464 B) with `recompiled: 0`, while `free_frame` lost
one. This matters more than the crypto trio, because `page_alloc_free` is on the
performance-critical list with a **< 1 µs** target — worth stating plainly that
662 ns still meets it, and that the true cost is unchanged.

**Left unexplained, deliberately.** `cp_try_wait_empty` (+104.6%, 131 → 268 ns)
is *not* accounted for by this: `ipc::semaphore::try_wait` moved the favourable
way (1 → 0 crossings), so layout predicts it faster and it measured slower. At
131 ns the absolute movement is ~137 ns and the benchmark is near the floor
where run noise dominates, so the likeliest reading is noise — but that is a
guess, and it is recorded as unexplained rather than folded into a tidy story.

**What NOT to do.** Do not bisect `86a923fe1..9ecef3188` for these. There is no
guilty commit — the ~5,400 lines of NTFS (§210) shifted addresses, which is what
every commit does. This is precisely the trap `mode_structure()` documents:
`http_build_response_1KiB` "was bisected across three commits before anyone
asked; the answer was 'binaries', and there was no guilty commit."

**Process lesson worth more than the finding.** Two rules paid off and one cost
~40 minutes:
- Re-benching the old commit was worth it — it is the only step that separated
  binary-dependent from host-dependent, which static analysis cannot do.
- The straddle check should have run **first**: seconds, no boot, and it was
  already named as the first suggested action in the `SUSTAINED SHIFT` report.
- The near-miss to remember: this was one step away from being "fixed" as a
  comparator defect that did not exist, and one step away from a bisect for a
  commit that does not exist.
