## B-STRINGS-HAS-NO-DATA-SECTION-OPTION (lane B, 2026-09-11)

**Status: FULLY CLOSED 2026-09-12.** Both halves. `strings-diff.sh` is
**62 passed, 0 differed, 12 differ on purpose** — from 54/15 this morning.

**The diagnostics half, which the entry called "related and cheaper".** It was
cheaper and it was one bug, not ten: upstream has THREE error shapes here and
this build had one. Measured against binutils 2.42 rather than reasoned about:

| input | binutils prints |
|---|---|
| `-n 0` | the sentence, no usage |
| `-n` with no value | the sentence, then the whole usage |
| `-t q` | the usage alone, no sentence |

`Error::referral` already separated the first two and nothing was reading it:
a getopt error — a missing argument, an unknown option — carries one, and a
value this program rejected itself does not. So the field that decides whether
to print `Try '… --help'` elsewhere in the tree is the field that decides
whether to print the usage here. Three cases fixed by reading it.

Six more were a referral this build invented: `strings` refers the reader to
the usage by SHOWING it, where coreutils proper refers to `--help` by naming
it, and this printed both.

**The last nine are xfail, and the reason is one line of our own help text.**
Every one of them prints the usage, and ours carries
`--target, @<file>, and every --unicode mode but `d' are refused` where GNU
prints `supported targets: elf64-x86-64 …`. We cannot print that list without
claiming support we do not have, so the two can never match — which is why
`--help` and `--version` were already xfail. **The part that can match is
still asserted**: each diagnostic sentence is unit-tested in `strings.rs`, 13
assertions on `e.sentence`, so what is given up is only the comparison of two
usage blocks that were never going to agree. Verified before reclassifying
rather than after: the message lines matched byte for byte once the spurious
referral was gone.

`-T nosucharch` is xfail for a different reason worth stating separately: GNU
accepts an unknown target on a non-object file and prints its strings, while
this build refuses `--target` outright.

**Status: FIXED 2026-09-12.** `strings -d` / `--data` scans only the
initialised, loaded sections of an ELF64. `scripts/strings-diff.sh` goes from
54 passed / 15 differed to 59 / 13, and the three cases that remain of the
original two are the diagnostics half described below, which is separate work.

**The rule was validated against binutils before a line was written**, not
after: on `/bin/true`, scanning every section with `SHF_ALLOC` set and a type
other than `SHT_NOBITS`, each as its own stream, yields **136** strings, and
`strings -d` yields **136**. Both halves of that test are needed — `.bss` is
`SHF_ALLOC` *and* `SHT_NOBITS`, so scanning it would read whatever the file
holds at that offset, which is the next section.

**Two things measured rather than assumed**, both of which the scoping note
below asked for:

| question | answer |
|---|---|
| what does binutils `-d` do to a non-object file? | nothing special — same output as the default |
| does our default already match theirs? | yes: on an ELF, default = `-a` = 165 strings |

So the fallback for anything that is not a parseable ELF64 is to scan the whole
file, which is upstream's behaviour rather than a gap being papered over.

**THE OFFSET WAS THE HALF THAT WAS EASY TO GET WRONG.** Each section is scanned
as its own stream, so a naive implementation reports the offset *within the
section* and every number under `-t` is wrong by the section's start. Nothing
in the harness would have caught it: `-d elf.bin` prints identical strings
either way. Three cases were added — `-d -t x`, `-d -t d`, `-d -n 8 -t x` — and
then verified to fail when the base offset is removed, because a case that
passes proves nothing until you have seen it fail.

A test was narrowed rather than deleted:
`the_object_file_options_are_refused_rather_than_ignored` becomes
`an_unimplemented_object_file_option_is_refused_rather_than_ignored`. It lost
one of its two subjects and kept the other, `--target`, which must still refuse
— silently ignoring it would hand a caller output for the wrong architecture
and look right. It now also asserts that `-d` IS accepted, so it fails if the
refusal is ever quietly put back.

Only ELF64 little-endian is parsed. A 32-bit or big-endian object is scanned
whole: wrong, but wrong in the direction of printing more rather than less.
Every offset and length out of the section table is range-checked with
`checked_add` and `get` — an object file is attacker-chosen input like any
other.

`cargo test -p coreutils --bin strings`: 45 passed, 0 failed.

The description below is kept in the tense it was written in.

`strings -d` / `--data` is absent. GNU's `strings` scans only the *initialised,
loaded* sections of an object file with it, which is the difference between
listing the strings a program will actually have in memory and listing every
run of printable bytes in the file — including ones in debug info and symbol
tables that never load.

Both halves of the duplicate pair lacked it, so it survived the retirement of
the standalone. `scripts/strings-diff.sh` covers it in three cases
(`-d elf.bin`, `--data elf.bin`, alongside `-a`), all currently refusing.

**Shape of the fix, scoped 2026-09-12 so it is not re-derived.** Walk the ELF
section headers and scan only sections with `SHF_ALLOC` set and a type other
than `SHT_NOBITS` — allocated means it is loaded, not-NOBITS means its bytes
are actually in the file rather than zero-filled at load. `.bss` is the section
that is `SHF_ALLOC` *and* `SHT_NOBITS`, which is exactly why both halves of the
test are needed; scanning it would read whatever the file happens to hold at
that offset, which is the next section.

**The constants go in `strings.rs`, not `use posix::…`.** They already exist as
`SHT_PROGBITS`/`SHT_NOBITS`/`SHF_ALLOC` in
`posix/src/linux_elf_section_types.rs`, and reusing them looks like the obvious
move — but `userspace/coreutils` deliberately depends only on host-buildable
path crates (`procinfo`, `ere`, `quote`), because the whole crate is tested
with `cargo test -p coreutils --target x86_64-pc-windows-gnu`. `posix` is the
target's libc shim. Taking the dependency to save three integer constants would
cost the crate its host build, which is where its tests run.

**Two things to measure rather than assume**, both cheap once the harness is
green on the rest: what binutils does with `-d` on a file that is NOT an object
file, and whether our default already matches its default — binutils' man page
says scanning the whole file is normally the default, so `elf.bin` and
`-a elf.bin` should already agree, and if they do not then `-d` is not the only
thing wrong here. Every offset and length out of the section table is
attacker-controlled and must be range-checked against the file size with
`checked_add`/`get`, not indexed — the crate denies `indexing_slicing` and
`arithmetic_side_effects` for exactly this shape of input.

Related and cheaper: the diagnostics for a bad option *value* — `-n 0`,
`-n -1`, `-n notanumber`, `-t q`, `-e q`, and each of those options given
nothing — differ from GNU's wording in 10 of the 15 remaining failures. The
exit statuses already agree, so this is sentences rather than behaviour.
