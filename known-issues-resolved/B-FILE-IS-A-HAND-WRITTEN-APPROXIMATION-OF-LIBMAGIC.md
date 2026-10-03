## B-FILE-IS-A-HAND-WRITTEN-APPROXIMATION-OF-LIBMAGIC — `file` names most formats in its own words, not file 5.45's (lane B, 2026-10-01) — **Status: FIXED 2026-10-02**

**Fixed:** `file` is now file 5.45 and its libmagic, ported source file by
source file. The library is its own crate, `userspace/libmagic` --
`apprentice` (the database reader, checker and compiler), `softmagic` (the
rule interpreter), `funcs`/`magic` (the order the tests run in, the library
calls), `encoding`, `ascmagic`, `is_tar`/`is_json`/`is_csv`/`is_simh`,
`fsmagic`, `der`, `print`, `readelf` (with `elfclass.h`), `cdf`/`readcdf`,
`compress` -- and `userspace/file` is `file.c` on top of it. The database is
file 5.45's own (`userspace/file/magic/`, vendored by
`scripts/file-magic-vendor.py`), compiled at build time exactly as `file -C`
compiles it and carried in the program, which maps it as upstream maps an
installed `magic.mgc` (design-decisions §1058). `-i`, `--mime-type`,
`--mime-encoding`, `--extension`, `--apple`, `-k`, `-z`/`-Z`, `-l`, `-C`,
`-P`, `-e` and the rest are upstream's, and so are its quirks where they
show in the output: `errno` printed after the fact (the CDF probe leaves
`EFTYPE` behind and a later ELF message names it; parsing a number resets
it), zlib's own messages for a damaged gzip stream (§1059), and the
decompressors upstream runs as programs, run the same way.

`scripts/file-diff.sh` builds file 5.45 from the release tarball and holds
ours to it byte for byte: upstream's 71 test files in seven modes, a sample
of the machine's own files, generated ELF files (every note, core file and
limit readelf.c reads), Composite Document Files and compressed files,
random mutations of all of them, standard input as a pipe and as a redirect,
names that are not files, wrong options, and the compiled database itself.
On 2026-10-02 it passed 42,266 cases. Getting there found two bugs of the
port's own, both fixed: an `!:mime` value exactly 80 bytes long panicked
where upstream writes its NUL into the next field, and getopt's complaints
named the program by its basename where upstream prints `argv[0]` as given.
A debug build, with overflow checks, ran 14,601 crafted and real files in
five modes without a panic.

Deliberately different, documented at the head of `userspace/file/src/main.rs`:
`-v` names the built-in database, and `-S` is accepted though there is no
sandbox.

The ISO media branch's generator and harness
(`scripts/file-isomedia-gen.py`, `scripts/file-isomedia-diff.sh`) are gone:
that branch is the database's own `animation` rules now, run by the
interpreter.

**Original report.**


**In short:** `file` tells you what kind of file something is. Ours checks a
few dozen formats with rules written by hand, so for most of them it uses
different words from GNU's `file` (`Apple MPEG-4 audio` where GNU says `ISO
Media, Apple iTunes ALAC/AAC-LC (.M4A) Audio`), knows far fewer formats, and
prints `-i` as a bare type where GNU adds `; charset=binary`. Scripts that
parse `file`'s output, which is common, get answers they do not expect.

**What is already faithful:** the `ftyp` family -- MP4, QuickTime, 3GP, AVIF,
HEIF and 150 more brands. Since 2026-10-01 that branch runs file 5.45's own
rules (`magic/Magdir/animation`), generated into
`userspace/file/src/isomedia_table.rs` by `scripts/file-isomedia-gen.py` and
evaluated as libmagic evaluates them (`src/isomedia.rs`); its harness,
`scripts/file-isomedia-diff.sh`, agrees with GNU's on all 726 files it
builds.

**The proper fix** is the same method for the rest: libmagic's `softmagic.c`
interpreter -- offsets (including indirect ones), the numeric and string
tests, `search`/`regex`, `name`/`use`, `default`/`clear`, the `!:mime` and
`!:ext` annotations, strength ordering -- and file's `Magdir` database
compiled into tables, with the encoding and `-i`/`--mime-type` output on top.
The generator's approach scales: each `Magdir` file becomes data, and the
interpreter grows by the test kinds that file uses. It is a port of a large
C program and its database; recorded here rather than started inside lane
F's AVIF request, which needed one branch.

**Where:** `userspace/file/src/main.rs` (every `detect_*` but the ISO branch).

**How to see it:** in WSL, `file -b x.m4a` against ours; or any format the
hand rules do not know, which ours calls `data` where GNU names it.
