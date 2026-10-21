# A → C, D: the project's own LICENSE at the root is taken for a vendored crate, and the image lacks it

**Status:** OPEN · **From:** lane A · **To:** lane C (`scripts/gather-notices.py`),
lane D (`scripts/create-ext4-rootfs.sh`, the staging) · **Filed:** 2026-10-21 ·
**Priority:** blocks the operator's licence commit, which waits in the integration
tree until item 1 is on `main`

## In short

The operator is adding a `LICENSE` (MIT) at the repository root, with a README
section saying SlateOS's own code is MIT and the ported components keep their
own licences. The commit is ready (`5747cbe60` in `C:\visual studio projects\os`,
not pushed) and cannot be published yet: with a `LICENSE` beside the root
`Cargo.toml`, `gather_tree` takes the repository root for a vendored crate,
and crashes on it. `test-gather-notices.py` then fails, so every lane's boot
test would go red, and `create-ext4-rootfs.sh` would stop at the notices step.

## What happens

`gather_tree` treats any directory holding both `Cargo.toml` and a file matching
`LICENCE_FILE` as a vendored crate. The root qualifies, and its `Cargo.toml` is a
virtual workspace (`[workspace]`, no `[package]`), so `_crate_notice` raises:

    the tree's notices do not gather: ./Cargo.toml: Cargo.toml gives no plain name and version

Reproduce in any worktree: copy a `LICENSE` to its root and run
`python scripts/test-gather-notices.py` (check 2 fails).

## Asked

1. **Lane C:** `gather_tree` must not take the workspace root for a vendored
   crate. Skipping a `Cargo.toml` without a `[package]` table would be the
   general rule; skipping the root alone is the narrow one. A test with a
   `LICENSE` at the fixture root, please, so this cannot come back.
2. **Lanes C and D:** SlateOS's own notice in the image. MIT requires "the
   above copyright notice and this permission notice" in all copies, and an
   image is one, but the notices pipeline gathers only third-party notices.
   The root `LICENSE` should reach `/usr/share/licenses` as a first-party entry
   (gather it as one, lane C; stage it, lane D) and show on the notices screen.

Please tell lane A when item 1 is on `main` (a notice is enough): lane A then
rebases the operator's commit onto it, has it reviewed and publishes it. Item 2
can follow, since until the `LICENSE` is published there is nothing to stage.
