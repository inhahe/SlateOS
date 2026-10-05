# C -> D: put the third-party notices in the image

**From:** Lane C. **To:** Lane D (the rootfs recipe). **Filed:** 2026-09-28.
**Status:** ✅ DONE 2026-10-01 by lane D -- reply at the end.
**Decision behind it:** `design-decisions.md` §1433.

**In short:** A SlateOS image may only be handed to anyone if it carries the
licence notices of the code other people wrote. That code includes the
crates.io libraries, the vendored RustCrypto crates, and the ported JPEG,
TIFF, WebP and AVIF decoders. Today no image carries them. Lane C has written
a script that gathers every notice the source tree holds into one folder. It
needs to run when the image is built, into `/usr/share/licenses`.

## What is asked

In the rootfs recipe (`scripts/create-ext4-rootfs.sh`, or wherever the image's
files are staged), run:

```sh
python scripts/gather-notices.py --out "$STAGING/usr/share/licenses"
```

and **fail the image build if it exits non-zero**. A non-zero exit means one
of these:
- a notice could not be gathered, such as a manifest naming a text that is not
  there;
- a crates.io package is missing from cargo's registry cache (a build of the
  workspace fetches it).

An image missing one notice is the failure this exists to prevent, so a
warning would not do.

Notes for fitting it in:
- **`--out` must be a directory that does not exist or is empty.** The bundle
  is written whole, never merged into an old one. A fresh staging directory
  satisfies that; reusing a staging tree needs the old `usr/share/licenses`
  removed first.
- It takes about 2 s. It reads the source tree, `Cargo.lock` and cargo's
  registry cache (`$CARGO_HOME`, else `~/.cargo`). It writes only into
  `--out` and starts no process.
- **What lands in the image:**
  - `index.yaml`, the list that `gui/notices` reads (`notices::SYSTEM_DIR` is
    `/usr/share/licenses`);
  - one directory of licence texts per component;
  - `NOTICES.txt`, everything in one file for a person without a screen.

  About 35 components today, a few hundred KB.
- `python scripts/gather-notices.py --check` does everything except write,
  for a dry run. `--list` prints one line per component.

## Why the image, not a compiled-in list

The notices have to cover everything the image carries. No single program
links everything: the video decoder lives in the video player, and the
crates.io libraries sit wherever they are used. §1433 records the
alternatives: lane F's per-crate constants, and a build script embedding the
list. It also records why neither of those covers the system.

## What happens if it waits

Nothing breaks. The screen that will show the notices
(`requests/c-e-show-the-third-party-notices.md`) says they are not installed
rather than showing an empty list. Only a published image lacks the notices
its licences require, so this matters before the first image is published
anywhere.

## Lane D — done, 2026-10-01

`scripts/create-ext4-rootfs.sh` runs `scripts/gather-notices.py --out
"$STAGE/usr/share/licenses"` after the themes. The image build fails if it
exits non-zero, or if there is no Python to run it with. A run today stages
the notices of 96 components in 163 files: 29 from manifests, 25 vendored
crates, 42 from crates.io. Three things beyond the call:

- **The registry is Windows cargo's.** The recipe runs under WSL, and the
  workspace is built by Windows cargo. Against WSL's own `~/.cargo` the
  gather failed for want of packages; against the Windows one it passed. So
  under WSL the recipe asks `cmd.exe` for `%CARGO_HOME%`, else
  `%USERPROFILE%\.cargo`, and converts the answer with `wslpath`.
  `SLATEOS_CARGO_HOME` overrides the search; without WSL's interop it is
  cargo's own home. Reading the tree and the cache through WSL takes about
  12 s, where your figure for Windows is 2 s.
- **Modes are set, as for the themes**: files 0644, directories 0755.
- **Nothing is removed first.** The staging tree is a fresh `mktemp -d`, so
  `--out` is always new. If a step above ever put something in
  `/usr/share/licenses`, the gather would refuse rather than lose it.

`scripts/test-rootfs-staging.sh` cases 28 to 31 cover it with a stand-in
gatherer: the call, the registry both ways, the modes, the count, and both
failures. It reaches `main` with lane D's next publish.
