# Open Questions — Operator Decision Queue

Decisions that genuinely need the human operator: architectural forks,
user-visible policies, and tradeoffs with no obviously-correct answer that
Claude has **deferred** rather than resolved autonomously.

This file is distinct from:

- **`design-decisions.md`** — decisions already *made* (each marked with who
  decided it). When the operator answers a question here, move it there as a
  `Decided by: Operator` entry and delete it from this file.
- **`known-issues.md`** — bugs and accumulated technical debt.
- **`todo.txt`** — the working scratchpad / judgment-call log.
- **`deferred-questions.md`** — questions that will need the operator *eventually*
  but cannot be answered usefully yet, each with a trigger for promoting it back
  here. Anything whose own text says "ask again later" belongs there, not here:
  this file is a queue, and a padded queue gets skimmed.

### How an answer actually arrives — read this before assuming nobody replied

The operator has answered this queue by writing a plain text file,
**`open-questions-answers.txt`, in the integration tree** (`E:/visual studio projects/os`), one paragraph per question keyed by its ID. As of 2026-09-12 that
file is dated 2026-09-07, holds about two dozen answers spanning all three lanes,
and every one of them has been processed. **The channel works. What does not work
is noticing it.**

- It is **untracked** — not ignored, just never added — so it exists in exactly one
  directory on one machine. It is on no branch, in no lane's worktree, and in no
  clone. Fetching and merging `origin/main`, which is what the start-of-task
  checklist tells you to do, cannot show it to you.
- **Nothing watches it.** No gate, no hook, no script mentions the filename.
- It was found on 2026-09-12 **by accident**, in `git status` output during an
  unrelated merge, five days after it was written.

So: **check it at the start of a task**, alongside the merge. Reading the
integration tree is fine — the rule against touching `os` is about *writing*.

```bash
cat "E:/visual studio projects/os/open-questions-answers.txt"
```

A question sitting at `Status: OPEN` here is **not** evidence that the operator has
not answered it. An entry can be open precisely because the operator *did* reply
and asked for a clearer explanation — which is a reply, and which is invisible
from this file alone.

*Both examples this note originally cited have since closed, which is worth
saying rather than quietly editing: C-Q9 was written up as §841 on 2026-09-13,
and lane B withdrew B-Q8's option (c) as overtaken on 2026-09-14. The point
stands and the examples did not — so if you are checking the claim against them,
check the dates first. Examples naming live entries go stale by being right;
this note now names its examples as history instead. — lane C, 2026-09-14.*

*Recorded by lane A. This describes what has been observed, not a policy the
operator has set; if a different channel is preferred, say so and this goes away.*

Format for each entry — **written for a reader who does not know the
subsystem**, because an entry the operator cannot decide from has failed no
matter how correct it is:

- **`In short:`** — 2–4 sentences, **no jargon**, opening every entry: what is
  wrong now, what a user would actually see, and what the choice is between. If
  a term of art seems unavoidable here, the paragraph is wrong — rewrite it.
- **Question** — the decision to be made, with every term of art glossed in-line
  on first use in ≤ 10 words, even if it is glossed in another entry. Assume
  nothing carries over: the operator reads one entry at a time, months apart.
- **Options** — each with pros, cons, and a one-line **`What changes:`** stated
  as an observable difference ("the clock reads Eastern instead of UTC"), not an
  implementation, so the options can be compared without reading the prose.
- **If never answered** — one line: is today's behaviour safe, is anything
  blocked, does it get worse with time.
- **Claude's recommendation** — if there is a defensible default (and what
  Claude is doing in the meantime).
- **Where it bites** — files/symbols affected, so the resolution can be applied.
- **Status** — `OPEN` until the operator decides.

