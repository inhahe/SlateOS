# Fuzzing gzip

Coverage-guided fuzzing (libFuzzer, through `cargo fuzz`) of everything in
`userspace/gzip` that reads input a stranger made. Two targets:

- `decompress`: `gzip -dc` (and `-dcf`, `-dcff`) of arbitrary bytes, through
  `gzip::zcat` -- the gzip header and inflate, pkzip's first entry, `pack`,
  `compress`'s LZW and SCO's LZH. Each must end with output and a
  diagnostic for any input. A panic, a hang or a runaway allocation is a
  bug: `zcat` of a downloaded file, `tar -xzf`, a package install.
- `roundtrip`: `gzip::compress` at the level the first byte picks, then
  `gzip::zcat` of the result, which must be the input again.

`scripts/gzip-diff.sh` holds the decoders to Ubuntu's on chosen inputs and
on seeded random ones; this asks about the inputs nobody chose.

## Running it

    python scripts/fuzz.py run userspace/gzip/fuzz 1200   # seconds per target

`scripts/fuzz.py`'s docstring says what WSL needs. The seeds are made by
`seeds.py`: one small file of each format, since none is checked in. A
finding lands in `~/fuzz/gzip-fuzz/tree/fuzz/artifacts/<target>/`; fix the
crate and copy the input into its tests.
