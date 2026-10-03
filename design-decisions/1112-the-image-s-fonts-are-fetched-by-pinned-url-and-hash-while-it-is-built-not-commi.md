## 1112. The image's fonts are fetched by pinned URL and hash while it is built, not committed

**Date:** 2026-09-26
**Lane:** D
**Decided by:** Claude (autonomous; lane F's request left the choice to lane D)

**In short:** the OS image now carries fonts -- Open Sans, Noto Sans,
JetBrains Mono and Noto Color Emoji, 11.5 MB -- so the desktop can draw real
text instead of its 8x16 bitmap face. The files are not kept in git. The
image build downloads each from a fixed address, checks it against a pinned
fingerprint (its SHA-256, a hash that changes if one byte does), and keeps it
in a cache, so each machine downloads them once. The price is that the first
image build on a machine needs the network.

| Choice | For | Against |
|---|---|---|
| **Fetch by pinned URL and SHA-256, cached by hash (chosen)** | the binaries never enter git's history; a file that is not the pinned one is refused however it arrived; offline after the first build | a fresh machine's first build needs the network; if an upstream URL goes away, a fresh machine cannot build until the pin moves |
| Commit the files | builds anywhere, offline, always | 11.5 MB in every clone's history for good, and more with each font added -- the CJK faces alone are tens of MB |
| Fetch the latest, unpinned | always current | the image changes whenever upstream does, silently, and nothing checks what arrived |

When a file cannot be had: a hash mismatch is always fatal. A failed fetch
is fatal too, unless `SLATEOS_ROOTFS_NO_FONTS=1` asks for an image without
fonts -- because no boot test would notice one (it still boots, in the bitmap
face), so building one has to be a choice, not a fallback. The pins live in
`scripts/create-ext4-rootfs.sh`, at `google/fonts` commit `23e54b51` and
`noto-emoji` tag `v2026-09-24-unicode18_0`; the cache is
`~/.cache/slateos/fonts` inside WSL. Revisit if the OS grows a package
mechanism that could carry fonts as packages.
