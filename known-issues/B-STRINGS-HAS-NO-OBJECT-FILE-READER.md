## `B-STRINGS-HAS-NO-OBJECT-FILE-READER` (lane B, 2026-08-27) — **open**, deliberate limitation, now only `-T`

**Status:** OPEN -- narrowed 2026-10-02: only `-T`/`--target` is left.

**In short:** GNU `strings` is part of binutils, whose object-file library
(BFD) can read any of a few dozen executable formats. Ours reads one -- ELF64,
little-endian, which is what this OS builds -- and that is enough for `-d` to
scan only an object's loaded data sections, as upstream does. What it cannot do
is honour `-T BFDNAME`, which names the format to read the file as. Rather than
accept a format and read the file as ELF anyway, it refuses `-T` with a message
that says why.

| Option | What upstream does with it | Ours |
|---|---|---|
| `-d` / `--data` | Scans only the initialised, loaded sections of an object file. | **Implemented** for ELF64; any other file is scanned whole, which is upstream's behaviour for a non-object. |
| `-T` / `--target=BFDNAME` | Names the object-file format to assume (`elf64-x86-64`, `binary`, ...). | Refused: `strings: --target needs an object-file reader, which this build does not have`. |
| `-U` / `--unicode=MODE` | Six ways to treat UTF-8. | **Implemented 2026-10-02**, ported from `print_unicode_stream` and `print_unicode_buffer`, quirks included (see below). |
| `@FILE` | Reads further arguments out of `FILE`. | **Implemented 2026-10-02**, libiberty's `expandargv` and `buildargv`. |

**Why `-T` is refused rather than ignored.** Accepting a format and reading
the file as ELF anyway would answer a different question than the one asked,
silently. A refusal is loud, and its status (1) is the one upstream uses for a
bad command line, so a caller that checks its status stops rather than
proceeds. (Measured: GNU accepts an unknown target on a file that is not an
object at all and prints its strings; `scripts/strings-diff.sh` records that
case as an expected difference.)

**What `-U` reproduces that no reasonable implementation would guess**, each
measured against GNU strings 2.42: the help advertises `show`/`s`, and the
parser takes `locale`/`l` for that mode and refuses `s`; `l` prints only a
character's *first byte* (`printf ("%.1s", ...)`); a four-byte character is
escaped through arithmetic that is not its code point, so U+1F600 prints as
`߆00` and U+10FFFF as `ြfff`; and any mode but `d` forces `-e S`,
after the whole option loop. One more is reproduced from the source rather than
measured: a four-byte sequence refused under `-U i` is pushed back out of order.

**The proper fix for `-T`**, should it ever be wanted: more readers behind the
same section scan -- ELF32, big-endian ELF, PE -- in a shared `coreutils::elf`
(or `objfile`) module that `readelf`, `objdump` and `nm` would want too, with
BFD's target names mapped onto them.

**Where it lives:** `userspace/coreutils/src/bin/strings.rs` -- `unsupported()`
and `elf_alloc_ranges()`.
