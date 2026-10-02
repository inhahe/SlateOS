## TD-B-BLKID-HAS-NO-UDEV-OUTPUT-FORMAT -- FIXED 2026-09-16

`blkid -o udev` is implemented, to the measured reference behaviour below
rather than to a guess. `OutputFormat::Udev` emits `ID_FS_TYPE`, `ID_FS_UUID`
/`_ENC`, `ID_FS_LABEL`/`_ENC`, `ID_PART_ENTRY_UUID` and
`ID_PART_ENTRY_NAME`/`_ENC`. 23 -> 27 tests, clippy clean.

**Two encoders, not one, and the tests exist to keep them apart.** The entry
below says "Do not reuse one encoder for both -- they differ, and the
difference is the point of having two tags", so the regression to guard is
precisely somebody tidying them into one function. Probed: replacing
`udev_plain`'s body with `udev_encode(raw)` fails the suite.

The control is the pair `AÿþB` and `AþÿB` -- two DIFFERENT labels
that the plain tag renders identically as `A__B`, while `_ENC` keeps them
apart. That is the same collision `from_utf8_lossy` caused in this program
before labels were carried as bytes, which is why the lossy tag is only safe
to ship *alongside* the reversible one.

`ID_FS_UUID_ENC` is emitted even though a UUID is hex-and-dashes and needs no
escaping: a consumer reading `_ENC` uniformly should not have to special-case
the one field that happens to be safe.

No `DEVNAME` line, unlike `-o export`. `-o udev` is consumed by a udev rule
that already knows which device it is processing, and the reference does not
emit one -- checked rather than carried over from the neighbouring format.

The description below is kept in the tense it was written in.

---

**The original entry, 2026-09-15, follows verbatim.**

`blkid -o udev` is the one output format util-linux has that we do not. It is
refused honestly today -- `unknown output format: udev` -- so nothing claims
it works; this is a missing feature, not a wrong one.

It is worth recording because of WHY it exists, which was measured against
util-linux 2.39.3 in WSL rather than recalled. A real ext4 image was built
with `mkfs.ext4 -L $(printf 'A\xff\xfeB')` -- a label that is deliberately
not valid UTF-8 -- and the reference was asked what it prints:

    $ blkid -o value -s LABEL lbl.img | xxd
    00000000: 41ff fe42 0a                             A..B.

    $ blkid -o udev lbl.img
    ID_FS_LABEL=A__B
    ID_FS_LABEL_ENC=A\xff\xfeB

So util-linux answers the "should output be escaped?" question by **refusing
to choose**: `LABEL` is the raw bytes, and the escaped forms live in `-o udev`
under DIFFERENT TAG NAMES -- `ID_FS_LABEL` with unsafe bytes replaced by `_`,
and `ID_FS_LABEL_ENC` with them hex-escaped. Both are present in the same
output, so a consumer picks the one it can handle rather than having a policy
imposed on it.

Two further measurements, both of which our `-t` now matches and did not
before 2026-09-15:

    blkid -t LABEL=$(printf 'A\xff\xfeB')          -> matches
    blkid -t LABEL=$(printf 'A\357\277\275...B')  -> does NOT match

That is, the reference matches the RAW bytes and does not match the U+FFFD
replacement form. Our `from_utf8_lossy` label made us do exactly the opposite
of both -- see the commit "blkid: lossy-decoding filesystem labels merged
distinct devices".

**What implementing it looks like.** Add `OutputFormat::Udev`; emit
`ID_FS_<TAG>` for each field, plus `ID_FS_<TAG>_ENC` for the two that can
carry arbitrary bytes (`LABEL`, `PARTLABEL`). The `_ENC` encoder is
util-linux's `blkid_encode_string`: pass through the safe set and emit
`\x<hh>` for everything else. The plain form replaces each unsafe byte with
a single `_`. Do not reuse one encoder for both -- they differ, and the
difference is the point of having two tags.

**Why it is not urgent.** `-o udev` exists for udev rules, and we have no
udev. Nothing in the tree calls it. The blocking half -- carrying label bytes
intact so an encoder has something faithful to encode -- is done; what is
left is the formatting.
