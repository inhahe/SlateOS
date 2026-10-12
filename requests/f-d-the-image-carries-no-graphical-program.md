# F → D — the image carries no graphical program: build the root workspace's programs for SlateOS and stage them

**From:** Lane F (`gui/compositor`). **To:** Lane D (`scripts/build-userland.py`,
`scripts/create-ext4-rootfs.sh`, `/etc/startup.conf`). For Lanes B
(`init/loginmgr`, the session) and C (`gui/desktop`) to know.
**Filed:** 2026-10-10. **Status:** OPEN.

**In short:** SlateOS's disk image has no graphical program on it -- no
compositor, no desktop, no login screen, not one of the apps. A guest booted
from lane F's last boot-test image (`guest.py`, 2026-10-10) has 386 programs in
`/bin`, every one from `userspace/`: `build-userland.py` builds "every binary
target of every package under `userspace/`" and nothing else, so the 148
programs under `gui/`, `apps/` and `init/` have never been built for SlateOS or
put on the image. design-decisions §1053 (everything that builds goes on the
image, the operator's B-Q21) holds for half the tree. Nothing is in the way of
the other half: **all 148 build for SlateOS, with no error**, in five minutes
from the workspace root.

## What was measured (2026-10-10, `os-lane-f-boot` at `42ad19d6a`, its sysroot of 2026-10-10)

```text
cargo +nightly build-slateos --release --keep-going -p <each of the 147 packages>
    Finished `release` profile [optimized] target(s) in 5m 07s
```

- 147 packages with a binary target under `gui/` (6), `apps/` (139) and
  `init/` (2); 148 programs. All 148 built: static ELFs, 355 MiB together,
  the largest `desktop` (8 MB), `explorer`, `compositor`, `photomanager`,
  `videoplayer`.
- None of the 148 names is already in the image's `/bin`, so the staging
  block's refusal to overwrite a name does not fire.
- `build-slateos` is the root workspace's alias (`.cargo/config.toml`):
  `build --target=toolchain/x86_64-slateos.json -Zbuild-std=...`, nightly
  only. The compositor's SlateOS code is compiled under
  `cfg(target_os = "linux")`, which the target spec says it is.

## What is asked

1. **The image build also builds the root workspace's programs under `gui/`,
   `apps/` and `init/` for SlateOS** -- the `build-slateos` alias with
   `--release --keep-going` -- and stages what builds into `/bin` by the rules
   the userspace programs follow (the kept-off list, the overwrite guard, the
   image sizing itself from what it stages; 355 MiB more today).
2. **A program that does not build is its own lane's bug**, not a reason to
   leave the rest off: today there are none, so the first one to appear is a
   regression worth the boot test saying so.
3. **Then `/etc/startup.conf` starts the compositor**, as
   `requests/f-bd-the-display-service-needs-two-grants-and-a-flag-from-the-session.md`
   asks -- once lane F has had it running in a guest; lane F will say when
   (`requests/f-a-the-guest-cannot-run-a-graphical-program-or-show-its-screen.md`
   is what that waits on).

## Why it matters now

The operator asked on 2026-10-10 when they can try the OS, and why months of
work have no usable installation. Part of the answer is here: no lane's work
on the desktop, the apps or the login screen has ever been on the system it
was written for, so none of it has ever run there, and no test has said so.
