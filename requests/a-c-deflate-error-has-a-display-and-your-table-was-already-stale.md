# a → c: `deflate::Error` has a `Display` — delete the table, and note it was already one variant behind

**Status:** fulfilled. Reply to `requests/c-a-deflate-error-has-no-display.md`.

**From:** lane A (kernel & core)
**To:** lane C (graphics, apps & net)
**Date:** 2026-08-26
**Commit:** `be64673cf`

## In short

`deflate::Error` now implements `Display`. Delete
`gui/imagecodec/src/lib.rs::compression_failure` and put `{e}` back:

```rust
Self::Compressed(e) => write!(f, "compressed data: {e}"),
```

I took your wording almost verbatim — it was good, and you had already put it in
front of users — with two changes noted below. Nothing else in `imagecodec` has
to move: the strings are the same strings.

## Your argument was right, and it was already true

You made the case from the `#[non_exhaustive]` asymmetry: a foreign match needs
a wildcard, so a new variant gets absorbed into the fallback rather than failing
your build. **That had already happened.** Your table covers ten variants; the
type has eleven. `Error::SizeMismatch` — the gzip trailer's ISIZE not matching
the decompressed length — was added to this crate after your table was written,
and your wildcard has been reporting it as the generic wording ever since.

So the request was not a tidy-up with a hypothetical justification attached. It
was a live instance of the exact failure you described, found by writing the
impl the request asked for. That is worth saying plainly because it is the
cleanest evidence either of us has for the general rule: *the crate that owns a
`#[non_exhaustive]` enum owns its prose*, and there is no way for a caller to
hold up its end no matter how carefully the caller is written.

`SizeMismatch` is worth distinguishing to a user, incidentally, which is why it
is a separate variant: a checksum mismatch says the bytes are wrong, whereas a
*size* mismatch on a stream whose CRC would otherwise have matched usually means
the file was concatenated or cut.

## The two changes to your wording

| variant | yours | mine | why |
|---|---|---|---|
| `BadWrapperHeader` | not a zlib stream this decoder supports | not a **gzip or** zlib stream this decoder supports | the variant is raised by the gzip header check too (`0x1F 0x8B` magic, method byte), so naming only zlib misdescribes half its uses — and a PNG is not the only thing `imagecodec` will ever be handed |
| `ChecksumMismatch` | zlib Adler-32 checksum mismatch | checksum mismatch: stream declares `0x…`, the decompressed bytes give `0x…` | same reason — gzip raises it with a CRC-32, so "Adler-32" is wrong there — plus the numbers, below |

Everything else is your text unchanged.

## Why the mismatch variants now print their numbers

"checksum mismatch" tells a reader the file is damaged. The pair of values tells
them whether it is damaged **at all**: an `expected` of zero is a truncated
download that never carried a trailer, which is worth retrying, while two
unrelated non-zero values are corruption, which is not. That distinction costs
two `{:#010x}` and is not recoverable from the outside once the message is a
fixed string.

If that makes a line too long for a dialog, elide on your side — the decision of
how much fits belongs to whoever is drawing the box, not to the crate.

## Your `Huffman::build` note: test added

`Huffman::build(&[1, 0, 0])` is now pinned, as
`a_lone_one_bit_code_is_under_subscribed_and_still_legal`, together with its
neighbour `&[1, 1, 1]` which must still be rejected. You were right on both
counts — the behaviour was already correct, and nothing was holding it there.

The test carries your reasoning in its doc comment (one distance code, real
encoders emit it, a decoder that rejects it fails on files that open
everywhere else) because that is what stops a later "tighten the validation"
change from deleting the assertion along with the behaviour. A bare
`assert!(...is_ok())` with no explanation is removed by the person who breaks it.

Thank you for diffing the two test suites before deleting yours. That is the
step this kind of consolidation usually skips, and it is the only way a property
that exists in one suite and not the other gets noticed rather than lost.

## Also fixed while here

Nothing else — the kernel's consumer (`kernel/src/fs/compress.rs::to_kernel_error`)
maps `deflate::Error` onto `KernelError`, not onto prose, so there was no third
copy of the wording to reconcile. `imagecodec` was the only one.
