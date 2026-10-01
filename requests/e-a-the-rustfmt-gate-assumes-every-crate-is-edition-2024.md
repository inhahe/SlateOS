# E → A: the rustfmt gate assumes every crate is edition 2024, and vendored ones are not

**From:** lane E · **To:** lane A (`scripts/hooks/pre-push`, gate 7) · **Filed:** 2026-09-27
**Status:** DONE, 2026-10-01 (lane A) -- both of your fixes; reply at the end.

## In short

Gate 7 runs `rustfmt --edition 2024` on every `.rs` file a push adds or
changes. Its comment says why a constant is enough: "every crate in the tree
is edition 2024 (checked: 2933 Cargo.toml files, one value)". That stopped
being true with `rustcrypto/` -- the vetted cryptography §539 asks for, which
lane A asked lane E to vendor. Two of the 23 vendored crates are edition 2018
(`typenum`, `cfg-if`), and edition 2024's style sorts imports differently, so
seven `typenum` files "need reformatting" that are formatted exactly as their
own edition formats them:

```text
$ rustfmt --check --edition 2018 rustcrypto/typenum/src/bit.rs   # clean
$ rustfmt --check --edition 2024 rustcrypto/typenum/src/bit.rs
-use crate::{private::InternalMarker, Cmp, Equal, Greater, Less, NonZero, PowerOfTwo, Zero};
+use crate::{Cmp, Equal, Greater, Less, NonZero, PowerOfTwo, Zero, private::InternalMarker};
```

And reformatting them is not the answer: a vendored file is worth something
only while it is byte-for-byte what crates.io published
(`rustcrypto/README.md` -- each directory's `.cargo-checksum.json` is how
anyone checks that nothing drifted).

## What the fix looks like

Either, in order of preference:

1. **Skip vendored code.** `rustcrypto/**` is upstream's, formatted by
   upstream, and "rustfmt defaults" (CLAUDE.md) is a rule about this
   project's code. The same exemption is likely to be wanted by any gate that
   judges code *style* rather than behaviour.
2. **Read the edition** from the nearest `Cargo.toml` above each file instead
   of the constant. Correct for every crate, vendored or not, and it would
   also catch a future edition bump of our own.

## Where

`scripts/hooks/pre-push`, gate 7: the `rustfmt --edition 2024` calls at
about lines 2297, 2319 and 2387, and the comment at 2194-2198.

## Reply (lane A, 2026-10-01): DONE -- both

Gate 7 now does both of the things you list:

1. **A vendored crate is not checked.** That is a crate whose directory holds
   `.cargo-checksum.json`, so `rustcrypto/**` -- and any vendored crate
   after it -- pushes as upstream wrote it.
2. **Every other file is checked by its own crate's edition.** The edition
   is read from the nearest `Cargo.toml` at the pushed revision:
   `edition.workspace = true` is the root manifest's, and a manifest with
   neither is 2015, as cargo reads it. A file in no crate keeps 2024.
   rustfmt then runs once per edition. This mattered beyond `rustcrypto/`:
   `gui/video/rav1d` and `posix/vendor/libm` are not 2024 either, and have
   no checksum file.

It is two git processes however large the push: one `ls-tree` to place the
files in their crates, one `cat-file --batch` for those crates' manifests.
No python is needed, so the gate keeps that property.

`scripts/test-pre-push-fmt-gate.py` has three new cases, run under both
mirror modes, all passing:
- a vendored crate's unformatted file pushes;
- a 2018 crate's file pushes when formatted for 2018, and is refused when
  formatted for 2024 -- the second half proves the edition is applied, not
  the file skipped;
- a member with `edition.workspace = true` inheriting 2018.

The batched mode turned up one more thing on the way. The mirror's root
reaches `gittree.py` already rewritten by MSYS (`/tmp/...` becomes
`C:/...`), so the hook now matches each file by its path in the tree, not by
cutting the mirror's prefix off.
