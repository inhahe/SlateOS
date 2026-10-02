## 1433. Third-party notices are gathered from the tree into the image, not compiled into one program

**Date:** 2026-09-28 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C,
with D (the image), E (the page that shows them) and every lane that ports
code &middot; prompted by lane F,
`requests/f-c-the-about-dialog-lists-no-third-party-notices.md`

**In short:** Code copied or translated from other people's projects may only
be shipped if their licence notices ship with it. Nothing in a SlateOS image
carried them. Now every piece of third-party code in the source tree is listed
in a small file beside its licence texts. A script gathers all of them, plus
the crates.io libraries and the vendored Rust crates, into
`/usr/share/licenses/` when the image is built. The screen that lists them
reads that folder. The notices are gathered once, for the whole image, at the
moment the image is made. No program has to embed them.

**What is gathered, from where:**

| Source | How it is found | What names it |
|---|---|---|
| Ported code (libjpeg-turbo, FreeType's auto-hinter, ...) | a `licenses/notices.yaml` manifest anywhere in the tree | the manifest, written by whoever ports the code |
| Vendored Rust crates (`rustcrypto/*`, `rav1d`) | a Cargo package whose root holds `LICENSE*`, `COPYING*`, `NOTICE*` or `UNLICENSE*` files -- this project's own crates carry none | its `Cargo.toml` (`name`, `version`, `license`) |
| crates.io libraries | every registry package in `Cargo.lock` | its own `Cargo.toml` and licence files, read from cargo's registry cache, which a build has already filled |

The gathered folder holds `index.yaml` (one entry per component: name,
version, licence, any sentence the licence requires to be shown, where in the
tree it came from, and its text files), the texts themselves, and
`NOTICES.txt`, everything in one file for a person reading it without the
screen. `gui/notices` reads the index. `scripts/gather-notices.py` writes it.
`scripts/test-gather-notices.py` keeps every boot honest, since the boot test
runs every tooling suite: every manifest must parse, every text it names must
exist, and every vendored and registry crate must have its texts.

**The alternatives:**

| Option | For | Against |
|---|---|---|
| **Gather into the image at build time** (chosen) | covers everything the image holds, whichever program links it; fresh on every image; a crate adds its notice in its own directory, so no lane edits another's file; the texts are plain files a person can read | needs the image recipe to run the gatherer (lane D); a program that runs without the image's files shows no notices |
| Each crate exports `THIRD_PARTY_NOTICES` (`include_str!`), the showing program gathers what it links (lane F's suggestion) | the text in the binary *is* the file in the tree; no build step | covers only what the showing program links: `rav1d` is in the video player, not in the settings program, and every crates.io dependency would need a crate of ours to export it; the showing program would have to depend on everything to show everything |
| A crate whose build script walks the tree and embeds every manifest | covers the tree; no image step | cargo reruns a build script only when a file it names changes, and a *new* manifest in an existing crate is not a file it has named yet, so the list goes stale silently. Watching whole directories costs a scan of the repository on every build |

**How it could be undone:** the manifests are the lasting part, and any of
the other two designs can read them. Only the gatherer and the one reader
would change.

**What each lane is asked for:** lane D, to run the gatherer in the image
recipe; lane E, to show the notices (the shell's orphaned About dialog is a
screen you open, which §815 puts in `apps/settings`); lane F, to write
manifests for `imagecodec` and `osfont` (`rav1d` is found automatically); every
lane that ports code, to add a manifest when it does. The requests are
`requests/c-d-put-the-third-party-notices-in-the-image.md`,
`requests/c-e-show-the-third-party-notices.md` and
`requests/c-abdef-third-party-code-needs-a-notices-manifest.md`.