Keep entries to what a *decision* needs. Detail that only matters after the
answer belongs in `known-issues.md` or the `requests/` file. Prefer a short
table to a paragraph and a concrete example to an abstraction. (The rule is in
`CLAUDE.md` → "Write `open-questions.md` for a reader who does not know the
subsystem".)

**The body of this file holds OPEN questions only.** When the operator answers
one: write it up in `design-decisions.md` as a `Decided by: Operator` entry,
**delete the entry from here**, and add one line to the `

## D-Q7 — [D] The C library tells programs text is plain ASCII, then reads and writes it as UTF-8. Which should it be? — Status: OPEN (raised 2026-09-29)

**In short:** a program can ask the C library how text is encoded -- whether
"é" is an error (plain ASCII, which has no accented letters) or two bytes
(UTF-8, what SlateOS uses everywhere). Asked, the library says ASCII; but
when it actually converts text, it treats it as UTF-8. Programs that go by
the answer -- every GNU program ported here -- act as though
accented text could not occur (plain quotes instead of curly ones, a
conversion to "the user's encoding" failing on "é"); programs that just
convert, work. The choice: say UTF-8 everywhere, as your August decision
for the shell (Q38: "no non-UTF-8 locale") suggests, or copy Linux, where a
program starts in a one-byte-per-character mode and switches to UTF-8 when
it asks for the user's settings.

**Terms.** *Locale*: a program's language and text settings, chosen with
`setlocale`; "C" is the built-in one every program starts in, and
`LC_ALL=C` in a script asks for it. *`CODESET`*: the question "which
encoding?", asked with `nl_langinfo`. *`MB_CUR_MAX`*: the longest character,
in bytes.

What each part says today:

| Part | Says |
|---|---|
| `nl_langinfo(CODESET)` | `ANSI_X3.4-1968`: plain ASCII |
| `iconv`'s default encoding (its empty name) | ASCII |
| `setlocale(LC_ALL, "")` -- "use the user's settings" | "C", whatever was asked for |
| `MB_CUR_MAX` | 4: UTF-8's |
| `mbrtowc`, `wcrtomb` and the other conversions | UTF-8, in every locale |
| `mbrtoc16`, `mbrtoc32`, `c16rtomb`, `c32rtomb` | UTF-8, as `mbrtowc` (ASCII only until 2026-09-29: a bug under every option) |
| `iswalpha` and the other class and case functions | Unicode's, in every locale: glibc's `C.UTF-8` rules since 2026-10-01 (design-decisions §1167); ASCII's before, which was wrong under every option for a program that asks for the user's settings |
| `fnmatch`'s and `regex`'s bracket classes (`[[:alpha:]]`) | bytes, ASCII's classes |
| the `locale` command (lane B) | the C locale's character set is ASCII |

| Option | *What changes:* |
|---|---|
| **A. UTF-8 everywhere, and said so** | "Which encoding?" answers UTF-8 in every locale, `setlocale(LC_ALL, "")` answers `C.UTF-8`, `iconv`'s default is UTF-8, the `locale` command says UTF-8. `LC_ALL=C` changes nothing about text. |
| **B. As Linux and musl: one byte a character in "C", UTF-8 when asked for** | A program starts in a "C" locale where every byte is one character (as POSIX.1-2024 requires of it); `setlocale(LC_ALL, "")` gives it `C.UTF-8` -- SlateOS's default setting -- where text is UTF-8 and everything says so. `LC_ALL=C` in a script gives byte-at-a-time behaviour, as on Linux. |
| C. Leave it | The mismatch above stays. |

- **A**: the least work, and what the library already does when converting;
  consistent with Q38. But `LC_ALL=C` -- which `./configure` scripts and
  many build and shell scripts set to get byte-at-a-time behaviour, and
  which makes GNU `grep`, `sed` and `sort` take their fast one-byte paths
  -- would no longer mean that here, and a program that never calls
  `setlocale` gets UTF-8 where on Linux it gets bytes. It departs from
  POSIX.1-2024, which requires the "C" locale to be one byte a character.
- **B**: ported programs behave exactly as they do on Linux, scripts' `LC_ALL=C`
  included, and it is what POSIX requires; every program that asks for the
  user's settings -- nearly all that handle text -- gets UTF-8, so what a user
  sees is UTF-8 throughout. More work: every conversion, `MB_CUR_MAX` and
  the character-class functions have to follow the program's (or thread's)
  locale -- a few hours. The "C" locale would be the one non-UTF-8 setting,
  which Q38's premise said SlateOS does not have; osh stays UTF-8-only
  either way.

**If never answered:** nothing breaks and nothing is blocked; ported
programs keep taking accented text for errors in the places that ask the
encoding by name, and each port that meets it is a case of this question.

**Claude's recommendation:** **B**, with the "C" locale as musl's --
every byte a character, so nothing in it is ever an encoding error -- and
`C.UTF-8` the default setting. It is what the software being ported is
written against, and what POSIX asks for, while keeping SlateOS UTF-8
wherever a user's text is shown. In the meantime lane D fixes only what is
wrong either way (`<uchar.h>`'s conversions, which must agree with
`mbrtowc`'s, and the class functions, which must know the letters the
user's text is written in), and leaves the ASCII answers alone.

**Where it bites:** `posix/src/langinfo.rs` (`CODESET`), `posix/src/locale.rs`
(`setlocale`), `posix/src/wchar.rs` and `posix/src/uchar.rs` (the
conversions), `posix/src/ctype.rs` (`MB_CUR_MAX`), `posix/src/iconv.rs`
(the empty name), `userspace/locale` (lane B); design-decisions §104 and
§351, which assume no non-UTF-8 locale.

## D-Q6 — [D] Some of the C library is translated from glibc, whose licence binds every program the library is built into. Keep it, or rewrite those parts? — Status: OPEN (raised 2026-09-28)

**In short:** to make the C library behave exactly as Linux's (glibc)
does, several parts of it were written by translating glibc's own source
code into Rust, line by line -- most recently the Tamil character set,
the new C23 maths functions and `clog10` -- and `<obstack.h>`'s macros
follow glibc's header's, macro for macro, and argp, the timezone code
and the system logger glibc's source, function for function. glibc's
licence (the LGPL) allows that, on a condition: anyone who receives a
program containing it
must be able to rebuild that program with their own copy of the library.
The C library is built into *every* program on SlateOS, so the condition
reaches every program, ours and anyone else's. An earlier decision
(design-decisions.md §1133) assumed the library should stay free of that
condition and chose other sources for the complex-number functions; the
translations since have not followed it. Which should hold?

**Terms used below.** *LGPL*: the licence glibc is under -- free to use and
change, but code derived from it stays under it, and a program containing
it must let the user swap in their own build of that code. *Statically
linked*: the library's code is copied into each program, as all programs
here are today. *Clean-room rewrite*: writing the code again from the
standards and from glibc's observable behaviour, without its source open --
the tests that compare us with glibc (glibc as the *oracle*) stay exactly as
they are, since running a program is not copying it.

What is translated, as far as lane D knows:

| Where | From glibc's | Since |
|---|---|---|
| `posix/src/iconv.rs`: the CP1255, CP1258 and TCVN converters' loops | `iconvdata/cp1255.c`, `cp1258.c`, `tcvn5712-1.c` | 2026-09-27, on `main` |
| `posix/src/iconv.rs`: the T.61 / ISO 6937 / ANSI X3.110 decoder | `iconvdata/t.61.c` and kin | 2026-09-28, on `main` |
| `posix/src/iconv.rs`: TSCII | `iconvdata/tscii.c` | 2026-09-28, on `main` |
| `posix/src/c23math.rs`: `nextup` ... `fminimum_mag_num`, `scalbl` | `math/`, `sysdeps/ieee754/*`, `e_scalbl.S` | 2026-09-28, on `main` |
| `posix/src/narrow.rs`: the narrowing functions' checks | `math/math-narrow.h` | 2026-09-28, on `main` |
| `posix/src/complex*.rs`: `clog10` | `math/s_clog10_template.c`, `x2y2m1` | 2026-09-28, on `main` |
| `posix/include/obstack.h`: the macros, macro for macro -- C, in a header a program compiles into itself | the installed `<obstack.h>` (`malloc/obstack.h`) | 2026-09-30 |
| `posix/src/argp/`: the parse, the help's order and layout, the line filler -- function for function | `argp/argp-parse.c`, `argp-help.c`, `argp-fmtstream.c` | 2026-10-01 |
| `posix/src/tz.rs`: `tzset`, the POSIX rule engine, the zoneinfo reader and `mktime`'s search -- function for function, read from the source | `time/tzset.c`, `time/tzfile.c`, `time/mktime.c` | 2026-10-01 |
| `posix/src/syslog.rs`: the logger -- connecting, building the record, sending it and trying again -- function for function, read from the source | `misc/syslog.c`, BSD's in origin (the University of California's licence) with glibc's changes under the LGPL | 2026-10-01 |

One more part, since this was raised, was written with glibc's source
open, though not translated from it: `posix/src/regex/parse.rs`
(2026-09-30) takes the order of glibc's `regcomp.c` checks -- which
character is special where, which error a malformed interval gets, and
what each of the GNU interface's syntax bits changes -- from reading that
file, and the oracles' cases then pin each rule: 11,250 pairs of tokens in
POSIX's two syntaxes, some 41,000 patterns in the GNU ones. Its code is not
glibc's: an explicit stack where glibc recurses, its own types, none of
glibc's lines. Under **B** it would be derived again from the oracles'
answers alone, which already fix every rule it has. (The matcher behind
it, the rest of `posix/src/regex/`, follows the standard and owes glibc's
code nothing; its fastmap, `fastmap.rs`, reaches glibc's answer by its
own reasoning over the tree, where glibc reads its automaton's states.)

The obstack functions behind `<obstack.h>`'s macros, `posix/src/obstack.rs`,
are not translated: they are held to the oracle's answers, which show every
chunk size the program's allocation function is asked for. The header is the
interface itself -- a program expands its macros, and must get what glibc's
give -- so under **B** it would be written again from the glibc manual's
description of each macro and held to the same check
(`posix/tools/oracle/obstack_harness.py --header`: both of the header's forms,
over glibc's own functions). (The LGPL, in 2.1's §5, lifts its conditions
from a program that uses only a header's data structure layouts and small
macros, ten lines or fewer -- as each of these is; whether that settles it
for this header is part of this question.)

argp, `posix/src/argp/`, was written from what glibc's argp source does, as
known rather than read -- but for argp-fmtstream.c's line-breaking scan,
read to settle a case -- and every rule held to the oracle: 175 scenarios
of glibc 2.39's. Its code is its own, a parse over raw pointers and the
crate's lists rather than glibc's structures, but its algorithms are
glibc's on purpose: help text that breaks where glibc's does needs the
same buffering and the same scan. Under **B** it would be written again
from the manual and the oracle alone, which fix the order of the calls
and the layout, though not every effect of the buffering.

The timezone code, `posix/src/tz.rs`, is glibc's `tzset.c`, `tzfile.c` and
`mktime.c` translated function by function, with glibc 2.39's source open
(2026-10-01): which zone a `TZ` value names, and what each call leaves for
the next -- `tzname`, the `posixrules` offset, `mktime`'s remembered guess
-- live in those files' details, and a program written against glibc
sees them. Under **B** it would be written again from the oracle's answers
alone (`posix/tools/oracle/tz_harness.py`, 57 scenarios), which fix every
behaviour they exercise, though not glibc's state after sequences of
calls they do not.

The system logger, `posix/src/syslog.rs`, is glibc's `misc/syslog.c`
translated function by function with its source open (2026-10-01):
what a record looks like, when the connection is made again, which
copies stop at a NUL -- a program logging through glibc sees all of it.
Under **B** it would be written again from the oracle's answers alone
(`posix/tools/oracle/syslog_harness.py`, 44 scenarios, with daemons
that restart, vanish and change kind), which fix every record, copy and
retry they exercise. That file began as BSD's, and under **A** its
notice -- the University of California's licence -- travels with the
translation as well; glibc's changes since are the LGPL's. The journal
that stands in for the daemon on SlateOS (design-decisions §1166) owes
glibc nothing.

The wide classes and case mappings, `posix/src/wctype_tables.rs`
(2026-10-01), are generated by rules of the shape of glibc's own
generator (`localedata/unicode-gen`), as known rather than read:
`Alphabetic` and the other scripts' digits make `alpha`, and the no-break
spaces are kept out of `space`. Each rule was then held to the oracle
until no code point differed (`posix/tools/wctype_gen.py --oracle`), and
two were found only that way. The tables are Unicode's data, not glibc's
code, so under **B** nothing changes: the rules are what the oracle fixes.

(The character tables themselves -- which byte means which letter -- are
facts read from glibc's data files and from running its converters, not
code; they are not in question. And not everything follows glibc's
source: the `long double` Bessel functions, `posix/src/besl.rs`, were
written from the mathematics, with glibc only run to see its answers.)

| Option | *What changes:* |
|---|---|
| **A.** Keep the translations; honour the LGPL | The files above say they are LGPL. Every program built on the C library must be re-linkable by its user -- which means shipping the library's object files with the system, or making the C library a shared library (`libc.so`) as design.txt plans for later. Nothing is rewritten. |
| **B.** Rewrite those parts clean-room; glibc stays the oracle, never the source | The library stays under whatever licence SlateOS chooses, with no condition on programs. The six parts are written again from the standards (C23, IEEE 754, the TSCII and ISO 6937 specifications) and must pass the same glibc-comparison tests they pass now; a rule is written down: glibc may be tested against, not read and copied. |
| **C.** Decide before the first public release, not now | Work continues as it is; the table above is kept current; before anything is distributed as a binary, A or B is applied. |

**If never answered:** nothing breaks and nothing is distributed yet; the
cost of B grows with every further translation, and lane D will keep
translating where glibc is the clearest description of the behaviour
wanted.

**Claude's recommendation:** **B.** The C library is the one piece of code
every program contains; keeping it free of conditions is worth a few hours
of rewriting, and the glibc comparison tests -- the part that actually
guarantees glibc's behaviour -- stay unchanged, so the rewrites cannot drift
from what the translations do today. Until you answer, lane D writes from
the standards with glibc as the oracle, and where it has read glibc's
source after all -- the timezone code and the logger, both 2026-10-01 --
the table above says so.

**Where it bites:** the six places above; `design-decisions.md` §1133 (the
earlier assumption); and every future port where glibc's behaviour is the
target.

## D-Q5 — [D] Chinese, Japanese and Korean text conversion needs about a megabyte of tables. Build them into every program that converts text, or load them from files? — Status: OPEN (raised 2026-09-28)

**In short:** the C library's `iconv` (the function programs call to
convert text between character sets -- say from an old Japanese e-mail's
Shift-JIS into UTF-8) now handles every character set that uses one byte a
character, 221 of them, with their tables built into the library. What is
left is Chinese, Japanese and Korean, whose character sets need two or more
bytes a character and tables of thousands of entries each: about 0.7 MB for
all of them even stored compactly, one direction only. The question is
whether that megabyte goes inside every program that uses `iconv`, or into
files the library reads the first time a program asks for one of these sets.

**Terms used below.** *Statically linked*: the library's code and data are
copied into each program, as all programs here are today; there is no shared
copy on disk. *Multibyte set*: a character set with more than one byte for
some characters -- EUC-JP, Shift-JIS, EUC-KR, GBK, GB18030, Big5,
ISO-2022-JP, and IBM's double-byte mainframe sets. *mmap*: reading a file by
mapping it into memory, so every program using it shares one copy in RAM.

| Option | *What changes:* |
|---|---|
| **A.** Built in, as the one-byte sets are (design-decisions §1117): the reading half stored, the writing half built when `iconv_open` first opens the set | Every program that calls `iconv` grows by about 0.7 MB on disk, and each open multibyte converter takes about 80 KB of memory and a few milliseconds to open. Nothing else to install; works on any disk layout. |
| **B.** Table files in the system image (`/usr/lib/iconv/`, one per set), read with mmap the first time a program opens that set -- what glibc does with its converter modules | Programs stay their current size; all of them share one copy of a table in RAM. The files must be on the disk: a program on a system without them is told the set is not available, as glibc says when its modules are missing. |
| **C.** A, until the C library can be a shared library (`libc.so`, one copy for every program), then nothing more to do | Same as A today; the per-program cost disappears when shared libraries arrive -- which design.txt plans ("within one system generation, apps share .so files") but nothing has built. |

**If never answered:** nothing gets worse -- these sets are refused today, as
they have been. It blocks Chinese, Japanese and Korean conversion in every C
program (a mail reader, `iconv -f SHIFT_JIS`, a text editor opening a legacy
file). The single-byte sets are unaffected.

**Claude's recommendation:** **B.** A megabyte in every program that happens
to convert text is the wrong place for data that most of them will never
touch, and sharing one mapped copy is how every mature system does it
(glibc's modules, ICU's data file). The files are made by the same generator
that makes the built-in tables today, and lane D's image recipe installs
them. If shared libraries arrive later, B still costs nothing extra.

**Where it bites:** `posix/src/iconv.rs` (the converters, whichever way the
tables arrive), `posix/tools/gen_iconv_*.py` (the tables),
`scripts/create-ext4-rootfs.sh` (installing them, for B).

## D-Q4 — [D] Background services that run before anyone signs in need passwords too. Where should they keep them? — Status: OPEN (raised 2026-09-27)

**In short:** some programs that run in the background need a password to do
their job, and must do it while nobody is signed in: dynamic DNS (keeping a
web address such as `myhome.duckdns.org` pointed at a home network) needs the
DNS provider's password; a scheduled backup to another computer needs that
computer's password; joining Wi-Fi at startup needs the network's passphrase.
The password manager cannot give them one at startup: each user's store is
locked with that user's own master password, so until they sign in and unlock
it, nothing can read it -- even a program you have allowed to (your C-Q25
answer). These passwords need a home a background service can reach at
startup, and the choice is how well that home is protected.

**Terms used below.** *Background service*: a program the system starts at
boot, before any sign-in (the backup scheduler and the dynamic-DNS updater are
two). *Encrypted at rest*: stored scrambled, so reading the disk directly -- a
stolen laptop, or the disk moved to another machine -- shows nothing useful.
*TPM*: a security chip in most PCs that keeps a key and hands it over only to
this machine's own, unmodified startup. *Capability*: a permission a program
holds as a token, as in C-Q25's "a capability key specifically for this".

| Option | *What changes:* |
|---|---|
| **A.** A file only the service can read -- what Linux does for Wi-Fi (`/etc/wpa_supplicant/wpa_supplicant.conf`) and NetworkManager's saved networks | Works at startup, simple. The password is stored unscrambled, guarded by who may open the file: other programs on the running system cannot read it, but anyone who reads the disk directly can -- unless the whole disk is encrypted, which then covers it (the kernel has a volume-encryption module, `fs::diskencrypt`; nothing encrypts the system disk with it yet). |
| **B.** A *system* section of the password manager, unlocked at startup with a key the TPM keeps, readable by services holding a capability for it -- C-Q25's idea, extended to startup | Encrypted at rest even without whole-disk encryption, and one place to see and revoke every stored service password. Needs TPM support, which does not exist yet; on a machine without a TPM the unlocking key must sit on disk, which makes it A with extra steps. |
| **C.** No stored passwords for background services -- they run only while their owner is signed in and has unlocked the password manager, reading it through C-Q25's capability | Nothing new to protect. Dynamic DNS, backups to another computer and Wi-Fi at startup stop whenever nobody is signed in -- which for a home server is all the time. |

**If never answered:** nothing gets worse today -- no background service
stores a password yet. It blocks the part of the dynamic-DNS updater that signs
in to the provider (`requests/e-ad-dynamic-dns-is-a-userspace-service-not-a-kernel-table.md`);
the rest of it can be built meanwhile.

**Claude's recommendation:** **A now, B when a TPM-backed store exists.** A is
what every Linux system does for these same passwords, and whole-disk
encryption, once the system disk uses it, gives A the at-rest protection B
would. The service, not Settings, writes the file (Settings hands it the
password and the service checks Settings may), and each service reads its
passwords through one small function -- so moving to B later changes that
function and nothing else.

**Where it bites:** `services/dyndns` (lane D, not yet written); lane E's
Dynamic DNS page in `apps/settings/src/remote.rs`, which would hand the
password to the service rather than store it in the password manager; later,
Wi-Fi at startup (`userspace/wpa`, lane B) and backups to another computer
(`requests/e-db-the-backup-service-runs-backup-run-due.md`).

## D-Q3 — [D] Programs cannot share memory, message queues or named semaphores with each other. Where should the shared ones live? — Status: OPEN (raised 2026-09-26)

**In short:** Unix programs often cooperate through things they open by name:
a block of shared memory, a queue of messages, a named counter that makes one
program wait for another. Here each of those is private to the program that
opened it — two programs opening the same name each get their own — and the
system cannot yet give two programs the same writable memory at all. So a
database whose worker programs share memory (PostgreSQL works exactly this
way) cannot run, and a message one program queues is never seen by another.
Making them shared needs a home outside any one program; which home?

**What exists now**, all of it in the C library (libc: the library every
program links for these calls), per program:

| Family | Calls | State |
|---|---|---|
| POSIX shared memory | `shm_open` + `mmap(MAP_SHARED)` | a file under `/dev/shm`, but the kernel refuses writable shared file mappings (`ENOSYS`, design-decisions §23) |
| POSIX named semaphores (counters programs wait on) | `sem_open` | a table inside libc |
| POSIX message queues | `mq_open`, `mq_send` | a table inside libc: 8 queues of 32 small messages |
| System V (the older Unix interface for all three) | `shmget`, `msgget`, `semget` | tables inside libc |
| Locks placed in shared memory | `PTHREAD_PROCESS_SHARED` | refused (`ENOTSUP`): the kernel's futex (its wait/wake primitive) cannot wake across programs yet — requested of lane A |

**The question.** Every option below first needs the kernel to share writable
memory between programs — anonymous and file-backed `MAP_SHARED` (a mapping
two programs see the same bytes through). That part is lane A's and not in
question. What is in question is where the *named objects* live:

| Option | *What changes:* |
|---|---|
| **A.** In the kernel, as Linux does | All six families work between programs; each call is a kernel call. The kernel gains three new kinds of object. |
| **B.** In a service program (an `ipcd`) | All six work between programs; libc asks the service over the system's message channels, so every send or receive costs a round trip through it. The kernel gains nothing beyond shared memory. |
| **C.** In libc, over shared files — glibc's own design for named semaphores | All six work between programs; each object is a file under `/dev/shm`, mapped into every program that opens it, waits sleep on shared futexes. No new kernel objects and no service; file permissions decide who may open what. A program that dies halfway through an update can leave that one queue stuck, as a crashed lock holder can anywhere. |
| **D.** Leave it | Programs that use these only within themselves keep working; nothing can share them. |

**If never answered:** safe for everything in the tree today (nothing here
shares these between programs), but it blocks every port that does —
PostgreSQL, anything using `sem_open` between programs, daemons built on
message queues. It does not get worse on its own.

**Claude's recommendation:** **C.** It is how glibc already builds named
semaphores, it keeps the kernel as small as the design asks (only scheduling,
memory, IPC primitives and capabilities in the kernel), and its only kernel
needs — shared writable memory and cross-program futexes — are needed by every
option anyway. A stays available for System V message queues if a port turns
out to need kernel-side behaviour C cannot give. Meanwhile lane D keeps the
single-program versions correct.

**Where it bites:** `posix/src/mqueue.rs`, `posix/src/semaphore.rs`
(`sem_open`), `posix/src/sysv_*.rs`, `posix/src/mman.rs` (`shm_open`); lane A:
`kernel/src/mm` (shared mappings) and `kernel/src/ipc/futex.rs`
(`requests/d-a-futexes-keyed-by-physical-page-for-process-shared-objects.md`).

## F-Q3 — [F] Screenshots: how does a program get permission to read what is on the screen? — Status: OPEN (raised 2026-09-27)

**In short:** the screenshot tool cannot take screenshots, because no program
can ask the display server (the compositor, the program that draws every
window) for the screen's pixels. Adding that request is straightforward. The
hard part is who may use it: whatever is on the screen -- a password being
typed, someone's private messages -- would be readable by any program that
asks. Today the compositor cannot even tell which program is asking, so the
choice is how a program earns the right, and what the person at the screen
sees when it does.

**The options.**

| Option | *What changes:* |
|---|---|
| **A.** Ask every time | Before each capture the compositor shows its own "Screenshot wants to capture the screen -- Allow / Deny" box, which no program can draw or click for you. One extra click per screenshot. |
| **B.** Ask once per program, remember | The first capture asks as in A; after "Allow", that program captures without asking, until the permission is removed in Settings. |
| **C.** The user's own action is the permission | The compositor does the choosing: Print Screen, or its own region/window picker, captures and hands the picture to the screenshot program. A program cannot start a capture by itself at all. |
| **D.** No permission | Any program can read the screen whenever it likes, as on X11 Linux and on Windows. |

**What each means.**

- **A** needs nothing that does not exist yet, and it cannot be abused
  silently: every capture is something the person agreed to just then. The
  cost is the click, which a screen *recorder* pays once per recording and a
  screenshot tool once per shot.
- **B** is the convenient one (macOS works this way), but it depends on the
  compositor knowing *which program* is connected, which it cannot yet: the
  kernel does not pass it an identity it can trust (design-decisions §495,
  lane A's side). It also lets another program use the trusted one as a
  proxy -- for example by starting it with arguments that capture and save.
- **C** is the most secure and the smoothest for the common case (the key
  press *is* the consent -- the way Wayland desktops do it), but the tool's
  own region and window pickers would move into the compositor, and a program
  that needs pictures on its own schedule (a recorder, automation) would still
  need A or B.
- **D** is simple and dangerous: any program, including one downloaded a
  minute ago, could quietly photograph the screen.

**If never answered:** safe. The screenshot tool and screen recorder keep
saying honestly that they cannot capture; nothing else is affected. Lane F
builds the rest of the request -- the pixels, the protocol -- behind a gate
that refuses, so that answering this is the last step rather than the first.

**Claude's recommendation:** **C, with A for programs that capture on their
own schedule** -- the person's own key press or pick is the permission, and a
recorder asks once per recording. B once the kernel can say which program is
connected, if the prompt proves tiresome. Not D.

**Where it bites:** lane E's request
`requests/e-f-an-application-cannot-read-the-screen-so-no-screenshot-can-be-taken.md`;
`gui/remote` (a capture request), `gui/compositor` (its handler, and the
prompt or picker), `gui/window` (the call an application makes);
`apps/screenshot` and the screen recorder on lane E's side.

## F-Q1 — [F] iPhone photos (HEIC) will not open. May SlateOS include a decoder for a patented video format? — Status: OPEN (raised 2026-09-25, narrowed 2026-09-27)

**In short:** an iPhone saves every photograph as HEIC, and SlateOS cannot
open one. Opening one means decoding HEVC, a video format covered by patents
that their owners license for a fee. That fee is why Windows sells HEIC support
separately for $0.99 and why Fedora Linux leaves it out. The question is
whether SlateOS should include such a decoder, offer it as a separate install,
or neither. (The other half of this question, AVIF, you answered "yes":
design-decisions.md §1333.)

**Your question: what is hard about "letting users replace the library"?**
You are right that it is nearly trivial. The usable open decoder, libde265, is
under the LGPL (a licence that lets anyone ship it, on one condition). The
condition is that a user must be able to swap in their own build of that one
library. The two things that could get in the way both have easy answers:

- **Rust bakes libraries into each program.** Linked that way, the decoder
  would sit inside every program that shows a picture, and we would owe users
  a way to rebuild all of them. So it goes in a separate library file or
  helper program instead. That is easy, and the helper program is the safer
  design anyway: a crafted picture that attacks the decoder then attacks a
  helper that can do nothing else.
- **This version of the LGPL (3) also forbids locking the user out.** If
  SlateOS ever insists that only software it signed may run, a user's own
  build of the decoder must still be allowed to run. As long as users can
  always add their own signing key, this costs nothing.

So the licence is not the obstacle. The **patents** are, and they are a legal
and policy question rather than an engineering one.

**The options** (the engineering is the same for all three up to the last
step: the decoder is built as a separate, replaceable helper, and the only
difference is whether a fresh install includes it):

| Option | *What changes:* |
|---|---|
| **A.** Include it | iPhone photos open out of the box. If SlateOS is ever sold or distributed widely, the HEVC patent pools may ask for royalties. |
| **B.** A separate install, one click away | The first time a HEIC file is opened, SlateOS offers to install "HEIC support". The base system contains no HEVC code; whoever installs it takes on the patent question, as with Windows' paid extension. |
| **C.** Not yet | iPhone photos keep saying they cannot be displayed. |

**If never answered:** safe. HEIC files show an error saying they cannot be
displayed, as now; nothing else is affected.

**Claude's recommendation:** **B**. Almost A's convenience, with the base
system free of the one kind of code that carries a fee. If SlateOS will only
ever be used privately, A is just as good, and simpler.

**Where it bites:** `gui/imagecodec` (the format dispatch; the HEIF container
reader §1333 builds for AVIF serves HEIC too), a helper program for the
decoder, and the image viewer's "cannot display" message.

## F-Q4 — [F] AVIF pictures open about twice as slowly as in a browser. Use the browsers' hand-written assembly, or write the fast parts in Rust? — Status: OPEN (raised 2026-09-27)

**In short:** AVIF pictures decode correctly but take roughly twice as long
as in Chrome or Firefox: a full-HD photograph takes about 0.2 seconds here
against 0.09 there. The difference is that the browsers' AV1 decoder (the
part that unpacks the picture) runs about 160,000 lines of hand-written
x86 assembly (instructions for the processor, written directly rather than
compiled), and ours was brought in without it. We can bring that assembly
in -- the browsers' speed at once, but code that Rust's safety checks cannot
look at -- or rewrite the fastest parts ourselves in Rust.

**Measured** (`imagecodec`'s `bench_avif_decode`, one thread, best of five,
on a machine busy with other builds, so treat the figures as rough):

| Picture | Here (Rust only) | Pillow (the browsers' decoder, with assembly) |
|---|---|---|
| 640x480 | 30 ms | 22 ms |
| 1920x1080 | 199 ms | 88 ms |
| 2560x1440 | 354 ms | 161 ms |
| 1920x1080, 10-bit, full colour | 353 ms | 202 ms |

**The options.**

| Option | *What changes:* |
|---|---|
| **A.** Bring in the assembly | Pictures open as fast as in a browser, straight away. SlateOS gains ~160k lines of x86 assembly (rav1d 1.1.0's own count) that the Rust compiler cannot check -- the same code Chrome, Firefox and Android run on every AV1 image and video, and among the most tested there is. Needs the NASM assembler on the build machine (a small install). |
| **B.** Write the fast parts in Rust | Each of the handful of hottest routines is rewritten with the processor's vector instructions from Rust, checked sample-for-sample against the plain version. Stays in checkable Rust, but it is a long piece of work, and the speed arrives routine by routine. |
| **C.** Leave it | Pictures stay about twice as slow as in a browser: fine for a still picture, noticeable for large ones and for AVIF animations. |

**If never answered:** safe, and nothing is blocked: pictures open, just
more slowly (option C). It does not get worse over time.

**Claude's recommendation:** **A.** The assembly is the most exercised code
in its field, it produces exactly the same pixels as the Rust (dav1d's own
tests hold the two to each other), and B would spend a long time reaching
what A gives at once. B stays possible later for any routine that proves to
matter, one at a time. The one real cost is the unchecked code; if keeping
every decoder in checked Rust matters more to you than speed, B.

**Where it bites:** `gui/video/rav1d` (its `asm` feature and the build step
that assembles it, left out when it was vendored -- `VENDORED.md`),
`known-issues.md` "[F] AVIF decoding has no committed benchmark", and every
AVIF picture or animation on the system.

## A-Q14: When we keep a previous copy of a file, should it be the content from *before* that save, or *after* it?

**In short:** the system can keep old copies of a file so you can go back to one.
You have already told us (A-Q10) to stop doing that work *while* a save is
happening and do it just after, so saving feels fast. Doing it afterwards has a
consequence we want you to confirm rather than decide for you: once the save has
finished, the previous content is already gone, so the copy we keep would be the
**new** content instead of the old one. You can still go back either way -- the
question is what each stored copy contains.

**Why there is a choice at all.** Today the copy is taken before the save
overwrites anything, which is why it holds the old content. Moving the work after
the save means the old content is no longer there to read. We can either accept
that and store the new content, or hold the old content in memory across the save
so we can still store it.

**The options:**

* **A. Store the content as it stands after each save.**
  *What changes:* after three saves you can recover the file as it was at save 1
  and save 2; save 3 is the file itself. "Undo my last save" still works. A crash
  in the moment right after a save loses the newest entry only. Nothing is held in
  memory.
* **B. Copy the old content into memory during the save, and do the slow part (the
  checksum) afterwards.**
  *What changes:* exactly what you can recover today, unchanged. The save gets
  most of the speed-up, because the checksum is the slow part, not the copy. The
  cost is that while a large file is being saved we briefly hold a second copy of
  it in memory -- for a very large file that is a real amount of memory, and it is
  memory the kernel cannot decline to find.

**Recommendation: A.** It is what your A-Q10 answer literally says ("the read-back
and checksum happen after the write has returned"), it holds nothing extra in
memory, and the thing you actually want -- going back to an earlier state -- works
under both. B's advantage is only that the stored copies line up with what the
feature stored before, which matters to nobody who has not read the code.

**The two halves of your own sentence point opposite ways, which is the real
reason this is being asked.** The A-Q10 answer describes the feature as "every
save currently reads back *the old contents* and checksums them" -- and then says
that read-back moves to after the write returns. Once the write has returned the
old contents are gone, so the two halves cannot both hold. Option A keeps the
second half and gives up the first; option B keeps the first and gives up part of
the second, doing the copy during the save and only the checksum afterwards.
Nothing about that was obvious when the answer was given, and it is not a
reversal of it -- it is the one detail the answer could not have anticipated.

**One honest flag against my own recommendation.** A exists in the codebase as a
test that asserts the opposite: after writing v2, the history must contain v1.
Under A that test's meaning changes. All session I have treated "a test whose
assertion flips" as a sign that an invariant was quietly redefined, so I am not
going to flip it on my own judgement, which is why this is here rather than
decided in passing.

**If this is never answered:** nothing breaks and nothing is at risk. A-Q10's
first half is already in -- the history is off unless a directory is enrolled, so
almost nothing pays for it. What stays unfinished is only the "do it after the
save" half, so any directory that *is* enrolled keeps paying the old cost during
its saves. It does not get worse with time.

*Filed 2026-09-14 by lane A. Bites at `kernel/src/fs/history.rs` --
`try_auto_record`, and the Test 7 block in that file's `self_test`.*

## A-Q15: A program can only have one network connection open at a time. Which way should we fix it?

**In short:** a program that opens two network connections at once — a web browser
fetching two images, a server talking to two visitors, anything ordinary — does not
work. Opening the second one silently destroys the first. Nothing has noticed until
now because every test we have opens one at a time. The fix is real work either
way, and the two ways are quite different in size and in who does them.

**What is actually happening.** Network traffic is handled by a separate helper
program (the "network daemon"), and the kernel talks to it through a shared block of
memory — a "ring". Each socket the kernel opens allocates **its own** ring. The
daemon, however, keeps only **one** ring mapped at a time: when it sees a different
one it throws away everything it knew about the previous one, including which
programs were waiting for connections. Its own source comment says this is
deliberate. So socket two wipes socket one.

**How it was found.** A test written to prove an unrelated fix was the first thing in
the tree that needed two sockets alive at the same time. It failed on three
consecutive boots. The first two explanations were wrong; the third was found by
reading the daemon's code.

**The first casualty is the GUI, not networking, and it fails silently somewhere
else.** On SlateOS the window system's own connection is a network connection:
`gui/remote/src/socket.rs` is built on `TcpListener`/`TcpStream`, and its doc
says the listener "is the compositor's end". So for any windowed program, the
display connection **is** socket number one.

That means the program that opens a second socket does not see the second one
fail. It sees its **window** die — the display connection is what the daemon
tears down. Thirteen apps call `oswindow::app::launch` today and none opens a
second socket, so nothing is broken right now. The first one that fetches
anything would be reported as "the browser closes itself when it loads a page",
and the fault would be hunted in the browser, or the compositor, or the window
system — anywhere but the network daemon that actually did it.

This is the strongest argument for fixing it before something needs it: the
symptom appears in a different subsystem from the cause, so the day it bites it
costs somebody a long hunt in the wrong place.

**The options:**

* **A. One ring shared by all sockets.** The kernel allocates a single ring at
  start-up and every socket uses it, tagging its messages with its own id.
  *What changes:* two connections work. Sockets stop being independent of each
  other — one very busy connection can make others wait, because they share one
  queue. Work is in the kernel, lane A.
* **B. The daemon keeps several rings mapped.** It holds a table of rings instead of
  one, and serves whichever a message arrives on.
  *What changes:* two connections work and stay independent. More memory per
  program, and a fixed ceiling on how many can be open. The work is in
  `services/netstack`, which is **lane B's** (`roadmap.md` line 156, and the
  lane table in `CLAUDE.md`) -- named explicitly because an option addressed to
  no particular lane is how two request files sat for ten days this month, each
  recording the other lane as owner.
* **C. Both, later.** Ship A now because it is one lane's work and unblocks
  everything, and revisit B if one connection starving another turns out to matter
  in practice.
  *What changes:* the same as A today, with a note to look again.

**Recommendation: C**, with A as the thing actually built now. A is smaller, lives in
one lane, and can be done without coordinating two trees. The independence B buys is
real but theoretical here: nothing in this OS yet drives enough traffic for one
connection to starve another, and if that day comes the measurement will say so.

**A named program is already queued behind this, so "nothing is broken today"
is too generous.** Nothing is *red*, which is a different claim.

The emoji picker stores a chosen emoji in a field whose doc comment says it is
"for clipboard / IPC output". That program contains no clipboard code and no
IPC code. The clipboard *service* exists -- `gui/clipboard`, whose own doc says
"all applications communicate with this service via IPC" -- but its `src/`
holds a single `main.rs` with no library beside it, so nothing can link to it,
and the desktop's clipboard viewer claims to integrate with it while containing
zero connects, sends or sockets. (Both checked here, not taken on report.)

The fix is a client that talks to that service, and that client opens a socket.
Under this bug that is socket number two, and socket number one is the
compositor connection. So the first program to reach for the clipboard does not
get a failed clipboard call -- it gets its window destroyed. **"The emoji picker
closes itself when I click an emoji"** is the bug report, and the hunt goes to
the picker, then the compositor, then the toolkit, and never to the network
daemon.

That work is queued now rather than hypothetical, which is the difference
between "fix this before something needs it" and "fix this before the next
thing ships broken".

**If this is never answered:** networking keeps working exactly as well as it does
today, which is one connection at a time. Nothing breaks that was not already
broken, and no data is at risk. What stays blocked is anything needing two at once —
a server accepting while serving, or a program fetching two things in parallel — and
the concurrency fix in `known-issues.md` `D-NETSOCK-SYNC` cannot be proven at all,
because the test that would prove it needs two sockets.

**Update 2026-09-15 — this no longer holds the boot red, and that is a change in
urgency, not in the question.** Until today the head-of-line self-test FAILED on
this limitation, so every boot was red and nothing could be merged to `main`
until you answered. That was my choice and I have reversed it
(`design-decisions.md` 941): three unrelated fixes had accumulated behind it,
including a kernel self-deadlock that has nothing to do with sockets.

The test now prints `NOT CHECKED`, names this question, and does **not** claim
the property passes. A second check fails the build if the limitation ever goes
away without anyone noticing, so nothing is quietly retired by the change.

What this means for you: **there is no longer any schedule pressure on this
answer.** Take it on its merits. The cost of leaving it open is unchanged --
one connection at a time, and `D-NETSOCK-SYNC` unprovable -- but it no longer
costs the other two lanes their merges.

*Filed 2026-09-14 by lane A. Root cause and evidence are in `known-issues.md` under
the head-of-line witness entry: `socket.rs:356`, `netstack_client.rs:158`, and
`services/netstack/src/main.rs:2594`.*

## C-Q31 — [C] You suggested a lane say when it starts work outside its own part of the tree. The tool exists -- may `CLAUDE.md` make it a rule? — Status: OPEN (raised 2026-09-27)

**In short:** answering C-Q20 you suggested that whenever a lane takes on a task
the roadmap does not clearly give it, it should write that down where the other
lanes will see it, so two lanes do not build the same thing. There is now a
small tool for exactly that: a lane records "I am doing X" and every other lane
sees it at once, without waiting for anyone's work to be merged. A tool nobody
is told about goes unused, and the place every lane is told things is
`CLAUDE.md`, which changes only on your word. So the question is whether to add
the paragraph below.

**The tool:** `scripts/lane-claims.py`. A claim is a small file in the one
folder all six lanes share -- where "stop everything" halts already live -- so it
is seen the moment it is written. It is a notice, not a lock: nothing is refused
because of it. Claims older than a week are shown as stale, not hidden.

**The paragraph proposed**, for `CLAUDE.md` under "Six Sessions", after the
paragraph about `requests/`:

> **Before starting a task the roadmap does not clearly give your lane, check
> and claim it.** `python scripts/lane-claims.py --check <the paths you will
> touch>` shows whether another lane has claimed them; if not, `python
> scripts/lane-claims.py --claim "<what>" --paths <paths>` tells every lane at
> once, and `--release <what>` when it is done or dropped. A claim is a notice,
> not a lock.

| Option | *What changes* |
|---|---|
| **A. Add it as written** (recommended) | every lane checks for and records a claim before out-of-territory work |
| **B. Add it, worded your way** | the same, in your words -- tell me the change, or edit it in yourself |
| **C. Leave `CLAUDE.md` as it is** | the tool exists and is used only by lanes that remember it |

**If it is never answered:** nothing breaks and nothing is blocked; the tool is
there and lane C uses it. What is lost is the protection it exists for, because
a claim only helps if the lane about to duplicate the work thinks to look.

## C-Q29 — [C] Copying in one program and pasting in another works nowhere. Should copy and paste travel through the window system? — Status: OPEN (raised 2026-09-26)

**In short:** nothing you copy can be pasted into a *different* program. Each
program keeps a private clipboard of its own, and the system's clipboard
program exists but nothing can reach it (`known-issues.md`
`TD-C-FIFTEEN-PRIVATE-CLIPBOARDS-AND-A-SERVICE-NOBODY-TALKS-TO`). Building the
missing link means choosing *how* a program talks to the clipboard, and there
are three ways. One is through the connection every program with a window
already has to the window system -- the way Linux's Wayland and macOS do it.
Another is a second, separate connection to the clipboard program. The third
is the operating system's core keeping the clipboard itself, which programs
would reach by asking the core directly. Only the first also carries dragging
things between programs, which the design asks for.

**Terms, once each:** *the window system* -- the compositor, the program that
draws every window and passes each its mouse and keyboard; *the clipboard
program* -- `gui/clipboard`, which keeps what was copied, with a history;
*A-Q15* -- the open question above: a program could hold only one network
connection at a time, and on SlateOS the window system's connection is that
one, so a second connection killed the program's window. Lane A has built the
fix (2026-09-27, being boot-tested); once it reaches the shared branch, a second
connection is safe. *The kernel* -- the operating system's core, which every
program can ask for things directly through *system calls* (requests a program
makes of the core itself). It already keeps a clipboard of its own
(`kernel/src/fs/clipboard.rs`: text, and a list of files), which today only the
kernel's own command shell can reach.

| Option | What changes |
|---|---|
| **A. Through the window system** (recommended) | Copy, paste and dragging between programs can all be built now. The window system carries a program's offer of data -- as text, formatted text, a picture or a file -- and the clipboard program keeps the history. |
| **B. A second connection to the clipboard program** | Copy and paste waits only on lane A's A-Q15 fix reaching the shared branch -- built, not yet published. Dragging between programs still has to go through the window system, so there are two ways of moving data between programs instead of one. |
| **C. The kernel's own clipboard, through new system calls** | Copy and paste between programs with no connection at all, and lane A's work alone (asked for by lane E: `requests/e-a-a-clipboard-door-for-applications.md`). But the kernel cannot tell which window you are using without asking the window system, so it cannot stop a program in the background from reading what you copied; it holds text and file lists, not pictures or formatted text; and dragging still needs the window system -- two ways, as in B. |
| **D. Leave it** | Copy and paste keeps working within each program and never between two. |

**Why A.**
- Dragging needs the window system whatever is chosen. Only the window system
  knows which window is under the pointer when you let go, so A is one
  mechanism for both; B and C are each a second one beside it. `design.txt`
  line 735 asks for exactly that: "a clipboard/drag-and-drop system that
  supports multiple data formats per operation".
- The window system knows which window you are using, so it can refuse a
  program in the background that reads the clipboard behind your back. Under B
  or C the clipboard would have to ask the window system anyway.
- It waits on nothing. (B's wait, on A-Q15, is nearly over -- lane A has built
  the fix -- so this matters less than it did when the question was raised.)

*Updated 2026-09-27 from lane A's note,
`requests/a-ce-the-clipboard-transport-is-c-q29-and-the-kernel-clipboard-is-a-third-option.md`:
option C added, and B's wait restated. Lane A will not add the system calls for
C ahead of your answer, since whichever transport is built first becomes how
every program copies.*

**Why you are being asked:** whichever way is chosen is how every program will
copy, paste and drag, for good -- a program written for one cannot use the
other without being changed -- and it splits work between lanes differently.
A needs lane F to add the offer and the transfer to the window connection, and
lane C to make the clipboard program the keeper of the history, with a small
client for programs; C is lane A's system calls, with the window system still
needed for dragging.

**If this is never answered:** nothing gets worse. Copy and paste keeps
working inside each program and never between two, and the emoji picker still
has no way to give you the emoji you pick.

## C-Q28 — [C] The start button should be the XOR logo, but the logo is not in the repository. Can you add it? — Status: OPEN (raised 2026-09-26)

**In short:** the design says the start button is "a round, shrunken version
of the XOR logo (`xor2.png`)". No file of that name is in the repository, on
any branch, so the start button draws a stand-in: a plain four-square picture
from the built-in icon set. It is not a decision to make so much as a file to
supply.

**What would happen with it:** the logo would be drawn once as a small SVG
(a vector picture, so it stays sharp at every size) and shipped as the
built-in theme's `start-here` icon -- the name icon sets use for "the start
menu" -- so any theme can still draw its own instead.

| Option | What changes |
|---|---|
| **A. You add `xor2.png` (or an SVG of it) to the repository root** | The start button becomes the logo, drawn to match. |
| **B. The four squares stay** | Nothing: the stand-in remains the start button. |

**If this is never answered:** nothing breaks and nothing gets worse. The start
button works and looks like a start button, just not like this system's own.

# Resolved` index at
the bottom under your own lane's subheading. An answered question left in the
body is pure clutter, and because it is older it sorts *first* — directly in
front of the questions that still need an answer, which is the one thing this
file exists to show. (This file is lane-*partitioned*, not append-only; the
reasoning is `design-decisions.md` §437 and the rule is `roadmap.md` →
"Three-Agent Parallel Execution" rule 3.)

New questions go at the end of the body, just above the `---` that precedes
the `# Resolved` index, numbered with your lane's prefix (`A-Q<n>`, `B-Q<n>`,
`C-Q<n>`). The unprefixed `Q<n>` numbers are pre-split and are not to be
extended.

**Read that last paragraph twice — it is the rule this file gets wrong.** "At
the end of the body" is not the end of the file, and appending to the end of
the file lands you *below* `# Resolved`, among the answered questions, where
the operator will never reach you. Three lanes have now done exactly that, and
eight entries had to be moved back. It is not carelessness: the end of the
file is simply where a text editor puts you, and the archive looks like the
place new things go because it is last.

`scripts/check-open-questions.py` enforces it — run it before you commit, or
let `scripts/boot-test.sh` run it for you. It **fails** the build on a question
filed below the boundary, on a body entry whose `Status:` is no longer `OPEN`,
and on two entries sharing an identifier while one is still open. It only
**warns** about a missing `C-Q<n>`-style identifier and about the two historic
duplicate numbers in the archive, both of which are another lane's text to fix
or history's to keep. Reasoning: `design-decisions.md` §903.

## A-Q13: Eight times in two days, one agent's push has cost another agent a 20-minute test run. Should pushing be gated?

**In short:** the three agents share one trunk. When one pushes something broken,
nothing notices until another agent runs the full test cycle -- which takes 20 to
40 minutes and fails partway through. That has happened eight times in two days,
and each time the agent who paid was not the one who caused it. The question is
whether pushing should have to pass something first, and if so what.

**Glossary.** *Gate* -- an automatic check that can refuse. *Boot test* -- the full
cycle: ~130 checks, a kernel build, then booting it in an emulator; 20-40 minutes.
*Pre-push hook* -- checks that run on the pushing machine before a push is allowed;
seconds to minutes.

**The evidence, all from 2026-09-12 to 09-14.** Eight breakages arrived on the
trunk and were found by a later agent's run:

| what | found after | would a pre-push check have caught it? |
|---|---|---|
| a shell quoting fault (`SC2046`) | ~520 s | yes -- the check exists, but only in the boot |
| a file read that hid three failures as one | ~520 s | yes, same |
| code reading two fields that did not exist yet | ~1800 s | yes, a compile error |
| eight file writes with the wrong line endings | 13 s and 16 s (twice, different files) | yes, same check, boot-only |
| a list claiming to hold every case while missing one | 321 s | yes, same |
| a shell fault in a *different* agent's tree | ~1895 s | yes, a compile error for another platform |
| **two checks that were themselves wrong** | 198 s, 262 s | no -- these were false alarms |

**Four were real, two were false alarms from checks I have since fixed.** That
ratio matters for the answer: adding more gating without fixing the gates buys
more false alarms, and an agent who learns to discount a red result is worse off
than one who never had the check.

**The thing that surprised me, and it rules out the obvious answer.** Lane C
already runs the whole test suite before every merge -- about 6-7 minutes -- and it
caught *neither* of the two faults in lane C's own tree. One was a compiler warning
for a different platform; the other was a separate check written in Python. So
"the agent tested before pushing" and "the trunk still works" are different
claims, and the first has been quietly standing in for the second.

**Why we cannot simply require the full cycle.** The trunk takes roughly 49
merges a day (489 in ten days, 86 on the busiest). The full cycle is 20-40
minutes and only one can run at a time on this machine. The arithmetic does not
close: requiring it would cap the project at a handful of merges a day.

**The options:**

* **Move the fast checks to push time.** Several of the checks above already
  exist and run *only* in the full cycle, for no reason anyone recorded. They
  take seconds.
  *What changes:* the agent who writes the fault sees it in seconds instead of a
  different agent seeing it 20 minutes later. Five of the six real faults above
  would have been caught this way. Costs a few seconds per push.
* **Require the full cycle before merging to the trunk.**
  *What changes:* the trunk is never broken; the project does a handful of merges
  a day instead of fifty. This is the strongest guarantee and the one the
  arithmetic refuses.
* **Change nothing; the agents keep absorbing it.**
  *What changes:* nothing. Eight runs in two days were spent on this, and the
  cost falls on whoever runs the cycle rather than whoever caused the fault, so
  no agent sees their own cost.
* **Fix the checks first, then decide.**
  *What changes:* nothing immediately. Two of eight alarms were the checks being
  wrong; that rate is worth lowering before making them block more.

**Recommendation: the first, and it is already half-blocked on A-Q11.** The
checks exist, they are fast, and they are deterministic -- the only reason they
run late is that nobody moved them. But putting them in the pre-push hook means
editing `scripts/hooks/pre-push`, which is the file A-Q11 asks about, and two
agents each believe it is theirs. I proposed one such move to lane B by notice
and deliberately did not make it. **Answering A-Q11 unblocks this.**

**If this is never answered:** nothing degrades, but the cost continues at
roughly four boot runs a day of wasted work, charged to whichever agent runs the
cycle. Lane C has seen the tally and seconds this question rather than filing a
separate one.

*Filed 2026-09-14 by lane A, with lane C's agreement. Lane C contributed the
measurement that its own pre-merge suite caught neither of its own faults, and
that a lane's gate has nothing scheduling it apart from another lane's boot --
its slowest run today, 578 s of 399 s mean, was slow because my boot was running.*

---
## A-Q11: Who owns `scripts/hooks/pre-push`?  Two lanes each believed they did, and both edited it the same night

**In short:** the tool that tells each agent which files it may edit does not mention
two files, and they are the two that sit between agents by nature: the script that runs
before any agent uploads work, and the script every agent's code is checked by. Two of
the three agents each concluded one of those files was theirs, and both edited it the
same night. Nothing broke, by luck. **The part that is still a hazard after those two
have stopped disagreeing: a third agent reading that tool would conclude it may edit
either file freely.**

The longer version: there is a script that runs automatically before any agent uploads work,
and it decides whether the upload is allowed. Tonight two of the three agents each
believed that file was theirs to edit, and both edited it within a few hours. Nothing
broke, because their changes happened not to touch the same lines. The tool that is
supposed to say who owns what does not mention the file at all.

**The evidence.** `scripts/which-lane.py` is what every agent consults, and what a new
session would consult:

* Lane A owns `kernel/**`, `bench/**`, `toolchain/x86_64-slateos.json`,
  `scripts/boot-test.sh`, `scripts/run-timeout.py`, `scripts/wedge-soak.sh`.
* Lane B owns `posix/**`, `userspace/**`, `services/**`, `init/**`,
  `toolchain/stubs/**`, `toolchain/build-sysroot.ps1`, `scripts/create-ext4-rootfs.sh`.
* Lane C owns the `gui/**`, `apps/**`, `net*/**` families.

Neither `scripts/hooks/pre-push` nor `scripts/coreutils-check.sh` appears in any lane's
owns list or any lane's never-writes list: `grep -c 'hooks/pre-push\|coreutils-check'
scripts/which-lane.py` returns **0**, in both lane A's tree and lane B's.

Lane B states their own instructions enumerate their write scope as the seven paths above
**plus `scripts/hooks/pre-push` and `scripts/coreutils-check.sh`**, and separately state
that `scripts/boot-test.sh` is lane A's. That is the whole of their claim and they infer
nothing further from it. Lane A's instructions name neither file; lane A inferred the
hook from owning "the boot test", which was an inference and not a reading.

**Why the omission is probably not random**, which is lane B's observation and the most
useful thing either lane found here: the table enumerates *trees* — `kernel/**`,
`posix/**`, `gui/**` — and these two files are not trees. A push hook every lane pushes
through and a check script every lane's crates go through have no tree to belong to, so
a tree-shaped table has nowhere to put them. That suggests the fix is a rule for
cross-cutting files rather than two more entries.

The omission is not a stale checkout. `git show origin/lane-b:scripts/which-lane.py`
diffed against lane A's copy: identical. It is a gap in the shared table that two lanes
filled with opposite answers.
list. Lane B reports that their own private instructions name it as theirs; lane A
inferred it from owning "the boot test". Their copy of `which-lane.py` is byte-identical
to lane A's, so this is not a stale checkout — it is a gap in the shared table that two
lanes filled with opposite answers.

**What actually happened, since it is the reason this is worth your time.** Lane A made
six edits to that file tonight (renaming a gate, widening it by five gates, moving its
summary, correcting its inventory). Lane B made one, and flagged the mismatch rather
than proceeding quietly. No collision occurred. `CLAUDE.md` names exactly this as "the
most expensive failure mode in this arrangement", and the only thing that prevented it
was which lines each happened to touch.

**The options:**

* **Assign it to lane B.**  *What changes:* lane A files a request for any hook change;
  since lane A owns `boot-test.sh` and most gates are wired in both, many changes would
  become two-lane handshakes.
* **Assign it to lane A.**  *What changes:* the reverse, and it sits oddly with lane B's
  own instructions, which they should not have to contradict to follow the table.
* **Declare it shared, with a rule.**  *What changes:* both may edit it; the rule has to
  say how (e.g. append-only per gate, as the shared documents already work), because
  "shared" without a convention is what produced tonight.
* **Answer the general case instead.**  *What changes:* `scripts/**` has roughly 120
  files and the table names six of them. Whatever is decided for the hook, the same
  ambiguity covers every unnamed script, and a rule for the directory would settle more
  than one question.

**If this is never answered:** the lanes keep editing it on opposite assumptions. The
failure is silent and occasional — two lanes touching the same region in one night — and
when it happens the loser's change disappears without either noticing, because git
merges a non-overlapping edit cleanly and nobody is watching that file for intent.

**What each lane is doing until this is answered**, recorded so the asymmetry is visible
rather than looking like one lane conceding. Lane B continues to edit the file, because
their instructions name it and they should not act against their own instructions on a
peer's reading — and they announce each edit first, so a collision cannot happen
unnoticed while this is open. Lane A has stopped, because nothing in lane A's
instructions authorises it: the difference is not politeness, it is that one lane has a
source and the other had an inference. Lane B has offered to make any hook change lane A
needs in the meantime, which is faster than a request queue.

*Raised by lane A 2026-09-12 after lane B flagged the mismatch. Lane A is not a neutral
party here and offers no recommendation between the first two options.*

## A-Q16 — [A] Two kinds of lock in the kernel; one skips the deadlock checker, for a reason that turns out not to be true. Which way should that be settled? — Status: OPEN

**In short:** the kernel has a cheap lock and an expensive lock. The
expensive one is watched by a deadlock detector; the cheap one is not, and
the stated reason it does not need watching is that nothing is ever locked
*inside* it. A new check measured that: it happens **1256 times per boot**,
in at least 24 places. Nothing has actually deadlocked, and the code is
probably fine -- but the reason we believed it was fine was wrong, and the
choice is whether to pay to find out properly.

**Glossary, because none of this is guessable.** A *lock* stops two pieces
of code touching the same data at once. A *deadlock* is two pieces of code
each holding what the other needs, so both stop forever -- the classic cause
is taking two locks in opposite orders. *lockdep* is the built-in detector
that watches lock orders and complains about a possible deadlock even when
one has not happened yet. A *leaf* lock is one that never takes another lock
while held; leaf locks cannot participate in an ordering deadlock, which is
why skipping the detector for them is sound.

**Where it bites.** `design-decisions.md` §70 split the kernel's locks in
two: `PreemptSpinMutex` (cheap, no detector, 489 uses, for "hot leaf
locks") and `crate::sync::Mutex` (detector + statistics). The §70 text says
ordering checks "add no value" for the cheap type *because* nothing nests
inside it. §949's new check measured 1256 nested acquisitions per boot
across ≥24 site pairs, including cross-module ones
(`ipc/completion` -> `proc/thread`, `fs/cgroupfs` -> `cgroup`).

One real instance was already fixed today: `INOTIFY_TABLE` documented its
own lock order in prose *and* used the untracked type, so the order it
documented could not be enforced. It is now the tracked type and lockdep
confirms the order holds.

| option | *What changes:* |
|---|---|
| **(a) Convert the non-leaf ones** (recommended) | the detector watches the orderings behind 89 distinct site pairs (measured 2026-09-17; the "12-15" here previously was extrapolated from a saturated cap); a real inversion becomes a loud boot failure instead of a hang. Costs per-acquire tracking on those paths. |
| (b) Restate §70 honestly, accept the risk | nothing changes at runtime; §70 stops claiming a reason that is false and says the type is chosen for cost with ordering unchecked. The 1256 stay unwatched. |
| (c) Convert only the cross-module pairs | the five cross-module orderings get watched; the same-module init-guard idiom (the bulk) stays as it is. |

**My recommendation: (a), scoped by measurement rather than all at once.**

**Measured after this question was filed, so the cost is no longer a
guess.** There was no `PreemptSpinMutex` arm in `bench_lock_primitives`
until 2026-09-17; there is now, and five boots agree:

| | bare `spin::Mutex` | `PreemptSpinMutex` | `crate::sync::Mutex` |
|---|---|---|---|
| typical | 26ns | **160ns** | **395ns** |

So converting one of these locks costs roughly **235ns per acquire**, and
that 235ns is identifiable work -- lockdep, contention statistics, and two
`rdtsc` reads -- rather than a general penalty for touching the acquire
path. Which means the decision is per-lock and answerable: a lock taken
once per boot costs nothing worth discussing, and one on a syscall path
might.

Two caveats on those numbers. They are QEMU TCG figures, so the *ratios*
transfer and the nanoseconds do not. And one of the five boots reads
28/268/719 -- uniformly higher across all three arms, so it is a slow boot
rather than a slow lock, and is excluded rather than averaged in.

A related measurement, because it bears on whether instrumenting these
locks is inherently costly: the §949 leaf check adds three atomic
operations to that same acquire path, and its cost is **below the noise
floor** -- 166ns without it against 159/162/159 with it, the instrumented
runs being the faster ones. So the 235ns is not "what it costs to touch
this path"; it is what lockdep and statistics specifically cost.

So: convert, read the arm, and revert any conversion that costs more than
it is worth. The arm now exists to read.

**If never answered:** the current behaviour is safe as far as anyone can
tell and has been for months, so nothing breaks tomorrow. What degrades is
that every new nesting added inside one of these 489 locks is equally
unwatched, and the check now reports 24 of them at its cap on every boot --
so the noise grows and the signal for a genuinely new one gets harder to
see.


## A-Q17 — [A] Moving or scaling a video/cursor layer silently does nothing. Should the kernel refuse the request, or start honouring it? — Status: OPEN

**In short:** the display hardware can draw a picture as a layer and place
or stretch it anywhere on screen -- that is how a video overlay or a mouse
cursor gets positioned without redrawing everything. A program asks for a
rectangle, the kernel stores the numbers, replies success, and never uses
them. So moving or resizing that layer does nothing at all, and the program
is told it worked.

**Glossary.** A *plane* is one such hardware layer. *Atomic modeset* is the
interface a display program uses to change several display settings at once,
so they either all take effect together or none do -- it is the modern way
Linux programs talk to a graphics driver. *Scanout* is the hardware
continuously reading a framebuffer to send pixels to the monitor.

**The measurement.** `drm/plane.rs` declares `src_x/y/w/h` (the region of
the picture to take) and `dst_x/y/w/h` (where to put it on screen). Every
occurrence of `dst_w` in the whole kernel is the declaration or one of five
writes; there are **zero reads**. One of those writes is
`drm/atomic.rs:423`, the atomic commit handler storing what a client asked
for. Nothing in the scanout path consults any of it.

| option | *What changes:* |
|---|---|
| **(a) Refuse a commit that sets a non-default rectangle** (recommended) | a program that tries to move a layer gets a clear error instead of false success. Programs that only ever use the full-screen default are unaffected. |
| (b) Honour the rectangle in the scanout path | layers can actually be placed and scaled -- the feature works. This is real driver work, per backend, and needs hardware to verify. |
| (c) Leave it, and document it | nothing changes; `/proc` and the API keep reporting success for a no-op. |

**Why this needs you and not me.** (a) is a user-visible behaviour change to
an API that Linux programs use by construction. Anything currently setting a
rectangle gets success today and an error afterwards -- and "currently
works" is doing a lot of work in that sentence, because what it means is
"currently appears to work while doing nothing". That is a trade between two
kinds of wrong, and which one is worse depends on what you want the OS to
be honest about.

**My recommendation is (a)**, on design-decisions 945: a simulated action
should be disclosed where its result is read, and an API return value is
where this one is read. There is no `/proc` header to put a note in, so the
only honest disclosure available is the error. (b) is the right end state
and is not blocked by (a) -- refusing now does not make honouring it later
harder.

**If never answered:** nothing breaks today, because nothing in the tree
sets a plane rectangle. It gets worse with time in a specific way: the first
real compositor to try it will spend a while looking for a bug in its own
code, since every call it makes returns success.


## A-Q18 — [A] Three lanes all append to the end of `known-issues.md` and it conflicted eleven times today. Should it get a per-lane seam like the other shared documents? — Status: OPEN

**In short:** the three agents keep one shared file of known bugs. All three
add new entries to the bottom, so any two that write between merges collide
at the same spot. Today that happened eleven times. Every collision is
trivial to fix -- keep both entries -- but each one stops a build pipeline
that has to be started again from the beginning.

**The other two shared documents already solve this, differently each.**

| document | seam | conflicts today |
|---|---|---|
| `design-decisions.md` | per-lane numbering bands; 426 numbered sections, each lane inserting in its own range | 0 |
| `open-questions.md` | per-lane question ids (`A-Q`/`B-Q`/`C-Q`); 31 open entries | 0 |
| `known-issues.md` | none -- everyone appends at EOF | **11** |

`roadmap.md` rule 3 says the per-lane conventions exist to make the merge
clean, and for the two documents that have one it works: `design-decisions.md`
auto-merged across a 72-commit divergence with zero conflicts. This file
never got the same treatment.

**Why it costs more than the fix suggests.** The resolution is mechanical --
both sides are additive -- but it is not *always* mechanical, and that is the
part worth knowing before choosing. Three times today the winning order
mattered, because one side was an amendment (`### MOSTLY FIXED ... and the
mechanism above was wrong`, `### TRIAGE ...`, my own `### Correction ...`)
whose meaning depends on sitting directly under the entry it amends. A
marker-deletion resolution silently re-parents such an amendment onto
whatever the other lane appended. Twice it was lane C's amendment at risk and
once it was mine.

| option | *What changes:* |
|---|---|
| **(a) Per-lane append sections** (recommended) | each lane appends inside its own `## Lane A / B / C` section, so two lanes writing between merges no longer touch the same lines. New entries land in a different place than today. |
| (b) Per-lane files, indexed | `known-issues-a.md` etc. with the existing index across them. No shared seam at all, but a reader needs three files, and cross-lane entries (of which there are many) need a home. |
| (c) Leave it | nothing changes; the collisions stay mechanical and frequent, and the amendment-ordering hazard stays live. |

**My recommendation is (a)**, because it matches what already works twice in
this tree and needs no new tooling -- `check-known-issues-index` already
walks the headings and would keep working. (b) is cleaner in principle and
worse in practice: today's most useful entries are the cross-lane ones, where
lane C's finding and mine turned out to be the same shape, and splitting the
file makes that harder to notice.

**Why it is yours and not mine.** It changes the layout of a document all
three lanes write, so one lane reorganising it unilaterally is exactly the
shared-word redefinition design-decisions 951 is about. It also wants a halt
to do safely -- moving existing entries into sections while two other agents
are appending would conflict with everything at once.

**If never answered:** nothing breaks. The cost is steady rather than
growing: roughly one chain restart per collision, plus the standing risk that
an amendment gets re-parented by a resolution that looks correct because no
conflict markers remain. I now assert adjacency (`parent < amendment <
other`) rather than marker-absence, which covers my own resolutions and not
anyone else's.

## E-Q1 — [E] Understanding speech needs a data file bigger than the space left on the system disk. Which engine, and does its data ship with the system or install later? — Status: OPEN (raised 2026-09-24)

**In short:** SlateOS is meant to understand speech — dictation and voice
commands — as well as to speak. Every speech-recognition engine accurate enough
to be worth having needs a large data file (a "model": what speech sounds like,
learned from recordings), 40 to 150 MB for English. The disk image SlateOS
installs from has about 40 MB free today. So two answers are needed: which
engine to adopt, and whether its model is part of every installed system (the
image grows) or something a user installs when they first turn dictation on.

Speaking, the other half, is not affected: its engine, eSpeak NG, needs under
1 MB for English and is being brought up now (`roadmap.md` §5.6).

### Which engine

All three run entirely on the machine; none sends audio anywhere.

| | engine | English model | how well it hears | how it feels to use |
|---|---|---|---|---|
| **A** | whisper.cpp (MIT licence) | 75 MB ("tiny") or 142 MB ("base"); larger ones exist, to 1.5 GB | best by a wide margin, and it adds punctuation and capitals itself | text appears a second or two after you pause, a sentence at a time |
| **B** | Vosk (Apache licence) | 40–50 MB | good, noticeably behind A | words appear while you are still speaking |
| **C** | PocketSphinx (BSD licence) | about 30 MB | poor at free dictation; fine for a short list of fixed commands | words appear while you are still speaking |

- **A** — *What changes:* dictating a paragraph gives a punctuated paragraph at each pause, with few mistakes.
- **B** — *What changes:* words stream in live, with more mistakes and no punctuation.
- **C** — *What changes:* "open mail"-style commands work; dictating prose is frustrating.

### Where its model lives

| | | *What changes* |
|---|---|---|
| **1** | in the system image, which grows from 384 MB to 512 MB | dictation works the first time it is switched on, on every install, with no network |
| **2** | an optional package, offered when dictation is first switched on | the base system stays small; first use needs the package, from the network or the install media |

### Recommendation

**A with 2:** whisper.cpp, with its "base" English model (142 MB) as an
optional package offered the first time dictation is turned on, and the
other-language models the same way. Accuracy is what makes dictation usable at
all, and most people never turn it on, so they should not carry 142 MB for it.
If you would rather it were always there, **A with 1** and the 75 MB "tiny"
model is the compact version of the same choice.

**Feasibility, measured 2026-09-25:** whisper.cpp 1.9.4 links against our C
library and the C++ runtime we use with nothing missing, on the first attempt
(a quick link test, not yet a committed recipe). So A is possible today; this
question is only about which engine is right, and where its model lives. It
does not yet say how fast it runs: the virtual machine the tests boot in has
none of the wide arithmetic instructions whisper.cpp uses on real hardware, so
it will be much slower there than on a real computer.

### If never answered

Nothing breaks. Speech output goes ahead without it; speech input stays
unbuilt. Nothing gets worse with time.

**Where it bites:** `roadmap.md` §5.6 `[E] Speech input / speech output`. Option
1 is lane D's image recipe (`scripts/create-ext4-rootfs.sh`); option 2 is lane
B's package manager (`userspace/pkg`).

## E-Q2 — [E] The weather app can fetch forecasts now. From whom, given that whoever supplies them learns where the user is? — Status: OPEN (raised 2026-09-26)

**In short:** The weather app shows no weather: it had no way to reach the
internet, and says so rather than inventing a forecast. Applications can now
open internet connections (the network scanner and the dictionary do), so it
can be made to work — but a forecast is always *for somewhere*, so whichever
company supplies it is told where the user is, every time the forecast is
refreshed. Which supplier, if any, is your call.

*Promoted from `deferred-questions.md`, where lane C parked it on 2026-09-18
until a program could make a network request at all. That trigger has fired.*

| | Option | *What changes* |
|---|---|---|
| **A** | Open-Meteo (a free forecast service that needs no account or key) | *Forecasts work as soon as the user names a place; Open-Meteo sees the place's coordinates at each refresh.* |
| **B** | A commercial service that needs a key | *The same, plus a key this project must obtain, ship and keep secret.* |
| **C** | Nothing by default; the user types in a service's address | *The app stays empty until someone configures it; no company is contacted unless the user chose it.* |
| **D** | Never fetch; remove the app | *One fewer app; nothing is ever sent.* |

**One more thing you should know, whichever you pick:** this system has no
way yet to check a secure (https) site's identity, so a request would go in
plain text — anyone on the same network could see which place was asked
about. Open-Meteo answers plain requests (checked 2026-09-26). Waiting for
secure connections is possible, but nothing on the roadmap delivers them
soon; the kernel has a TLS implementation that does not check certificates
(`kernel/src/net/tls.rs`), which is not the same thing.

**Recommendation: A**, with the app asking nothing until the user adds a
place, saying in the window which service it asks, and letting the user turn
it off. It is the option under which the app is useful; the user's own
action (adding a place) is what starts any sending.

**Related decisions already made, which your answer may overrule:** the
dictionary now looks up words its built-in list lacks at dict.org, when the
reader asks (design-decisions §1214), and the speed test measures against
public test servers when Start is pressed (§1215) -- both decided by Claude,
both contacting a third party only on the user's action. A looked-up word
reveals less than a location, and a speed test nothing of the user's, but it
is the same kind of choice. If you would rather no program contacted a third
party by default, say so here and all three change.

### If never answered

Nothing breaks: the weather app stays empty and says why, as today. Nothing
gets worse with time.

**Where it bites:** `apps/weather/src/main.rs` (`render_cannot_fetch`); the
fetch itself would sit on `net/httpclient`'s request/response parsing and a
plain `TcpStream`, as `userspace/pkg` does.


## E-Q3 — [E] System Restore now keeps the programs' settings and data. Should it also cover the system's own files, and how? — Status: OPEN (raised 2026-09-27)

**In short:** System Restore can now take a restore point of every program's
settings and data and put them back. It cannot do the same for the system
itself -- its programs, how it starts, its services, its installed packages --
and shows those as "needs the system's permission". Undoing a bad system
update is the other half of what people expect from System Restore. The
question is which way to build that half, because the three ways differ in
what they need from the rest of the system and in how much can go wrong.

| Option | What changes for the user | What it needs | Risk |
|---|---|---|---|
| **A. A privileged restore service** that copies system folders (`/etc`, `/boot`, `/usr`...) the way the settings folder is copied | "System files" becomes a component that can be ticked | a service running as root that System Restore asks, and a way to replace files the running system is using (a restart into a restore mode) | high: replacing a live system's files can leave it unbootable if interrupted |
| **B. Filesystem snapshots** (a copy-on-write filesystem, as `design.txt` prefers) | whole-disk restore points, taken instantly | a copy-on-write filesystem -- SlateOS is ext4 today (`design.txt`: "ext4 first") | low once the filesystem exists; blocked until then |
| **C. Package generations** (Nix-style, as `design.txt` also suggests: "roll back filesystem, reinstall package generation") | "Undo the last update" in the package manager rather than here | the package manager to keep each install as a generation | low; covers updates, not hand-edited system files |

*What changes:* A -- a restore point can include the system. B -- the same,
without copying. C -- a bad update is undone from the package manager, and
System Restore stays about the user's own files.

**Recommendation:** C for updates when the package manager can keep
generations, and B for whole-system snapshots if a copy-on-write filesystem is
added; not A. A copy-based restore of a running system's files is the one
design here that can leave a machine that does not start.

**If never answered:** nothing gets worse. System Restore keeps the user's
settings and data and says plainly that it does not cover the system.

**Where:** `apps/systemrestore/src/points.rs` (`SnapshotComponent::source`,
`NEEDS_THE_SYSTEM`); `design-decisions.md` §1217.

## E-Q4 — [E] There are two recycle bins, and neither can see what the other holds. Which one is SlateOS's? — Status: OPEN (raised 2026-09-27)

**In short:** deleting a file in the file manager or the image viewer moves it
to a recycle bin in your home folder, and since today the file manager can show
that bin and put things back from it. The system has a second recycle bin of its
own, built into the kernel (the core of the OS that every program runs on),
which programs could use through a system call (the way a program asks the
kernel to do something) -- but no program does. A file in one is invisible to
the other. And the design asks for more than either does: *every* delete,
including one typed at the command line, should go to the bin; each drive
should keep its own bin; and old items should go only when space runs short.
The question is which bin to build that on, because the three answers put the
work -- and the rules about whose files are whose -- in different places.

**The two, side by side.**

| | The home bin (`~/.recycle`) | The kernel's bin (`/_TRASH`) |
|---|---|---|
| Used by | the file manager, the image viewer, and now Disk Cleanup | nothing but the kernel's own debug shell |
| Whose | one per user | one for the whole machine, shared by every user |
| File names | kept exactly, whatever bytes they hold | text only, and at most 255 bytes of path |
| Per drive | no: deleting from a USB stick copies the file onto the system disk | no: "one per filesystem" is planned, not built |
| Command-line deletes | cannot reach it | could, through the system call, once the shell's `rm` used it |

| Option | *What changes* for the user | What it needs | Cost / risk |
|---|---|---|---|
| **A. The kernel's bin, extended** | a file deleted at the command line appears in the file manager's bin | lane A: one bin per user and per drive, names kept exactly, no length limit; then lane E points the desktop at it | the kernel decides where each user's deleted files live -- policy in the core, where the design keeps as little as possible |
| **B. The home bin only**; the kernel's is retired | nothing, for the desktop; command-line deletes stay permanent unless `rm` is taught to recycle | lane A removes `/_TRASH`; lane E adds one bin per drive to `apps/recyclebin` | cheapest; leaves the design's "every delete goes to the bin" to each program choosing to |
| **C. A recycle-bin service** (a background program, like the backup service) that owns every user's bins on every drive; the kernel's delete call hands the file to it | as A: every delete reaches the one bin the file manager shows | a new service (lane D), the call redirected to it (lane A), the desktop asking it (lane E) | most work; keeps the policy out of the core, which is the design's microkernel rule |

**Recommendation:** C as the destination, reached through B: keep the home
bin as the one the desktop uses (it already is, and its format loses nothing),
add one bin per drive to it, and build no more on `/_TRASH`. When the service
exists it takes the home bin's format with it, so nothing a user has deleted is
stranded by the move.

**If never answered:** nothing breaks or gets worse. The desktop's deletes are
recoverable from the desktop's bin; command-line deletes are permanent, as on
most systems; and `/_TRASH` holds only what someone put there from the kernel
shell by hand.

**Where:** `apps/recyclebin/src/lib.rs` (the home bin); `kernel/src/fs/trash.rs`
and `SYS_FS_TRASH` .. `SYS_FS_TRASH_EMPTY` (618-621) in
`kernel/src/syscall/number.rs` (the kernel's); `design.txt`, "recycle bin".

## A-Q21 — [A] Seven security modules are built but nothing uses them. Staged for later, or believed to be working? — Status: OPEN

**In short:** the kernel has seven pieces of code whose job is to say
"no" — checking passwords, unlocking encrypted disks, deciding who may
open a file. All seven are written and tested. None of them is called by
anything except a command typed by hand into the kernel's own shell. So
nothing in the running system currently asks permission from any of them.
I cannot tell from the code whether that is the plan or an oversight, and
the answer changes what should happen next.

**The seven, and what reaches each.** "Reached from kshell only" means the
single caller outside the module is the kernel's interactive shell — a
person typing, not the system running.

| module | its job | reached from |
|---|---|---|
| `authbroker` | authenticate a principal | kshell only |
| `diskencrypt` | unlock an encrypted volume | kshell only |
| `capsettings` | may this user reach this path | kshell only |
| `secpolicy` | allow/deny by policy | kshell only |
| `sealing` | refuse writes to a sealed file | kshell only |
| `reclock` | byte-range file locks | **no longer latent -- wired to `fcntl(F_SETLK/F_GETLK/F_UNLCK)` on 2026-09-21** |
| `vfs::flock` | whole-file advisory locks | **this row was wrong -- see the correction below** |
| `secureboot` | enrol keys, verify a boot image | `kshell` and `/proc` only — **no syscall at all** |

**Two rows corrected on 2026-09-21, because a decision queue with stale rows
is not decidable.**

- **`reclock` is now reachable.** It was accurate when filed -- the table had
  zero callers outside its own module. It is wired to `fcntl` as of today, and
  the kernel's reason for granting every lock unconditionally turned out to be
  a comment asserting an invariant nothing enforced. So this one is answered by
  events rather than by the operator.

- **`vfs::flock` was never latent, and I should not have written that it was.**
  `nr::FLOCK => sys_flock(args)` is in the Linux dispatch table at
  `syscall/linux.rs:3505`, `sys_flock` calls `Vfs::flock_resolved`, and
  `posix/src/sys_file.rs` re-exports `flock()` for programs to call. Any
  Linux-ABI process can take a whole-file advisory lock and always could. The
  claim "nothing takes one" described the kernel's *internal* callers and was
  then written into a column headed "reached from", which is a different
  question -- the one that matters here is whether a *program* can reach it,
  and it can.

**What this does and does not change about the question.** It does not dissolve
it: five modules (`authbroker`, `diskencrypt`, `capsettings`, `secpolicy`,
`sealing`) plus `secureboot` are still reachable only by a human typing into
kshell, and that is still the thing worth deciding. It does narrow it from
seven to six, and it removes the two entries where the answer was "wire it"
rather than "decide the policy".

**More of the same shape, measured today.** The pattern is wider than security
modules. Of the eight tables holding per-file metadata (see `known-issues.md`
2026-09-21 and `design-decisions.md` §957), the syscall layer reaches almost
none of them:

| module | syscall-layer callers | kshell callers |
|---|---|---|
| `acl` | 0 | all of them |
| `fcomment` | 0 | 10 of 13 |
| `queryable` | 0 | 45 of 49 |
| `tags` | 0 | 18 of 28 |

So a user-visible feature set -- file comments, tags, indexed attributes,
POSIX ACLs -- exists, is tested, and cannot be used by any program. That is the
same question as the one above with a different blast radius: for `secpolicy`
the consequence is that nothing is enforced, and for `tags` it is that a
feature the design promises is unreachable. Worth answering together.
**The eighth one is worse than latent, and it arrived after this was
filed.** `userspace/sbctl` reports creating secure-boot keys and signing
kernel images and does neither — `fs::write` appears nowhere in the crate.
It prints *"Keys created successfully."* The other seven are silent about
being unwired; this one tells the operator their kernel image is signed.
If the answer below is "staged", that is defensible for the seven and not
for this one, which should stop claiming success whatever is decided.

**One detail that decides how it reads.** `diskencrypt`'s unlock is
`unlock_volume(id, _passphrase)` — the underscore means the passphrase is
not used at all, and its comment says so plainly: *"Simulated passphrase
check (in real implementation, derive key and verify)"*. Candid in the
file; invisible to anyone reading the function's name.

**And the same in `secureboot`, found 2026-09-25.** Its check of a boot
image, `verify_image(image_name, hash)`, never looks at `hash` — the
fingerprint of the file being checked. It passes every image unless secure
boot is switched on, and when it is on, it passes every image as long as one
trusted key is on file, which the default table always has. So it is not only
unused; its answer is fixed in advance. That changes option **B** for this
row: it cannot simply be connected, because the first user of it (`sbctl
verify`, lane B's tool) would then report *verified* for any file at all. It
would need a real check first — for instance an allow-list and a deny-list of
image fingerprints (`db` and `dbx`, the two lists the PC firmware standard
already defines), which needs no certificate code in the kernel. Lane B's
`requests/b-a-sbctl-needs-a-userspace-door-to-fs-secureboot.md` is parked on
this question.

| option | *What changes:* | cost |
|---|---|---|
| **A. It is staged — write that down** | nothing runs differently; each module gains a header saying it is not yet enforced, and one list tracks them | an hour. Stops the next person (me, twice already) re-deriving "nothing calls this" while judging how serious a bug is |
| **B. It should be live — wire it up** | a wrong passphrase stops unlocking a volume; a sealed file stops accepting writes; `/proc` denial counts start moving | real work, module by module, and **each one activates its own latent defects on the day it is connected** — several key their tables by pathname, so two names for one file get two answers |
| **C. Leave as is** | nothing changes | free, and the modules keep reading as finished when looked at individually |

**Recommendation: A now, B per-module later.** A is cheap and removes the
specific trap: these all currently look complete in isolation. B is the
right destination but is not one decision — it is seven, each wanting its
own fix-first-then-connect, because connecting one before fixing its keying
turns a dormant bug into a live one.

**If this is never answered:** nothing breaks today, and that is exactly the
risk. The modules look finished, their `/proc` counters read zero, and a
zero reads as *nothing was denied* rather than *nothing asked*. The cost
arrives the first time someone wires one up believing it already worked.

**Where it bites:** `kernel/src/fs/{authbroker,diskencrypt,capsettings,
secpolicy,sealing,reclock}.rs` and `vfs.rs`'s `flock_resolved`. Full
measurement, one grep per row, in `known-issues.md` 2026-09-21.

## A-Q22 — [A] If you rename a file, should it keep its version history? — Status: OPEN

**In short:** the system keeps old versions of files, so you can go back to
yesterday's copy. Every saved version has to be filed under something, and
there are two choices: the file's *name*, or the file itself. They differ the
moment you rename something. Under one choice, renaming `budget.txt` to
`budget-2026.txt` carries all its old versions along. Under the other, the
renamed file starts with a blank history and the old versions stay filed under
the name nobody uses any more. Both are defensible and real products ship each
one, so I would rather you picked than have me pick silently.

**One term, glossed:** an *inode* is the filesystem's internal identity for a
file — a number that stays the same when you rename it, and is different for
any other file, even one later given the same name.

| option | *What changes:* | argues for it |
|---|---|---|
| **A. File it under the inode** (history follows the file) | you rename a document and its history comes with it. If you delete a file and later create a new one with the same name, the new one starts empty rather than inheriting a stranger's history | this is what Dropbox, Google Drive and macOS Versions do. It matches how people think about "this document" |
| **B. File it under the path** (history belongs to the location) | you rename a document and its history stays behind under the old name. A config file at `/etc/app.conf` keeps one continuous history even if the file there is replaced wholesale | this is what someone watching *one important file* usually wants — the history of that slot, including "it was swapped out" |

**Why it is not obvious.** The two options are each correct for a different
job. B is right for an audit trail — you want to know the thing at this
location changed, and a replacement is the most interesting change there is.
That is exactly why integrity monitoring here is filed under the path, and why
changing *it* to the inode would have broken it. A is right for a document you
are working on, where a rename is not an event at all.

**My recommendation: A**, weakly. The subsystem is called "file version
history" and sits next to comments and tags, which are all about a document
rather than a slot; and B's audit-trail job is already served by
`fs::integrity`, which does exactly that and is staying path-filed. But I hold
this loosely — if you picture the feature as "show me what happened to this
config file", B is the better answer and I would change it.

**If this is never answered:** nothing breaks and nothing degrades. Today the
history is filed under the path (option B) by default, simply because that is
how it was first written, not because it was chosen. The cost of leaving it is
that five sibling tables — permissions, locks, seals, immutable flags, and as
of today ACLs — have just been moved to the inode, so `history` is now the odd
one out. Someone tidying later may "fix" it to match its neighbours without
realising that is a user-visible behaviour change rather than a consistency
cleanup. Answering it turns a silent default into a decision either way.

**The same question arrives for tags, with one extra wrinkle -- added
2026-09-21.** File *tags* (`kernel/src/fs/tags.rs`) face the identical choice:
does a tag belong to the file or to the name? Answer A (the file) and renaming
a tagged photo keeps its tags; answer B (the name) and it loses them. Whatever
you decide for history, the same answer almost certainly wants to apply here,
so this is one decision rather than two.

The wrinkle is that tags are stored **twice**, once each way round:

| index | maps | answers |
|---|---|---|
| `by_path` | name -> its tags | "what is this file tagged?" |
| `by_tag` | tag -> the names carrying it | "what is tagged *holiday*?" |

Change only one of them and the two stop being mirrors: a file with two names
would be one entry in the first and two in the second, so the answers to those
two questions would disagree about the same file. *What changes:* searching for
a tag would list one file twice, under both its names.

That is a consequence of the choice, not a separate decision, and it is mine to
implement once you pick -- I mention it only so the cost is visible: option A
here means re-keying both indices, not one, and resolving identities back to
names when displaying a search result.

**Where it bites:** `kernel/src/fs/history.rs`. The conversion itself is small
and mechanical (the pattern is in `kernel/src/fs/immutable.rs`); it is the
*behaviour* that needs your call, not the work. Background in
`design-decisions.md` §957.

## B-Q22 — [B] `kill PID` should ask a program to stop. One of our two `kill`s ends it on the spot instead. Which design do we keep? — Status: OPEN (raised 2026-09-25)

**In short:** two different programs are both called `kill`, and which one the
system ends up with depends on which one the build happened to link last. One
sends Unix-style signals, which works today. The other was written to send a
"please shut down" message to a system service -- which is what the design
notes prefer -- but that service was never built, so the message always fails
and the program then ends the target immediately, giving it no chance to save
anything. So with that one, the ordinary `kill 1234` behaves like the
last-resort `kill -9 1234`. Which approach should the single `kill` we keep
use?

**The two programs.**

| | `userspace/kill` (the "native" one) | `coreutils`' `kill` |
|---|---|---|
| How it stops a program | a message to `org.slateos.ProcessManager`, then a forced kill when that fails | a signal (Unix's numbered "please stop" notification) through the kernel's `SYS_SIGNAL_SEND`, the same path every program's own `kill()` call uses |
| Does its delivery work today? | **No**: nothing anywhere provides that service, so every plain `kill PID` falls through to the forced kill | Yes; a program that asked to be told can clean up first |
| The command line scripts use (`kill -s TERM PID`, `kill -l`) | not understood: `-s` is read as a signal called `s` | yes, measured against procps (the Linux `kill`) |
| Extras | `killall NAME`, `-w` (wait for it to exit), `--timeout` | none |

The system image installs `kill`, and `killall` as another name for it -- which
works only if the build picked `userspace/kill`.

**The design note.** `design.txt`: *"should signals just be done through
[IPC]... ai agrees that shutdown should be done through ipc rather than linux
signals"*. So the native program follows the design's preference; it just has
nothing on the other end.

| Option | *What changes:* |
|---|---|
| **A.** One `kill`, signals through the kernel | `kill PID` asks politely and `kill -9` forces, as on Linux; scripts' `kill -s TERM` and `kill -l` work; `killall` becomes its own small program. |
| **B.** One `kill`, the shutdown-message design, built for real | Same command line as A, but a program is asked to stop by a message it must know how to receive; this needs the service written and a message every program understands, and programs that ignore it are forced after a timeout. |
| **C.** Leave both | `kill PID` means either "ask" or "force", depending on the build order. |

- **A** is small (the signal half exists and passes its tests; the work is
  merging the extras across and deleting the other crate) and matches what
  every program ported from elsewhere already expects: bash, Python and the
  POSIX layer all stop programs through that same kernel call.
- **B** is the design note's direction, but it is a new system protocol, not a
  `kill` change: a service to write, a message format, and every program taught
  to answer it. Until then a message-based `kill` has no one to deliver to.
  Nothing about A prevents B later -- the kernel's signal delivery is itself a
  message the kernel carries, and a future shutdown protocol could sit behind
  the same command line.

**If never answered:** not safe, quietly. Whenever the build links
`userspace/kill` last, every `kill PID` on the system ends programs without
letting them clean up; whenever it links the other, `killall` stops working.
Nothing reports either.

**Claude's recommendation:** **A** now, with **B** recorded as its own future
project if you want shutdown to become a message protocol. It fixes the
forced-kill behaviour and the build-order lottery immediately and forecloses
nothing.

**Where it bites:** `userspace/kill/src/main.rs` (`ipc_graceful_terminate`,
and its fallback to `SYS_PROCESS_KILL`); `userspace/coreutils/src/bin/kill.rs`;
`scripts/rootfs-bin-manifest.txt` (`kill`, `killall = kill`);
`scripts/check-bin-collisions.py`'s one remaining baseline entry;
`known-issues.md` → `TD-B-TWO-PACKAGES-BUILD-A-BINARY-CALLED-KILL`.

## B-Q23 — [B] What is SlateOS for? The answer decides which of its 469 programs ship. — Status: OPEN (raised 2026-10-01, as the operator asked in B-Q21)

**In short:** we have written 469 programs. You asked (answering B-Q21) for a
list of all of them with a line each -- that is now `programs.md`, generated
from the code so it cannot go stale -- and for options on what SlateOS is for,
each with what it would drop. For now everything that builds goes on the disk
image (your answer A); this question is about the image a user eventually
installs. Nothing here deletes code: a program left off the image stays in the
repository, and can be installed later by the package manager (`pkg`).

**The 469, grouped** (the full list with descriptions is `programs.md`):

| Group | Count | Examples |
|---|---|---|
| Everyday command-line tools | 139 | `ls`, `cp`, `grep`, `sed`, `awk`, `tar`, `less`, `nano`, the shell |
| Running the system | 99 | services, users and passwords, logs, disks and partitions, boot |
| Hardware and performance | 27 | `lscpu`, `lspci`, `top`, `htop`, `free`, power and thermal tools |
| Networking tools | 29 | `ping`, `ip`, `curl`, `wget`, `ssh`, `rsync`, `dig`, `tcpdump`, firewalls |
| Network servers | 6 | `sshd`, `ftpd`, `telnet`, `inetd`, `ntpd`, `finger` -- programs other machines connect *to* |
| Linux security frameworks | 6 | `apparmor`, `selinux`, `audit`, `firejail`, `polkit`, `sanitize` |
| Developer tools | 17 | `make`, `gdb`, `strace`, `perf`, `objdump`, `readelf`, `yacc` |
| Desktop applications | 94 | editor, email, calendar, file explorer, spreadsheet, music and video players |
| Games | 45 | chess, solitaire, minesweeper, tetris, sudoku |
| The desktop itself | 7 | compositor (draws the screen), desktop shell, notifications |

**One group is odd out whatever you pick.** The six Linux security frameworks
configure mechanisms SlateOS does not have -- its security is capabilities
(tokens a program must hold to touch anything), not Linux's labels and profiles
-- so they are candidates for deletion under your rule that a command which
does not work should not exist (§1006), not merely for leaving off an image.
That is checked one program at a time, separately from this question.

### The options

| Option | *What changes* | Drops from the image |
|---|---|---|
| **A. Everything, for everyone** -- a desktop that is also a developer's machine and a small server | Every program is installed; nothing to choose. The image is the largest. | nothing |
| **B. A desktop for people who also program** | The servers are not running on a desktop by default. | the 6 network servers |
| **C. A desktop for people** | As B, and the programming tools move to an optional "developer" package. | 6 servers + 17 developer tools |
| **D. A lean desktop** | As C, and games become an optional package. | 6 servers + 17 developer tools + 45 games |
| **E. Let the installer's question decide** -- `design.txt` already has the installer ask what the machine is for (it lists desktop, gaming, development, server and others, to tune memory and scheduling) | Each answer installs its own set: *desktop* is B, *development* adds nothing to B (it already has the tools), *gaming* is B, *server* drops the applications and games and keeps the servers. A user who picks wrongly adds a package later. | depends on the answer |

**My recommendation: E, with B as what "desktop" means.** The installer
already asks the question this entry is asking, per machine rather than once
for everybody -- so the image can hold everything (your "A for now") while
each installation takes the part it was chosen for. Within a desktop, `make`
and `gdb` cost little and "install the developer package first" is friction
exactly when someone is trying something; network servers are the opposite --
a server a desktop does not need is attack surface (a door into the machine)
even when idle, so installing one should be a choice.

**If this is never answered:** nothing breaks. Your B-Q21 answer (A, "for now")
stays in force: everything that builds goes on the image. The cost is only that
the image stays at its largest and every program on it is something that must
keep working.

**Where it bites:** `scripts/rootfs-bin-manifest.txt` and
`scripts/create-ext4-rootfs.sh` (lane D's), and the package definitions in
`pkg/`.

# Resolved

**The body above holds OPEN questions only.** When the operator answers one,
write it up in `design-decisions.md` as a `Decided by: Operator` entry,
**delete the entry from the body**, and add one line here. That is the whole
point of the file: it is scanned for what still needs a decision, so an
answered question left in the body is pure cost — and, being older, it sorts
*first*, right where it is most in the way. (Why this is not append-only:
`design-decisions.md` §437.)

## Resolved — lane A

- A-Q20 A lane may only publish work after a green test run, and lane A's has
  been red for days on another lane's faults. What should a blocked lane do?
  — resolved 2026-09-26 (968), the operator leaving it to Claude: **publish,
  under three conditions, and only then** — every red rung is another lane's
  tracked fault, `main` already fails it for the same image, and the lane's
  own failures are zero; each publish that relies on this says so.
- A-Q10 Saving a file costs twice what it needs to: keep the automatic undo
  history? — resolved 2026-09-13 (936): **opt-in per directory AND off the
  save path.** History is off by default and enabled per directory; where it is
  on, the read-back and checksum happen after the write returns. Accepts a
  bounded cost: a crash in that window loses one version of one file, in a
  directory that opted in.
- A-Q12 Old FAT media show filenames as `????????`: which alphabet do we assume?
  — resolved 2026-09-13 (935): **3 and 4 — neither, then optionally both.**
  Undecodable 8.3 names render as visible escapes, which are reversible and so
  cannot collide; a mount may additionally be told its code page for correct
  names when the user knows the disk's origin. The escape is the right answer
  without information; the code page is an optimisation for when it exists.
- A-Q9 Networking exists twice, in the kernel and as a daemon: should the daemon
  become the default? — resolved 2026-09-12 (934): **C, then D.** Fix the
  head-of-line block first (`D-NETSOCK-SYNC`: a listener and all its accepted
  connections share one session behind one lock), then flip `net.userspace` on by
  default, then delete the in-kernel stack. The order is the decision: flipping
  first would ship the rough edge to everyone, and deleting is the only step with
  no way back but a revert.
- A-Q8 Desktop icon layout exists in two places, kernel and shell: which is the
  authority? — resolved 2026-09-12 (933): **C, neither — it leaves the kernel.**
  `fs::deskicons` and `/proc/deskicons` are deleted; `gui/desktop/src/icons.rs`
  becomes the layout authority and persists positions in userspace, which also
  makes the `icon_size` setting live. Lane C wires first, lane A deletes after,
  so no reboot loses icon positions in between.
- Q45 Convert the whole shell to bytes, or only the expanded word? — resolved
  2026-08-21 (§261): **B, the expanded word.** One data path — keystroke to
  syscall — goes byte-clean end to end; the source line stays text, as in bash.
- Q49 Modern AMD graphics: write it blind, buy hardware, or say we don't
  support it? — resolved 2026-08-21 (§262): **A for now**, C someday. The
  operator's "write it blind but label it untested" variant is recorded in the
  entry along with why it was not adopted.
- Q50 The Intel iGPU driver we also cannot run — which way? — resolved
  2026-08-21 (§263): **C.** Switch the iGPU on in firmware, boot SlateOS on
  this PC's bare metal from a USB stick, then write i915 against the real chip.
  Operator does the physical half; lane A readies the bootable-USB path first.
- Q51 Start the Mesa port now, or leave 3D parked? — resolved 2026-08-21
  (§264): **B, do the port** — sequenced after wifi, before Chromium. Chromium
  uses Mesa heavily but bundles SwiftShader, so Mesa is a performance
  prerequisite for it, not a functional one.
- Q52 Should the contamination-canary check keep failing on noise? — resolved
  2026-08-21 (§265): **D then C.** 20+ idle rounds first, then a shifted-band
  rule instead of zero tolerance.
- Q53 71% of benchmarks move >10% from a no-op rebuild — change the rule? —
  resolved 2026-08-21 (§266): **E.** Restate the threshold against each
  benchmark's measured band now; real hardware (unblocked by §263) is the fix
  that makes it mean something again.
- Q54 Switch to the 3.5× faster accelerator, split, or stay? — resolved
  2026-08-21 (§267): **E then C.** Measure whether the fast accelerator removes
  the noise; if so split — benchmarks fast, correctness gate stays on TCG where
  SMEP/SMAP/UMIP are actually exercised.
- Q46 [opt-level=0 benchmarks: release default or bench-only?] — resolved
  2026-09-07 (§922): **C + commit-count gate trigger;** implemented as
  pre-push gate 15.
- Q47 [D: drive full — shared vs separate target directory?] — operator input
  received 2026-09-07: tree now on E: with ~300 GB free; serialisation cost
  may be near zero; needs re-evaluation on E: before deciding.
- Q56 [Linux ABI exempt from native file-permission checks] — resolved
  2026-09-07, recorded 2026-09-09 (§924): **A, enforce parity**, paid for by
  suspend-and-prompt or an ahead-of-time grant (the same facility as §918).
  Operator's two follow-ups answered in §924: no per-account default-grant
  mechanism exists anywhere in the tree (it is a new feature), and yes it
  should cover native programs too — a per-account default grant is not
  ambient authority, because a real, revocable token is still issued.
- Q57 [capability-request prompt for keyboard/mic/camera?] — resolved
  2026-09-07 (§918): **A, yes,** and fix the error message.
- A-Q1 [`find -size` bare number: bytes here, blocks elsewhere] — resolved
  2026-09-07 (§916): **C, match POSIX** — bare number means 512-byte blocks.
- A-Q2 [C-test programs link unknown library; fix in fastpy] — resolved
  2026-09-07 (§915): **A, fix fastpy directly.**
- A-Q3 [kernel self-tests halt machine on production boot] — resolved
  2026-09-07 (§914): **D, halt on integrity failures, log-and-continue for
  the rest.**
- A-Q4 [`oci run` continues when option unapplied] — resolved 2026-09-07
  (§917): **A, refuse to start.**
- A-Q5 [shell `grep`: case-insensitive + line numbers by default] — resolved
  2026-09-07 (§919): **A, match standard defaults;** also integrate
  operator’s custom grep features.
- A-Q6 [deletion commits + fake-name commits in published history] — resolved
  2026-09-07 (§920): **A, leave history as-is.**
- A-Q7 [70 ms/file-open on D: — antivirus or disk?] — resolved 2026-09-07
  (§921), **closed by measurement 2026-09-09 (§923): it was the disk.**
  Cold reads cost 19.3 s on D: vs 0.27 s on E: for the same 807 files (71x);
  warm, both drives are identical. The "warm pass still costs 61.8 s"
  observation that ruled out the disk does not reproduce (0.19 s). No
  antivirus exclusion should be requested. Re-runnable:
  `python bench/file-read-latency.py`.

## Resolved — lane B

- B-Q8 through B-Q21 (not B-Q15) -- answered by the operator on 2026-09-27
  in lane F's session and relayed verbatim to lane B; each is written up in
  `design-decisions.md` with the operator's words:
  - B-Q8 Which width table? -- (§1042) the operator left the table to Claude,
    which took GNU's (gnulib, Unicode 15.1.0); SlateOS's terminal is to answer
    a width query too (requested from lane C), and the operator's worry about
    glyphs that disagree with the table is answered there.
  - B-Q9 Keep our shell or switch to genuine Oils? -- (§1043) **genuine Oils
    becomes the default**; the Rust OSH is kept as a fallback, not deleted.
  - B-Q10 grep's `-P`: manual or program? -- (§1044) **the manual**: a match can
    belong to any number of windows. Fixed the same day in the operator's
    `grep.py` and `grep.cpp` and in ours.
  - B-Q11 169 names inside other programs -- (§1045) Claude's recommendation:
    case by case, and a kept name is installed as the same file.
  - B-Q12 Should `osh` quote names? -- (§1046) no change: genuine Oils is to be
    the default, so a toggle in ours is not worth having.
  - B-Q13 Randomness shapes; the effort rule's home -- (§1047) Claude's
    recommendation: a userspace library, apart from cryptographic randomness;
    the rule stays in the parent `CLAUDE.md` only.
  - B-Q14 Which `logger` survives? -- (§1048) Claude's recommendation; the tree
    had already arrived there -- util-linux's logger, ported, is the survivor.
  - B-Q16 Record §1005 and §1006? -- option 1. Both entries existed (written
    2026-09-07; the question searched for the wrong heading format), so
    nothing was missing to write.
  - B-Q17 Delete sbctl's refusing commands? -- (§1049) **option 2**: the four
    that need RSA/X.509 are deleted; `enroll-keys` and `reset` wait for lane A.
  - B-Q18 Which large port next? -- (§1050) Claude's recommendation, the fastpy
    compiler; the operator's further ports (Mono, Xonsh, Nushell on SlateOS,
    their WinDirStat fork, a debugger, a LithicBackup reimplementation) are now
    on the roadmap.
  - B-Q19 A standing rule for search-and-replace edits? -- (§1051) **yes, both
    habits, in the `CLAUDE.md` of all three accounts** -- to be applied when
    the operator confirms in lane B's own session, since the answer came by
    relay.
  - B-Q20 `shred --random-source` -- (§1052) the premise had gone: `shred` is now
    a port of GNU's with the option working; the one detail the operator
    proposed differently (restart a source file each pass) is kept as GNU's
    and put back to them.
  - B-Q21 Most programs never reach the image -- (§1053) **stage everything that
    builds for now** (lane D's recipe), then a catalogue of every program and
    options for what the OS is for; a program is recorded where every lane
    looks.
- B-Q7 Which copy of the command-line tools is canonical, after the premise
  behind June's §8 turned out to be false? — resolved 2026-09-07 (§1005,
  `Decided by: Operator`): **B, `coreutils` is the one home.** The better half
  of each of the 41 duplicate pairs survives inside it; the duplicate crate is
  deleted; the 45 bundle-only names stay put rather than becoming 45 crates.
  The operator noted that the "dependency shape that exists nowhere in the tree
  yet" bullet reads like an effort argument and would carry no weight if it
  were one — it is an architectural argument (option A cannot be reached
  without a per-tool crate importing the bundle, the shape §8 set out to
  retire), and B wins on the other reasons regardless. §8 superseded, §359
  un-suspended.
- 2,288 of the 2,756 commands in `userspace/` report success for work they
  never did — which ones do we keep? — resolved 2026-09-07 (§1006,
  `Decided by: Operator`): **stricter than my option A — delete every
  fabricating command, not only the ones that can never work.** A name that
  could be ported one day is added back when it is implemented, not before,
  because a command's existence is a claim made to `command -v` probes as well
  as to people, and a refusing stub answers "yes" to the probe and fails later.
  The audit script is pinned as a ratchet once the deletion lands.
- The test machine cannot produce random numbers, on purpose, and eighteen
  tests in the apps depend on that — should it start? — resolved 2026-09-07
  (§1007, `Decided by: Operator`): **A, land it.** Lane C rewrites its eighteen
  `assert_eq!`-on-two-draws tests in its own tree; lane B files the request and
  the list rather than editing inside lane C's globs. `main` may be red in
  between, which was accepted as the lesser cost.
- B-Q5 70 compiled programs are stored in git and go stale without git
  noticing — keep storing them, or rebuild on demand? — resolved 2026-08-21
  (§355, `Decided by: Claude (autonomous)`): **B, build on demand**, against my
  own earlier "A for now" and against lane A's revised case for C. Measuring the
  arrangement rather than arguing about it settled it: the stamp gate covers
  **9 of the 70**, and **60 of the unguarded 61 were stale at that moment** — so
  drift is the steady state, not an occasional accident. C cannot reach those 61
  at all, because their compiler (fastpy) is a *different repository* whose
  revision this tree cannot record. Rebuilding every fixture costs ~65 s, and the
  kernel already `include_bytes!`s an untracked build output, so B demands no
  toolchain the tree did not already demand. B ships with the guard inverted —
  the rootfs build must refuse to stage a short fixture set, because
  `load_test_elf` self-skips and naive B would otherwise turn stale tests into
  *no* tests, silently green.
- B-Q6 Should the console login prompt obey the system-wide failed-guess
  delay? — resolved 2026-08-21 (§354): **A, and `su` joins with it.** Both obey
  the shared tally for every account including root; the delay-your-neighbour
  effect is accepted as bounded. `passwd` contributes but is never delayed,
  because it gates the remedy rather than access.
- B-Q4 Two user databases that drift apart — which one is real? — resolved
  2026-08-21 (§353): **C, one store with two faces.** `/etc/users.yaml` is the
  truth; `/etc/passwd` and `/etc/shadow` are generated from it on every change.
- B-Q3 Password hashes that can no longer be checked: fail closed, or admit
  those users once more? — resolved 2026-08-21 (§352): **A, fail closed.** Root
  runs `passwd <user>`; no authentication code is kept alive to accept a known
  non-hash.
- B-Q2 GNU's curly quotes in diagnostics, or keep straight ones? — resolved
  2026-08-21 (§351): **B, follow GNU.** Curly marks in the `invalid argument`
  family only; file names stay straight, as they are in GNU.
- Q48 Real kernel objects for "set the clock" / "bind port 80" / "raise your
  own rlimit", or leave them denied? — resolved 2026-08-21 (§350): **B, objects
  for all three.** The operator took B for the port too, where the
  recommendation had been to drop the rule; an object can express "everyone may"
  and dropping the check cannot express anything else.
- B-Q1 Which tzdata do we ship, from where, and how is it updated? — resolved
  2026-08-15 (§311): ship **full tzdata**, vendored as prebuilt TZif binaries
  and updated as a `pkg/` package.

## Resolved — lane C
- **Under the optional Filled look, should the grey boxes be paler?** (C-Q15)
  -- answered 2026-09-27: the boxes stay as they are; instead the accent and the
  other interface colours are kept separately for each look, so a choice made
  under one never lands on the other. `design-decisions.md` §1421.

- **Should the games follow the desktop theme?** (C-Q16) -- answered
  2026-09-27: every game's menus and panels do; each board is decided by
  whether its colours mean something to a player (Claude's calls, sent to lane
  E); and every game made as polished as possible. §1422.

- **Five finished features nobody can reach: wire them up or delete them?**
  (C-Q17) -- answered 2026-09-27: wire them up; where a feature also exists in
  reachable form, the one kept first takes everything both could do. §1423.

- **An event coloured like the accent vanishes on today's date: whose colour
  wins?** (C-Q19) -- answered 2026-09-27: neither colour is changed; a warning,
  both ways round, with a way straight to changing the event's colour. §1424.

- **Four lists of the installed programs: which is the real one?** (C-Q20) --
  answered 2026-09-27: one library in userspace; the kernel's list goes; and
  every program, category, file type and default any of the four held is
  carried over before anything is deleted. §1425.

- **What runs a daily backup?** (C-Q21) -- answered 2026-09-27: a background
  service started at boot, whether or not anyone signs in; a backup missed while
  the machine was off runs as soon as it is on again, without asking. §1426.

- **Automatic sign-in: how does someone reach a different account?** (C-Q22)
  -- answered 2026-09-27: no pause at start-up; a key held while starting shows
  the chooser, and the screen says so from the first moment; starting for
  repair skips automatic sign-in. §1427.

- **The feature list is often wrong: re-check it?** (C-Q23) -- answered
  2026-09-27: section by section as work is picked from it, each check dated.
  §1428.

- **Which keyboard shortcuts are on by default?** (C-Q24) — answered
  2026-09-27: Alt+F4, Alt+Tab, Super, Super+R, Print Screen and its variants
  (including two that save to a file), the dedicated volume and brightness
  keys, and inside programs Ctrl+C/X/V/Z, Ctrl+Shift+Z and Ctrl+F4; everything
  else available but off; Super+Tab folded into a setting for Alt+Tab. The
  operator also asked for a redo tree. `design-decisions.md` §1416.

- **How do passwords leave the password manager?** (C-Q25) — answered
  2026-09-27: both a plain-text export, made unmistakably clear it is insecure,
  and an encrypted backup; plus a program may ask for a password only with a
  capability for it and the user's consent in a prompt. §1417.

- **Where do a program's settings live?** (C-Q26) — answered 2026-09-27: one
  YAML file per program under the user's settings folder, and a settings
  service beside it that tells open windows about changes -- not the function
  that saves. §1418.

- **Should something build every crate before a merge?** (C-Q11) — answered
  2026-09-27 by delegation: the operator left it to Claude, asking that the
  check's cost be measured while the machine carries its normal load and set
  against the time it has saved. Measured and decided 2026-09-27
  (`design-decisions.md` §1430): the boot test already builds and lints every
  crate before a merge, at about 2.4% of its time, and has caught real breaks;
  nothing is added. The
  operator's two testing ideas that came with the answer went to lane A:
  `requests/c-a-two-ways-to-test-a-change-without-a-full-boot.md`.

- **The Open and Save windows should be the file explorer: which way?**
  (C-Q30) — answered 2026-09-27: the explorer shows the window for every
  program, and the program is handed only the file chosen (option A, which
  Claude recommended). Written up as `design-decisions.md` §1415, with the
  work by lane.

- **Nothing draws the mouse pointer; what happens over fullscreen?** (C-Q18)
  — answered 2026-09-27: the pointer is always shown, drawn on the
  presenter's copy today and by the display's hardware cursor plane once a
  screen is shown without copying; the light/dark request is met by the
  existing Default and Inverted outlined schemes. Written up by lane F as
  `design-decisions.md` §1334. Before that it was
  deferred 2026-09-25 to `deferred-questions.md` DQ3, at lane F's request
  (`requests/f-c-c-q18s-premise-changed-the-pointer-is-drawn-over-fullscreen-at-no-cost.md`).
  Lane F built the pointer as a layer laid over the picture as it is shown, the
  way a graphics chip's cursor plane is, and every presenter that exists copies
  a fullscreen picture anyway, so the pointer costs fullscreen nothing -- the
  trade the question asked about does not exist yet. The operator's answer
  settled the later case too, so it does not come back.

- **What does "selected" look like, and what happens to a toolbar?** (C-Q13,
  C-Q14) — both answered 2026-09-12. Selection takes the accent everywhere, at
  the *same* one-pixel thickness rather than a thicker line — the code already
  did that and only the explorer mock drew it heavier. Full-width strips keep
  their fill by default, with the hairline-separator treatment offered as a
  setting beside it. Written up as `design-decisions.md` §834 and §835; together
  they unblock the last 369 draw sites of the border conversion.


- **Should cards be shaded at all, and what colour?** (C-Q10) — answered
  2026-09-11. **Borders, with shaded cards kept as an optional theme.** The
  operator also specified the colours: black border and black headings,
  off-white background, and one blue-green doing three jobs — selected border,
  secondary text, and a switch that is on. Written up as `design-decisions.md`
  §829, with the heading/description split as §830.

  Two consequences the answer forced, both recorded in §829 because they change
  the palette beyond what was asked: the blue-green **replaces** the blue accent
  rather than joining it (every blue-green that clears 4.5 lands 1.19–1.74 from
  `#0036A3`, which is not a second colour), and `subtext0` takes the same value
  as `subtext1` (they were 1.10 apart — one colour — but `subtext0` has 1,087
  uses to `subtext1`'s 161, so it is the one to revisit if they should differ).

  **Still open, and deliberately not closed with it:** how to colour the card
  theme so every combination clears 4.5. The operator's own words — "I guess we
  still have to figure out how to color them". Scoped down from "the default
  look" to "an optional theme", which lowers the urgency without removing it.
  Tracked as `TD-C-THIRTEEN-LIGHT-ACCENTS-STILL-FAIL-ON-CARDS` and revisited
  when the border conversion is done.

- C-Q1 Should normalization consult font coverage? — resolved 2026-08-15
  (§428): **no** — normalization stays font-blind, and the font-fitting stage
  decomposes what the face cannot draw. This was the last 339 sweep
  disagreements, all one question.

- C-Q3 Should all three lanes keep publishing finished work through the one
  shared `os` worktree, after two collided in it? — answered 2026-08-21 by the
  operator, **b**; written up 2026-08-24 (§538): no. A lane publishes with
  `git push origin lane-<x>:main`, a fast-forward that needs no working
  directory and is *refused* rather than tangled if another lane got there
  first. `os` becomes a read-only window on the result.

- C-Q5 Should this OS keep writing its own cryptography by hand? — answered
  2026-08-21 by the operator, **c**; written up 2026-08-24 (§539): the
  primitives (hash, cipher, password hash) are ported from vetted
  implementations; the vault format and the service plumbing on top stay ours.
  The line falls where testing stops reaching — a cipher can compute the right
  answer and still leak the secret through its timing, and no test we write
  sees that, whereas a file format that loses a record is an ordinary bug. The
  eleven hand-written SHA-256 copies collapse to one ported one.

- C-Q4 Nothing can print, and two disconnected halves of a printing system
  exist — which should applications talk to? — answered 2026-08-21 by the
  operator, **c**; written up 2026-08-24 (§540): neither. Printing becomes a
  background service applications submit jobs to, so a job outlives the
  application that started it. Lane C had recommended the cheaper shared
  library (b); the operator overruled it as a stop-gap that would only be
  rewritten, since a library and a service differ in *who owns the job*, and
  every caller written against the library is a caller to migrate.

- C-Q2 On a line mixing Hebrew or Arabic with English, should the Right arrow
  key move the caret one character later in the sentence, or one step right on
  the screen? — answered 2026-08-21 by the operator, **b (visual)**; written up
  2026-08-24 (§541): the screen. A key named for a screen direction follows the
  screen; Home/End and word-motion stay logical, because those name positions
  in the sentence. Caveat carried into the implementation: a widget that does
  not also remember which side of a direction boundary the caret is on will
  **skip a whole right-to-left word** in one press — worse than the old
  behaviour, so a half-switched widget is a regression, not a partial win.

- C-Q6 We have written the Settings screens twice — which copy is the real
  one? — answered 2026-09-07 by the operator, **C**; written up §815: split by
  kind. What the desktop *shows* you (volume overlay, login screen) stays in
  the shell and gets wired up; screens you *open* move to the Settings app and
  the shell's copies go. The operator added a styling mandate that was not part
  of the question: both follow `Aero Desktop (offline).html`, themeable parts
  read from current settings, and the demo's look is the default theme —
  recorded in `roadmap-detailed.md` as instructed.

- C-Q7 The high-contrast scheme's highlight is three times dimmer than the
  others — change it? — answered 2026-09-07; written up §816: **white**, and
  the highlight colour becomes user-configurable in every scheme. The
  configurability is the operator's requirement and binding; the white-over-cyan
  default was delegated to lane C. The operator's colour-vision reasoning was
  correct, but the stronger point was their own first sentence — a highlight
  need not carry meaning in hue at all, and luminance contrast is read
  identically by every form of colour vision.

- C-Q8 The world's timezone data cannot be written because the lane map hands
  the job to a directory that does not exist — who does it? — answered
  2026-09-07 by the operator, **B**; written up §817: lane B, which already
  owns the package manager, with the map corrected in the same change. The
  map's error was the cause of the stall, not a missing decision.

- An account with no password: should the lock screen let it through? —
  answered 2026-09-07 by the operator, **C**; written up §818: such an account
  is never locked at all, so nothing appears that pretends to be protecting
  anything. Setting a password is what turns locking on.

- Which cipher, and who owns it? — answered 2026-09-07; written up §819:
  **ChaCha20-Poly1305**. The operator's rule was "fastest with AES-NI unless
  the bottleneck is the disk anyway"; for a kilobyte vault dominated by key
  derivation, neither cipher is measurable, so the exception applies. The one
  condition that would have flipped it — this becoming the full-disk cipher —
  does not hold: disk encryption already exists in `kernel/src/fs/diskencrypt.rs`
  with AES-256-XTS, a mode not interchangeable with an authenticated-message
  cipher. The entry's unglossed jargon, which the operator called out, is
  glossed in §819.

- C-Q27 — `CLAUDE.md`'s lane row gave lane C a `pkg/**` that has never existed
  (the package manager is `userspace/pkg/`, lane B's). **Moot 2026-09-22**: the
  operator had `CLAUDE.md`'s lane section rewritten for six lanes, and the new
  table — generated from `scripts/which-lane.py`'s, which was already right —
  names no `pkg/`. No decision was needed, so there is no design-decisions
  entry beyond the mention in §1100.

## Resolved — lane D

*(None yet. Lane D was created on 2026-09-22; its first question will be
**D-Q3**, because D-Q1 and D-Q2 are the old names of `deferred-questions.md`'s
DQ1 and DQ2 and are never reissued.)*

## Resolved — lane E

*(None yet. Lane E was created on 2026-09-22.)*

## Resolved — lane F

- F-Q2 Remote desktop's video fallback: which video format? — resolved
  2026-09-27 (1332): **VP9**, with hardware encoders and decoders where they
  can be found, and a software fallback threaded across every core.

(F-Q1's AVIF half, answered "yes", is §1333; its HEIC half is still open
above. C-Q18, answered for lane F's code, is §1334; lane C files its index
line when it retires the entry.)

## Resolved — pre-split (unprefixed `Q<n>`, single-agent era)

These numbers are not to be extended; new questions use `A-Q<n>` / `B-Q<n>` /
`C-Q<n>`.

- Q55 [C] The installer read `size = "100 GB"` as 107 GB — should a decimal
  spelling mean a decimal number? — answered 2026-08-21 by the operator, **c**;
  written up 2026-08-24 (§542): neither spelling is guessed at. `GB` is
  **refused**, with an error naming both alternatives; only `GiB` and bare `G`
  are accepted. Lane C had weakly recommended honouring the spelling (b) while
  naming c the honest option. The deciding point: both "pick one" answers leave
  some existing config file meaning something its author did not intend, with
  nothing announcing it — and a partition table is not a place to be helpful
  about a guess.
- Q45 Should `RenderCommand::Text` carry an overflow policy, rather than text
  being cut mid-glyph with no ellipsis? — resolved 2026-08-15 (§427): **yes** —
  the draw command carries the policy and the compositor draws the ellipsis.
  (Note: `Q45` was reused by lane A for an open question while this one still
  sat in the body — an ID collision the old append-only rule made unavoidable
  and this split removes.)
- Q44 Which mapping of our `(ResourceType, Rights)` handles onto Linux `CAP_*`
  bits, given libc reported "all capabilities held" to everything? — resolved
  2026-08-15 (§312): a **conservative projection** of the real handles, not a
  fiction.
- Q42 One-shot repo-wide rustfmt, or keep formatting only touched files? —
  resolved 2026-08-15 (§310): **one-shot repo-wide**, with a
  `.git-blame-ignore-revs` file alongside so the reformat does not poison
  `git blame`.
- Q40 Should osh reproduce bash's *null array element*, which looks like an
  upstream defect? — resolved 2026-08-15 (§309): **no** — byte-fidelity with
  bash has an "unless it is a defect" clause.
- Q41 Should bash be cross-compiled instead of osh reimplemented? — resolved
  2026-08-14 (§305): **both** — osh ships as the shell, cross-compiled bash
  ships beside it, and osh's bash-fidelity scope is frozen.

### Earlier (Q1–Q39)

- Q38 Should osh be locale-aware, or UTF-8-only? — resolved 2026-08-07 (§104):
  **option A — osh is UTF-8-only**, and `scripts/osh-bash-diff.py` moves to a
  UTF-8 locale so the reference bash agrees. The rejected scope (making osh
  locale-aware as bash is) stays written down in `known-issues.md` under
  `TD-OILS-THE-CORPUS-HARNESS-RUNS-THE-REFERENCE-BASH-IN-THE-C-LOCALE`, at the
  operator's request, so a future change of mind starts from a survey.

- Q38 Add antivirus exclusions so the osh corpus sweep is runnable again? —
  resolved 2026-08-07 (§106): **option A**, scoped to *process* exclusions for
  `bash.exe` and `osh.exe` rather than blanket path exclusions. The command
  itself still needs an elevated shell and is written out in §106.
  (Note, as on `Q45` above: `Q38` was issued twice, on the same day, for two
  unrelated questions — the same append-only collision. Both were answered
  before it could matter, and the numbers are left as they were rather than
  edited, because this list records what the operator answered and the number
  is part of what was answered. `scripts/check-open-questions.py` reports the
  pair as a warning for that reason, and fails only on a collision involving a
  question that is still open.)

- Q37 How far osh's bash parity goes when the behaviour is an upstream bash
  *defect* — resolved 2026-08-07 (§105): **option A — waive it.** A divergence
  is waivable only when the bash side has been traced to its source and found
  to be an unchecked error path with nothing suggesting intent; anything short
  of that is designed behaviour and gets matched.

- Q35 Whether promoted fastpy coreutils replace the Rust ones — resolved
  2026-08-07 (§108): **option A for now**, with a stated trajectory toward B
  per command, gated on a parity suite *and* a performance bar, and surfaced as
  a user opt-in rather than a silent swap. fastpy's scope is explicitly not
  coreutils — the operator's intent is OS functions such as a file explorer or
  a settings dialog. The remaining sub-question (which way the shipping default
  points) is carried forward as Q39.

- Q34 Escalate to a full compiler-instrumented KASAN kernel to catch
  B-KNULLJUMP? — resolved 2026-08-07 (§107): **option B.** The lighter shadow +
  quarantine path was built, hardened and run at scale (100/100 clean, which is
  inconclusive at a ~1-in-120 base rate) without localizing the wild store, so
  the escalation lands as a separate instrumented debug build profile.

- Q36 How osh splits `$PATH` on the Windows dev host — resolved 2026-08-04
  (§103): **option B — split at the `$PATH` boundary only, with a drive-letter
  escape.** `:` is the separator everywhere (the whole rule on SlateOS); on
  Windows `;` is honoured too, since the inherited value is written that way;
  and a `:` after a single letter *and followed by `/` or `\`* is a drive
  letter, not a split point. Decided by Claude autonomously rather than by the
  operator — the recommended option proved small, local and easy to reverse,
  and leaving it open was blocking every corpus case needing a `$PATH` list.
  The operator may overrule.

- Q33 Next phase of the fastpy integration (initiative F) — resolved 2026-07-23
  (§87): **option B — reduce the embedded-ELF kernel bloat (TD-KERNEL-EMBED-BLOAT)
  first**, before promoting fastpy coreutils to real `/bin` commands. The ~48
  self-test ELFs are `include_bytes!`'d into `.rodata` (~3.5 MiB each); move them
  (and future fastpy binaries) onto the rootfs disk and load-from-disk. Operator
  said "I lean towards B"; Claude recommended A (promote to `/bin`) but noted B as
  a defensible prerequisite. B is a prerequisite-ish step toward a `/bin` that
  lives on disk anyway.

- Q32 Build KASAN-style heap-corruption detection to root-cause B-KNULLJUMP —
  resolved 2026-07-23 (§86): **option A — build KASAN-style shadow memory now.** A
  1/8-scale shadow region marking every heap byte addressable/poisoned, with
  instrumented alloc/free and checked stores on the suspect paths, debug-gated to
  protect the <200 ns heap target. Catches the whole live-write corruption class
  at the corruptor's write rather than the victim's later read. Operator said
  "A"; Claude recommended A. Targets the symbolized scheduler-`BTreeMap`-node
  corruption (see `known-issues.md`).

- Q31 SlateOS native-ABI main-thread ELF TLS setup (initiative F) — resolved
  2026-07-21 (§82): **option A — the posix crt sets up main-thread TLS in
  userspace** (finds `PT_TLS` via the linker-defined `__ehdr_start`, lays out a
  variant-II TLS block + TCB, sets the thread pointer), **plus a new native
  `SYS_SET_FS_BASE`** syscall calling the kernel's existing
  `set_current_task_fs_base`. Keeps the microkernel loader minimal and matches
  the kernel's "reset fs_base to 0, userspace sets it up" design. Operator said
  "I'll go with A"; Claude recommended A. Unblocks fastpy binaries (whose C
  runtime uses compiler `__thread`) running on-target.

- Q30 C cross-toolchain for fastpy's SlateOS runtime (initiative F) — resolved
  2026-07-21 (§81): **option A (a clang cross-toolchain to musl), realized via
  `zig cc --target=x86_64-linux-musl`** — a self-contained, portable clang +
  bundled musl headers + musl libc, so no heavyweight system-wide LLVM install
  and no separately vendored musl headers were needed (sidesteps both cons of
  A). Operator said "do A"; Claude picked zig as the concrete mechanism. The
  pure-mode runtime now cross-compiles and a real fastpy program links to a
  ~2.9 MB SlateOS ET_EXEC ELF with zero undefined symbols.

- Q29 fastpy → SlateOS target strategy (initiative F) — resolved 2026-07-21
  (§80): **pure-mode native compile first (A); add the CPython bridge later as a
  superset (B)** — "A at first but eventually B." Unblocks *starting* initiative
  F. Sequencing: mature the POSIX layer → add the `x86_64-slateos` fastpy target
  + port the C runtime in pure mode → compile one real OS component. Claude
  recommended A-first-then-B; operator confirmed.

- Q28 `osh` `$EUID`/`$UID` identity — resolved 2026-07-21 (§79): **default root
  (`0`/`0`) [option A], made per-user configurable** via `OSH_UID`/`OSH_EUID`.
  Seeded as real readonly-integer vars (readonly-enforced, bash-faithful
  listings). Claude recommended A; operator accepted and added the
  default-plus-per-user-override framing. Implemented; known-issues
  TD-OILS-IDVARS updated.

- Q27 `osh` advertising as bash (`$BASH_VERSION`/`$BASH_VERSINFO`) — resolved
  2026-07-21 (§78): **option A (advertise), as a per-user toggle
  (`OSH_BASH_COMPAT`) defaulting on** — mirrors upstream Oils' own `bash_compat`
  flag (which defaults on for `osh`, off for `ysh`; upstream sets
  `BASH_VERSION='5.3'`). osh keeps its level at 5.2 (never claims a 5.3-only
  feature). Claude recommended A + proposed the toggle; operator chose A and
  asked for the per-user-default framing.

- Q26 Oils (OSH) port strategy confirmed — resolved 2026-07-21 (§77): **finish
  the Rust reimplementation (A) now; keep A as a permanent user option even if a
  faithful C++ `oils-for-unix` port (B) lands later.** Claude recommended
  finishing A; operator confirmed and added that B is an additive future option,
  not a replacement.

- Q25 next large initiative + fixed ordering — resolved 2026-07-18 (§69):
  **Option A** (the interactive-shell userland) first, with the explicit
  clarification that the shell is **Oils (OSH)** — a bash-*superset* shell —
  **not bash itself** (roadmap-detailed.md §2.7). Fixed initiative order recorded
  durably so it need not be re-asked: **A → F → B → C → D → E** (1. Oils/OSH +
  coreutils, 2. fastpy build-system integration, 3. Mesa/GPU userspace [gated by
  Q18/virgl], 4. Chromium, 5. WINE, 6. additional filesystems). Claude recommended
  A-then-F; operator set the full ordering.

- Q24 raw `spin::Mutex` holder-preemption — reactive vs. proactive audit —
  resolved 2026-07-18 (§70): **Option B** (proactive kernel-wide audit/conversion)
  — "no technical debt, do it the right way." Not a blind sed: the heap and other
  deliberately-raw locks stay raw + manual-preempt; hot leaf locks move to a
  preempt-aware `PreemptSpinMutex`; contended non-leaf locks move to
  `crate::sync::Mutex` (lockdep); conversion is incremental and validated with
  `wedge-soak.sh` green. Claude recommended A (reactive) with C as escalation;
  operator overruled and chose the full proactive sweep.

- Q23 session model for daemon-backed AF_INET **server** sockets — resolved
  2026-07-18 (§71): **Option A** (shared, refcounted session; no daemon-ABI
  change) for the interim, since the whole per-op synchronous socket path is a
  stepping stone to the async socket server that will replace the ring-per-op
  model wholesale. Standing operator guideline recorded: **do not gold-plate
  interim/throwaway netstack infrastructure** — server sockets get A only; the
  concurrency limitation is documented and temporary. Claude recommended A;
  operator confirmed A.

- Q22 netstack Phase 5 cutover — deletion scope + cutover strategy — resolved
  2026-07-14 (§66): **Q22a → Option C** (phased deletion — L2–L4 core first, app
  protocols re-homed to userspace individually) and **Q22b → (ii) staged**
  (persistent daemon + socket-forwarding behind a default-off boot switch; prove
  parity in QEMU, flip the default, then delete). Claude recommended both; operator
  approved both.

- The coreutils "which set is canonical?" question — resolved 2026-06-12;
  standalone per-tool crates are canonical (§8).
- Q1 `set_mempolicy_home_node` / NUMA mempolicy on UMA — resolved 2026-06-13,
  **operator-confirmed 2026-06-14**; keep the UMA no-op returning 0, option A
  (§10).
- Q2 `/proc/sys/vm/overcommit_memory` & memory-commit policy — resolved
  2026-06-13, **operator-confirmed 2026-06-14** (keep the shipped defaults:
  native strict/committed, Linux lazy/overcommit; both configurable); build the
  both-strategies model (Option 5); map the system-wide overcommit knob to a
  fine-grained native cap (`admin.memory_policy`), not `CAP_SYS_ADMIN` (§11).
- Q3 next major initiative — resolved 2026-06-13; terminal/dev before GUI,
  GCC/CMake/Make toolchain first, CPython then fastpy (§9).
- Q4 toolchain on Slate OS: run-prebuilt-Linux vs native-port — resolved
  2026-06-13; **Path Z** (run prebuilt Linux toolchain binaries on the Linux-ABI
  layer now, native-port selectively later), native-first/no-leak kept
  inviolate, clang green-lit for install (§12).
- Q5 file-backed `mmap` — how far to take the fix — resolved 2026-06-14
  (§22), then **REOPENED 2026-06-14** by the operator, then **RE-RESOLVED
  2026-06-14**: adopt **C-lite** (a unified *read-only* page cache for
  shared-library text dedup + de-double-caching), deferred until a concrete
  consumer appears (the dynamic linker is the likely first; stable VFS
  file-identity is the precursor); writable `MAP_SHARED` writeback stays declined
  / `ENOSYS` (§23). Deferral trigger logged in `todo.txt`.
- Q6 cross-process memory introspection — resolved 2026-06-14: keep
  channel/shared-memory IPC for *consensual* sharing; add a
  **debug-capability-gated** cross-address-space `process_vm_readv`/`writev`
  (`Rights::DEBUG` on a `Process` capability; `EPERM` without it). `ptrace`
  remains a deferred follow-up behind the same gate (§24).
- Q8 Path Z libc + rootfs — resolved 2026-06-14, **operator-delegated to
  Claude**: go straight to **glibc** on an **ext4** rootfs, no musl
  stepping-stone (§25). Claude reversed its own earlier musl-first recommendation
  per the operator's stated preference for hard-work-upfront over throwaway
  scaffolding, given the static-load path is already proven end-to-end.
- Q7 kernel-task-stack-vs-IRQ overflow (B-DF1) — resolved 2026-06-15,
  **operator-chosen option A** (Claude recommended A): per-CPU guard-page IRQ
  stack with a manual nesting-aware switch + deferred preemption, plus the
  `cli`/`sti` recursion guard the restructuring exposed (§26). Validated:
  `http_gzip_8KiB` no longer double-faults at the gzip→dashboard transition.
- Q9 bare-ELF ABI auto-classification — resolved 2026-06-24, **operator-chosen
  option D** (Claude recommended D): default unmarked bare ELF → Linux ABI, add
  `NT_GNU_ABI_TAG` note-walk as a positive Linux signal, stamp native binaries
  with an explicit SlateOS marker; `spawn_process_with_abi` override kept (§33).
- Q10 fullscreen-capture video codec — resolved 2026-06-24, **operator deferred
  to Claude's recommendation**: hardware encode via the GPU driver long-term
  (option C), defer the software-codec port near-term (option D), no stub
  encoder meanwhile; if a software path is ever needed first, AV1/`rav1e` over
  H.264 (§34).
- Q11 zero-copy page-flipping for large channel messages — resolved 2026-06-24,
  **operator-chosen option B** (Claude recommended B): explicit opt-in
  `MSG_ZEROCOPY`-style flag + caller-provided page-aligned landing region; copy
  path stays the default. Compiler follow-up: keep it programmer/library-
  controlled (library-level auto-threshold helper), the compiler does not
  auto-insert the flag (§35).
- Q12 next large initiative — resolved 2026-06-24, **operator-chosen option E**:
  build the C-lite read-only page cache now; lifts the §23 "not now" hold (§36).
- Q13 de-double-cache file data — resolved 2026-06-30, **operator-chosen option A**
  (Claude recommended A): page-cache-primary — the page cache is the single cache
  for regular-file data, the buffer cache caches only filesystem metadata (§38).
- Q14 connect the two cgroup subsystems — resolved 2026-06-30, **operator-chosen
  option A** (Claude recommended A): cgroupfs as the frontend,
  `kernel/src/cgroup.rs` as the enforcement engine; fork/clone/spawn inherit
  `cgroup_id` (§39).
- Q15 next focus — resolved 2026-06-30, **operator-chosen option A then C/D**:
  execute Q13 + Q14 first, then a large initiative — C (GPU accel) or D (Docker /
  container-runtime port) in operator-indifferent order; this is the explicit
  go-ahead for the Docker port (§40).
- Q16 `container diff` baseline semantics — resolved 2026-07-01, **Claude
  autonomous (operator-approved Docker-port scope)**: implemented **option A**
  (overlay-only diff). See `design-decisions.md` §41.
- Q17 `container exec` semantics — resolved 2026-07-14, **operator-chosen
  option B** (Claude recommended B): keep the netns-debug `container exec` facade
  AND add real rootfs-binary exec under a distinct verb (`container run-in` /
  `exec --rootfs`); the `docker exec` delegate + `docker build` `RUN`/`HEALTHCHECK`
  route to the real path (§58).
- Q18 GPU acceleration scope — resolved 2026-07-14, **operator-chosen option B**
  (Claude recommended C): build the kernel-side virtio-gpu render-ioctl dispatch
  now with honest "no-3D" reporting (GETPARAM `3D_FEATURES=0`, no capsets, correct
  errno on 3D ioctls); defer the Mesa port until a virgl test environment exists
  (§59).
- Q19 container network model — resolved 2026-07-14, **operator-chosen option B**
  (Claude recommended B): generalise to N-interface multi-network membership
  (Docker parity) as its own dedicated increment (§60).
- Q20 hard-lockup (BSP-dead) detector — resolved 2026-07-14, **operator-chosen
  option A** (Claude recommended A): build the `i6300esb` watchdog + inject-nmi
  detector, opt-in behind the existing `boot-test.sh --hard-lockup-watchdog` flag
  (§61).
- Q21 `nft`/`iptables` compat tooling — resolved 2026-07-14, **operator-chosen
  option C** (Claude recommended C): keep `nft`/`iptables` as an explicit
  parser/pretty-printer only, fix the docs, steer users to `fw`; defer full/minimal
  kernel wiring (§62).
