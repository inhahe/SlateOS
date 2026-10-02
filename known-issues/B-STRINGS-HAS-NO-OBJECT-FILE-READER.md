## `B-STRINGS-HAS-NO-OBJECT-FILE-READER` (lane B, 2026-08-27) — **open**, deliberate limitation

**In short:** GNU `strings` is part of binutils, so it can open an executable,
find the "data" section inside it, and scan only that. Ours cannot — it has no
code for reading executable file formats — so it always scans the whole file
from the first byte to the last. Three of upstream's options ask for something
that needs that reader. Rather than accept them and quietly scan the whole file
anyway, we refuse them with a message that says why.

| Option | What upstream does with it | Ours |
|---|---|---|
| `-d` / `--data` | Scans only the initialised-data sections of an object file. | Refused: `strings: --data needs an object-file reader, which this build does not have` |
| `-T` / `--target=BFDNAME` | Names the object-file format to assume (`elf64-x86-64`, `binary`, …). | Refused, same sentence with `--target`. |
| `-U` / `--unicode=MODE` | Six modes for re-rendering UTF-8 runs (`show`, `invalid`, `hex`, `escape`, `highlight`). | Only `d` (the upstream default — treat UTF-8 as ordinary bytes) is accepted; the other five are refused. |
| `@FILE` | Reads further command-line options out of `FILE`. | Refused. Nothing else in this coreutils has a private argument-file syntax. |

**Why refuse rather than ignore.** Accepting `-d` and scanning the whole file
would answer a different question than the one asked, silently, and with *more*
output than the caller expected — the shape of error a reader is least likely
to notice. A script that greps the output would simply get wrong answers. A
refusal is loud, and the exit status (1) is the one upstream uses for a bad
command line, so a caller that checks its status stops rather than proceeds.

**Everything else is implemented and was measured against GNU strings 2.42**,
including the parts no reasonable implementation would guess: a bare `-`
operand means `--all` and is *not* a file to scan; `-n`'s argument goes through
`strtoul` in base 0, so `-n 010` means eight and `-n 0xA` means ten; the two
"too big" refusals are worded differently on either side of `0xffffffff`;
`-t q` and `-e q` print the bare usage with no sentence of their own; `-n`
counts *characters* while `-t` reports *bytes*, so a 16-bit run of five
characters needs `-n 5` but its offsets advance by two; and a multi-byte
character that fails the printable test restarts the scan one byte later, not
one character later.

**The proper fix**, should it ever be wanted: `-d`/`-T` need an ELF section
reader. The natural home is a shared `coreutils::elf` module (`readelf`,
`objdump`, and `nm` would all want it), not a private one inside `strings`.
`-U`'s five other modes need a UTF-8 decoder plus the four rendering styles,
which is self-contained and could be done without the reader.

**Where it lives:** `userspace/coreutils/src/bin/strings.rs` — "Where this
deliberately diverges" in the module doc, `unsupported()`, and the test
`the_object_file_options_are_refused_rather_than_ignored`.
