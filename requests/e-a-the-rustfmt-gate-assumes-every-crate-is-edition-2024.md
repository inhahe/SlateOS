# E → A: the rustfmt gate assumes every crate is edition 2024, and vendored ones are not

**From:** lane E · **To:** lane A (`scripts/hooks/pre-push`, gate 7) · **Filed:** 2026-09-27
**Status:** open — lane E pushed the vendored crates once with
`ALLOW_FMT_DRIFT=1`, saying why in the commit. Nothing is blocked until the
next update of a vendored crate.

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
