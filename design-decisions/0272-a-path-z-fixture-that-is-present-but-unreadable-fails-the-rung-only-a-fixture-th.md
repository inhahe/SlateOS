## 272. A Path-Z fixture that is present but unreadable fails the rung; only a fixture that is genuinely absent may skip it

**Date:** 2026-08-24
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** The kernel's boot self-tests include ~84 "rungs" that each run a
small program off a test disk. Any of them may legitimately be missing — the
disk is a build artifact, and a fresh checkout has no disk at all — so a rung
whose program is missing is allowed to sit out. Until now it sat out for *any*
reason the file failed to load, including "the filesystem returned an I/O
error". That is the wrong grouping: a broken filesystem would have switched
off every rung that could have detected it, and the boot would have finished
looking clean. This entry records the split — absent means skip, unreadable
means fail — and the cost of getting it wrong in the other direction.

### The code that made the decision necessary

```rust
match crate::fs::Vfs::read_file(&path) {
    Ok(v) if !v.is_empty() => Some(v),
    _ => None,
}
```

Four outcomes collapse onto that `_`:

| Read outcome | What it means |
|---|---|
| `Err(NotFound)`, nothing mounted at `/mnt` | lean build, no test disk |
| `Err(NotFound)`, `/mnt` mounted | image built without this fixture |
| `Err(_)` — EIO, permission, corrupt directory | **the VFS or ext4 is broken** |
| `Ok(v)` with `v.is_empty()` | the image build produced a truncated file |

Every caller then read `None` as "absent" and returned `Ok(())`. The first two
rows are environment facts and skipping is right. The last two are defects,
and they are defects *in the subsystems these very rungs exercise*.

### The decision

`pathz_test_elf` / `pathz_command` return `KernelResult<Option<Vec<u8>>>`:
`Ok(Some(v))` run, `Ok(None)` absent-and-counted, `Err(_)` fail the rung.
Absence is established by asking whether the mount point exists *first*, and
then only by `NotFound`; every other error is a defect and reddens the boot.

### The tradeoff, stated honestly

**Against:** this converts a class of transient environment problem into a
failed boot test. A flaky host disk, a half-written `rootfs.ext4`, an ext4
quirk under memory pressure — any of these now fails a run that would
previously have gone green with a few extra SKIP lines. Boot tests take
8–25 minutes, so a false red is not free.

**For:** the alternative is a suite that goes quiet exactly when it has
something to say. That is not a hypothetical failure mode here; it is the
recorded history of this file. `B-PATHZ-PREREQUISITE-SKIPS-ARE-SILENT` is the
same shape (26 tcc rungs no-op'd unnoticed when `/bin/tcc` left the image),
and the fix for it — `pathz_skip` and the end-of-boot verdict — was itself
bypassed by these 84 sites, which printed their own SKIP line in a format the
verdict does not parse and incremented nothing. So a boot that lost 29 rungs
could still print `Path-Z prerequisites: complete — 0 rungs skipped`.

The asymmetry decides it. A false red costs one re-run and is
self-announcing. A false green costs however long it takes someone to notice
that a subsystem has had no test coverage — and by construction, nothing will
announce it. §270's rule ("a skip must be at least as loud as a failure")
already implies this; §272 is what it looks like when the skip decision is a
*read error* rather than a capability check.

### What was explicitly not done

- **Retrying the read.** Tempting for the transient case, but it converts a
  deterministic failure into a flaky one and hides the very signal being
  bought. If reads are unreliable, that is the finding.
- **A "lenient" mode for lean builds.** The mount-point check already covers
  the only legitimate lean case, and a mode flag would be a switch someone
  eventually leaves on.
- **Skipping on `Ok(v)` with `v.is_empty()`.** A zero-byte fixture is not an
  absent one; it is a build that produced a file and got it wrong, which is
  worth a red line rather than a quiet one.
