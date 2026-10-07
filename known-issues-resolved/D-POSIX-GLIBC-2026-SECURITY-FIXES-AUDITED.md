## D-POSIX-GLIBC-2026-SECURITY-FIXES-AUDITED — glibc's 2024-2026 security fixes checked against this C library: none of their bugs is here, but looking found five of our functions far short of glibc's (lane D, 2026-09-30) — **Status: FIXED 2026-09-30 (all five of ours; `posix_spawn`'s attributes, brought up again below, stay with TD-D-POSIX-SPAWN-IGNORES-ITS-ATTRIBUTES)**

**In short:** glibc fixed a run of security bugs in 2024-2026, and the
oracle's glibc -- Ubuntu's 2.39, `2.39-0ubuntu8.9` -- carries the fixes.
Each was checked against this library's own version of the function. None
of the bugs is here: ours were written differently, and where a fix came
with a test, its case now runs against ours. But reading the functions
beside glibc's found five of ours that do far less than glibc's, in ways a
program ported from Linux will notice: `getopt` and `getopt_long`,
`regcomp`, `wordexp`, `strfmon`, and `memalign`'s rounding (fixed) --
and brought up again `posix_spawn`'s attributes, already recorded.

**The fixes, one by one:**

| glibc's fix | What glibc got wrong | Here |
|---|---|---|
| CVE-2026-5435, CVE-2026-6238, bug 34289 | `ns_sprintrrf` read past a record's data (CERT, TKEY, TSIG, LOC, A6) | ported with the fixes, glibc's own test swept (`posix/src/nameser.rs`) |
| CVE-2026-0861 | `memalign`'s padded size wrapped for a huge alignment | dlmalloc's check holds for every alignment; the `PTRDIFF_MAX` cap glibc has is added, and `tst-malloc-too-large` is mirrored (`posix/src/malloc.rs`) |
| CVE-2025-0395 | the assertion message's buffer was a struct short | no buffer here; the message is now glibc's (`posix/src/assert.rs`) |
| CVE-2026-5450 | `%mc` grew its buffer one byte short | capacity is checked before each byte; the test is mirrored, `%mlc` too (`posix/src/scanf.rs`) |
| CVE-2026-5928 | `ungetwc` compared against the byte stream | the character's own bytes are pushed back; the test is mirrored (`posix/src/wchar.rs`) |
| CVE-2026-19542 | `tdelete`'s parent stack overflowed | a fixed 128-entry stack, each push checked -- the size glibc's fix chose (`posix/src/search.rs`) |
| CVE-2026-4437 | DNS answers read on past the answer section | lookups go to the kernel's resolver, whose parsers loop over ANCOUNT only (`kernel/src/net/dns.rs`) |
| CVE-2026-0915 | `getnetbyaddr`'s DNS query built from uninitialised bytes | `getnetbyaddr` reads files only (`posix/src/netdb.rs`) |
| CVE-2025-8058 | `regcomp` freed twice after an allocation failed | nothing freed by hand: every table is dropped once, and glibc's test -- each allocation failed in turn -- is mirrored (`posix/src/regex.rs`) |
| CVE-2026-19499 | `strfmon` right-justified over its own padding | ours ignores widths altogether -- see `strfmon` below |
| CVE-2025-15281, CVE-2026-6368, CVE-2026-6791 | `wordexp`'s `WRDE_REUSE`, `WRDE_APPEND` and `~user` | ours has none of the three -- see `wordexp` below |
| CVE-2024-2961, CVE-2026-4046, CVE-2026-77117, CVE-2026-80489 | iconv's ISO-2022-CN-EXT, IBM1364, SHIFT_JISX0213 and EUC-JISX0213 converters | none of those charsets is here |

**What reading them found instead** -- each to be rewritten from the
standard with glibc as the oracle, as the rest of this library is:

- **`getopt`, `getopt_long`, `getopt_long_only`** (`posix/src/getopt.rs`):
  no error message is ever printed (`opterr` is read by nothing); argv is
  never permuted, so `prog file -v` does not see `-v` as glibc's does; no
  `-` or `+` optstring prefix, no `POSIXLY_CORRECT`, no `::` optional
  argument, no `-W`, no `optind = 0` restart; long options match only
  whole, never by an unambiguous prefix, and there is no "ambiguous"
  error. Nearly every C command-line program leans on some of this.
  **Fixed 2026-09-30**: glibc's, all 2,571 of its parses in
  `posix/src/getopt_oracle.txt` answered alike.
- **`regcomp`** (`posix/src/regex.rs`): no interval expressions (`\{m,n\}`,
  `{m,n}`) and no back-references (`\1`), both of which POSIX requires;
  patterns past 1024 bytes, programs past 512 instructions and more than 9
  groups are refused. The userland's own tools use `userspace/ere`, which
  has both; C programs that call `regcomp` get this. **Fixed 2026-09-30**:
  intervals, back-references, every GNU operator glibc's `regcomp` reads,
  REG_STARTEND, and no fixed limit; glibc's answers to some 544,000 cases
  given alike but for the 16,444 where they contradict the standard or
  glibc's own (design-decisions section 1160). What is bounded still:
  D-POSIX-REGEX-BOUNDS-AND-WORST-CASES.
- **`posix_spawn`'s attributes** (`posix/src/spawn.rs`): the flags are
  stored, and a child asked for with a signal mask, default signal actions,
  a new session or a scheduler gets none of them. Already known:
  `TD-D-POSIX-SPAWN-IGNORES-ITS-ATTRIBUTES`, waiting on the kernel record
  asked of lane A in
  `requests/d-a-ignored-signals-and-spawn-attributes-need-a-kernel-record.md`.
- **`wordexp`** (`posix/src/wordexp.rs`): input past 4096 bytes is cut
  off, more than 256 words are not kept, `WRDE_APPEND` and `WRDE_DOOFFS`
  are ignored (an append leaks the list it replaces), command substitution
  gives back its own text, and there is no arithmetic, no `${...}` form but
  the plain one, no `IFS` splitting, no pathname expansion, no `~user`.
  **Fixed 2026-09-30**: every POSIX expansion, glibc's answers to 320 cases
  (`posix/src/wordexp_oracle.txt`) given alike but for 15 where it
  contradicts POSIX, each recorded in `posix/src/wordexp.rs`.
- **`strfmon`** (`posix/src/monetary.rs`): not variadic (it takes one
  `double`, so a second conversion prints the first value again); the
  field width, the `-` and `#` flags and `%L` are not honoured; output
  that does not fit is cut short and counted as success, where glibc
  answers -1 with `E2BIG` -- glibc's own test of CVE-2026-19499 gets 4
  back here. **Fixed 2026-09-30**: glibc's in the C locale, all 1,572
  calls in `posix/src/strfmon_oracle.txt` answered alike.
- **`memalign`** (`posix/src/malloc.rs`) refused 0, 3 or 24 as an
  alignment, being `aligned_alloc`; glibc's takes 0 as `malloc` and rounds
  the others up to a power of two. **Fixed 2026-09-30.**

**Where:** the modules named above; the patches are in
`glibc_2.39-0ubuntu8.9.debian.tar.xz` (Launchpad), `debian/patches/`.
