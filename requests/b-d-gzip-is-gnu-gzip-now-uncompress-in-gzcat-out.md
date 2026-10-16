# B -> D: `gzip` is GNU gzip now -- install `uncompress`, drop `gzcat`

**From:** Lane B (`userspace/gzip`). **To:** Lane D
(`scripts/rootfs-bin-manifest.txt`). **Filed:** 2026-10-16. **Status:**
OPEN -- small; until it is done `gzcat` on the image compresses instead
of decompressing.

**In short:** `userspace/gzip` was written from scratch; it is now GNU
gzip 1.12, ported, and held to Ubuntu 24.04's byte for byte -- its `.gz`
files included -- by `scripts/gzip-diff.sh`. Ubuntu's `gunzip`, `zcat`
and `uncompress` are shell scripts that run `gzip -d` and `gzip -cd`; the
port answers to those three names itself and does what the scripts do,
their own `--help` and `--version` included. `gzcat` is not one of them:
Ubuntu has no `gzcat`, and GNU gzip, built as Ubuntu builds it, ignores
the name it was run by -- so `gzcat`, installed as `gzip`, is plain
`gzip` now and compresses what it is given.

## What it asks

In `scripts/rootfs-bin-manifest.txt`:

| line now | becomes |
|---|---|
| `gunzip = gzip` | unchanged |
| `zcat = gzip` | unchanged |
| `gzcat = gzip` | removed |
| -- | `uncompress = gzip` (new) |

Nothing else changes: the binary is still `gzip`, from the `gzip` crate.

## Not asked

Ubuntu's gzip package also installs `zcmp`, `zdiff`, `zgrep`, `zegrep`,
`zfgrep`, `zless`, `zmore`, `znew`, `gzexe` and `zforce`. Each is a shell
script, not a program, and none is ported or staged; when the image wants
them, they would be staged as the scripts they are (gzip 1.12's `*.in`
files, with Ubuntu's two CVE fixes to `gzexe` and `zdiff`).
