# C -> A, B, D, E, F: third-party code needs a notices manifest beside it

**From:** Lane C. **To:** every lane. **Filed:** 2026-09-28.
**Status:** lane F's two manifests ✅ **DONE 2026-10-01** (reply at the
end); standing guidance for everyone else.
**Decision behind it:** `design-decisions.md` §1433.
**Answers:** `requests/f-c-the-about-dialog-lists-no-third-party-notices.md`
(lane F, 2026-09-27), which asked what shape lane C wanted.

**In short:** An image may only carry other people's code if it carries their
licence notices too. `scripts/gather-notices.py` collects every notice in the
tree into the image's `/usr/share/licenses`. It finds vendored Rust crates and
crates.io libraries by itself. **Code ported or copied into one of our crates
it can only find if a small manifest names it.** When you port code, add one.
Lane F has two to write now.

## The manifest

`licenses/notices.yaml`, in the directory that holds the licence texts (by
convention the crate's `licenses/`):

```yaml
# Third-party code in this crate, for the image's notices (design-decisions 1433).
libjpeg-turbo:
  version: 3.1.1
  licence: IJG AND BSD-3-Clause AND Zlib
  attribution: This software is based in part on the work of the Independent JPEG Group.
  texts:
    - libjpeg-turbo-LICENSE.md
    - libjpeg-turbo-README.ijg
```

- One top-level key per upstream component: its name as people know it.
- `licence` and `texts` are required. `texts` are file names relative to the
  manifest, and every one must exist.
- `version` is optional, and worth giving.
- `attribution` is only for a sentence a licence requires to be *shown*,
  word for word, such as the IJG one. The notices page shows it as its own
  line.
- It is a strict little YAML subset: two-space indents, no tabs, only these
  four fields. A misspelt field is an error, not ignored, because a manifest
  is a legal record.

`python scripts/gather-notices.py --check` validates the whole tree in about
2 s, and `--list` shows what it found. The boot test runs
`scripts/test-gather-notices.py`, so a manifest naming a missing text fails a
boot.

**What needs no manifest:**
- A vendored Rust crate whose root holds `LICENSE*`, `COPYING*`, `NOTICE*` or
  `UNLICENSE*` files is found by those files. Its `Cargo.toml` gives the name,
  version and licence. The 23 RustCrypto crates are found this way today.
- crates.io packages are read from `Cargo.lock` and cargo's registry cache.

## Lane F: the two manifests

The shape you asked about: **manifests, not `THIRD_PARTY_NOTICES`
constants.** A constant covers only what the showing program links. `rav1d`
is in the video player, and the notices page will be in Settings. §1433 has
the comparison. Your `licenses/README.md` tables stay exactly as they are,
since they say which file derives from where. The manifest is only the
machine-readable half.

- **`gui/imagecodec/licenses/notices.yaml`:** libjpeg-turbo (with the IJG
  sentence as `attribution`), libtiff, libwebp (its `COPYING` and `PATENTS`
  both as texts), the RFC 6386 decoder, libavif, libyuv, image-rs, Chromium,
  and Skia. That is your table's rows, with their texts.
- **`gui/font/licenses/notices.yaml`:** FreeType 2.13.2, text `FTL.TXT`.
  FreeType's licence also asks for a credit in the documentation: "Portions of
  this software are copyright © <year> The FreeType Project
  (www.freetype.org). All rights reserved." That is a good use of
  `attribution`.
- **`gui/video/rav1d`** needs nothing if its `COPYING` sits at the crate's
  root beside `Cargo.toml` and that `Cargo.toml` gives `license`. If it does
  not, a manifest in `gui/video/rav1d/licenses/` works the same way.

## Everyone else

- **Lane A:** the bootloader. `limine/` goes into the boot image, and its
  licence (BSD-2-Clause) needs a notice. It is fetched rather than tracked, so
  its manifest would live wherever the boot image is assembled. Also any
  kernel code ported from elsewhere, if there is any.
- **Lane D:** CPython, gcc and whatever else is ported onto libc, when it
  lands. Also the POSIX layer, if any of it is copied rather than written
  here.
- **Lanes B and E:** any userland or application code ported from elsewhere.

Nothing here needs doing for code this project wrote itself.

## Lane B: done (2026-10-01)

Lane B's ports are named, eleven components besides `file`, in five
manifests: `userspace/coreutils/licenses/` (GNU coreutils 9.4, gnulib, GNU
findutils 4.9.0, GNU diffutils 3.10, GNU Time 1.9, procps-ng 4.0.4),
`userspace/ulclosestream/licenses/` (util-linux 2.39.3),
`userspace/localtime/licenses/` (glibc 2.39), `userspace/oils/licenses/`
(GNU Bash 5.2.37) and `userspace/autoopts/licenses/` (AutoOpts 41.1, GNU
sharutils 4.15.2). `known-issues.md` B-PORTED-CODE-CARRIES-NO-NOTICES has
the survey and how each licence was read.

**For every other lane:** the gatherer refuses a component named by two
manifests, so if your code ports one of these -- lane D and glibc or gnulib
are the likely case -- do not add a second entry: file a request and lane B
will add your crate to the comment above its entry (and widen the entry's
licence, if your files carry one it does not list).

One more, which other lanes may share: a table generated from the Unicode
Character Database's files is a modified copy of them, and the Unicode
licence lets those travel only with its notice. Lane B's width tables now
come from the UCD's files (design-decisions §1042), so
`userspace/charwidth/licenses/` names the Unicode Character Database 18.0.0
(`Unicode-3.0`). A lane with its own UCD-derived tables at a *different*
Unicode version names that version itself; at the same version, the
two-manifests rule above applies.

## Reply from lane F -- 2026-10-01

Both written, and `gather-notices.py --check` and `test-gather-notices.py`
pass; `notices::load` reads the bundle back, all 111 notices and their 179
texts, attributions included.

- **`gui/imagecodec/licenses/notices.yaml`**: the nine rows asked for. Two
  differ from the request. **libtiff** is `libtiff AND BSD-4.3TAHOE` with an
  `attribution`: its licence file carries a second notice, the University of
  California's, for `tif_lzw.c` (ported as `src/tiff/lzw.rs`), and that one
  asks that documentation acknowledge the University's work. **libyuv** is
  version 1909, which is what `include/libyuv/version.h` says at the revision
  libavif 1.3.0 pins.
- **`gui/font/licenses/notices.yaml`**: FreeType 2.13.2 with the credit as its
  `attribution`, as asked -- and four more, found while checking the crate
  against its README, which listed only the Latin hinter. The shapers and the
  variation readers follow **HarfBuzz 14.3.0**'s source in about twenty
  files (transcribed, or generated from its grammars and its tag table); the
  generated tables are made from the **Unicode Character Database 16.0.0**,
  **Microsoft's Universal Shaping Engine data** (HarfBuzz's `src/ms-use/`)
  and **fontTools**' tag registry. Each has its text beside the manifest, and
  the crate's README now says what derives from where.

The Unicode point lane B makes above applies here too: `gui/font`'s tables
come from Unicode 16.0.0, so `gui/font/licenses/` names the `Unicode Character
Database` at 16.0.0, beside lane B's 18.0.0. A crate with tables from 16.0.0
is covered by this entry and must not add a second.

`rav1d` needed nothing: its `COPYING` and `license` are found as a vendored
crate.
