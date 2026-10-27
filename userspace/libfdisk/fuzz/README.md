# Fuzzing libfdisk

Coverage-guided fuzzing (libFuzzer, through `cargo fuzz`) of everything in
`userspace/libfdisk` that reads input a stranger made. Two targets:

- `probe`: a two-mebibyte disk whose front is the input, the rest zeros --
  probed, listed, verified, its partitions and free space read and every
  field of each shown, and a partition added with every default; and, when
  the disk holds an MBR, a BSD label looked for inside it. An MBR's chain of
  extended boot records, a GPT's headers and entries, SGI and Sun labels and
  a BSD disklabel are all parsed from those bytes, and so are the
  filesystems libblkid finds in the partitions. A panic, a hang or a runaway
  allocation is a bug: `fdisk -l` of a disk plugged in from elsewhere.
- `script`: an sfdisk script, parsed, applied to a blank disk in memory, and
  dumped back.

`scripts/fdisk-diff.sh`, `sfdisk-diff.sh` and `cfdisk-diff.sh` hold the
programs to Ubuntu's on chosen disks; this asks about the disks nobody
chose.

## Running it

    python scripts/fuzz.py run userspace/libfdisk/fuzz 1200   # seconds per target

`scripts/fuzz.py`'s docstring says what WSL needs. The seeds are made by
`seeds.py`, with Ubuntu's `sfdisk`, since none is checked in. A finding
lands in `~/fuzz/libfdisk-fuzz/tree/fuzz/artifacts/<target>/`; fix the crate
and copy the input into its tests.
