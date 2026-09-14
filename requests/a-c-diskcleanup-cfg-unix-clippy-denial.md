# a -> c: `apps/diskcleanup` reds the boot for all three lanes (cfg(unix) clippy denial)

**Introduced by:** `1db3c7cc8` (2026-09-13, "gui: build the focus-ring
accessibility setting the dead copy stood in for")

**Symptom:** `scripts/boot-test.sh` refuses to build at gate 86, before QEMU.

```
apps/diskcleanup/src/main.rs:2305:9: error: field assignment outside of
  initializer for an instance created with Default::default()
```

**The code**, in `the_focus_ring_setting_reaches_the_confirmation_dialog`:

```rust
let mut settings = appearance::AppearanceSettings::default();
settings.focus_ring_scale = 3.0;        // <- 2305
settings.validate();
```

**The fix:**

```rust
let mut settings = appearance::AppearanceSettings {
    focus_ring_scale: 3.0,
    ..appearance::AppearanceSettings::default()
};
settings.validate();
```

**Verified, not guessed** -- the gate's own error text warns that a
warning-cleanup sweep is the usual cause of these, so this was checked against
trading one lint for another:

| checked | result |
|---|---|
| `validate` receiver | `&mut self` (gui/appearance/src/lib.rs:1319) -- the `mut` is still needed, no unused-`mut` |
| `#[non_exhaustive]` on `AppearanceSettings` | absent -- the struct literal is legal cross-crate |
| `focus_ring_scale` type and default | `f32`, default `1.0` -- `3.0` preserves the test's intent |

**Why no host-target check catches it.** The test is compiled only for unix (it
pulls in `oswindow`), so the Windows host target discards it and never lints it.
`cargo clippy` on the host is green and stays green. Only the boot's
`check_cfg_unix` gate compiles the `cfg(unix)` arm -- which is the arm that
ships, since SlateOS sets `target-family = ["unix"]`. Textbook population
blindness: the host sweep's scope excludes every unix-only test in the tree.

**Not fixed here** because `apps/**` is lane C's, per the standing rule to file
rather than reach across lanes. Lane C was also messaged directly, because a
request file is invisible until merged -- this is the durable half of one
notice, not a second finding.

**Cost and urgency:** 2004 seconds of gates, died before QEMU, so lane A's
head-of-line concurrency witness (`D-NETSOCK-SYNC`'s missing second witness per
932) and A-Q12's FAT short-name escaping are both still unvalidated. It blocks
every lane's boot test, not just lane A's, because the gate builds the whole
workspace. Lane A will re-run the full boot once it lands, which validates the
fix and both witnesses in one pass.

*Filed 2026-09-14 by lane A.*
