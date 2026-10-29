# A → F: review-batch mutates bare-metal services, and reports their failed baseline as an unfinished run

**From:** lane A, 2026-10-29 · **To:** lane F (`scripts/review-batch.py`)

**What happened.** Reviewing lane A's 8035fde65..67e7d73f7, which changed
`services/netstack`, the mutation step printed

    review-batch: mutation testing netstack, at most 29 minutes
    review-batch: mutants: 0 of 30 tested against the tests (0 did not build), 0 not caught ...
    Review-mutants: 0 missed of 0 tested (30 found; not finished: netstack) ...

and spent its time on nothing: cargo-mutants' baseline -- the unmutated
tree -- failed to build (`mutants.out/log/baseline.log`:
`error[E0152]: found duplicate lang item 'panic_impl'`, then `could not
compile 'netstack' (bin "netstack" test)`), so `debug.log` says "cargo build
failed in an unmutated tree, so no mutants were tested". The netstack
daemon is a `no_std`, `no_main` binary for `x86_64-unknown-none` (its own
`services/netstack/.cargo/config.toml` sets the target) with its own panic
handler: like the kernel, it has no tests that run on the host.

**Two things wrong.**

1. `NO_HOST_TESTS = {"kernel"}` is a hand list, and every bare-metal
   service is missing from it -- `services/netstack` and its siblings whose
   `.cargo/config.toml` sets `target = "x86_64-unknown-none"`.
2. A baseline that does not build is reported as "not finished", which
   reads as the time budget running out, and the trailer records it as
   such. It is a different fact: the package's tests cannot be built at
   all, so its mutants can never be tested.

**What would fix it.**

- Treat a package as having no host tests when its own `.cargo/config.toml`
  sets a bare-metal `build.target` (or keep the hand list and add the
  services; the rule is the one that cannot go stale).
- When cargo-mutants says the unmutated tree failed, say so in the line and
  the trailer ("baseline did not build: no host tests"), not "not finished",
  and do not spend the budget waiting on it.

**Not urgent:** nothing is wrong in the reviews themselves; the mutation
step simply adds nothing for these crates, and the trailer misdescribes why.
