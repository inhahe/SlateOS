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
| `mbrtoc16`, `mbrtoc32`, `c16rtomb`, `c32rtomb` | ASCII only -- a bug either way, being made UTF-8 like `mbrtowc` now |
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
`mbrtowc`'s), and leaves the ASCII answers alone.

**Where it bites:** `posix/src/langinfo.rs` (`CODESET`), `posix/src/locale.rs`
(`setlocale`), `posix/src/wchar.rs` and `posix/src/uchar.rs` (the
conversions), `posix/src/ctype.rs` (`MB_CUR_MAX`), `posix/src/iconv.rs`
(the empty name), `userspace/locale` (lane B); design-decisions §104 and
§351, which assume no non-UTF-8 locale.

## D-Q6 — [D] Some of the C library is translated from glibc, whose licence binds every program the library is built into. Keep it, or rewrite those parts? — Status: OPEN (raised 2026-09-28)

**In short:** to make the C library behave exactly as Linux's (glibc)
does, several parts of it were written by translating glibc's own source
code into Rust, line by line -- most recently the Tamil character set,
the new C23 maths functions and `clog10`. glibc's licence (the LGPL)
allows that, on a condition: anyone who receives a program containing it
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
from what the translations do today. Until you answer, new lane D work is
written from the standards with glibc as the oracle only.

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

## C-Q32 — [C] Programs cannot show a notification at all, and there were two half-built notification systems. Should the desktop shell be the one? — Status: OPEN (raised 2026-09-29; lane C has gone ahead with the recommendation, see below)

**In short:** a program has no way to pop up "download finished" or "new
mail" today. There were two unfinished pieces that each did half the job.
The desktop shell has the notification pane (the list you open from the
bell), Do Not Disturb and your notification rules. A separate notification
program (`gui/notifications`) drew the pop-ups ("toasts") and kept a list of
its own -- but nothing starts it, no program can reach it, it keeps its own
Do Not Disturb that ignores your settings, and it had quiet hours of 22:00
to 07:00 switched on for everyone. The question is which becomes the one
system. Lane C has built the recommended answer's first half -- the shell
now pops up its own notices -- because it is cheap to undo until programs
can send anything.

**What each has now:**

| | The shell (`gui/desktop`) | The separate program (`gui/notifications`) |
|---|---|---|
| Runs in a booted system | yes | no -- nothing starts it |
| Pop-ups (toasts) | **yes, since 2026-09-29** (§1447) | yes |
| The list of past notifications | yes, from the bell | yes, a second one |
| Your Do Not Disturb and per-program rules (Settings → Notifications) | yes | no -- a copy of its own |
| Programs can send to it | no | no -- the message types exist, the channel does not |

| Option | *What changes* |
|---|---|
| **A. The shell is the notification system** (recommended; its first half built) | pop-ups come from the shell, beside the pane and the bell they belong to; the separate program is retired; programs send to the shell |
| **B. The separate program is the notification system** | it is started at login and shows the pop-ups and the list; the shell's pane becomes a view of that program's list, asked over a channel, and the shell's own pop-ups go |

**For A:** one list, one Do Not Disturb, one set of rules -- the shell
already has them working, and your settings already reach them. It is how
Windows, GNOME and KDE do it: the shell shows notifications. **Against A:**
the shell grows; if it crashes, notifications stop -- but the whole desktop
has stopped then anyway.

**For B:** notifications in a process of their own, isolated from the
shell. **Against B:** the list, Do Not Disturb and the rules live in the
shell today, so either they move or two processes keep one state in step
over a channel -- the source of the "two copies drifted apart" bugs this
tree has fixed many times.

**Either way,** programs need a channel to send on, which is another lane's
to build (the window system's protocol is lane F's, the services lane D's);
lane C files that request once this is decided.

**If it is never answered:** nothing breaks. The shell pops up its own
notices now; what stays missing is programs' notifications, which need the
channel above either way. Under B the pop-ups built for A are the drawing
code the separate program would reuse.

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

## B-Q8 — [B] Two programs we copy disagree about the width of 626 mostly-invisible characters. Which do we copy? — Status: OPEN (re-asked 2026-09-14)

**In short:** Terminal text sits in fixed cells. A Chinese character takes two,
an accent mark that sits on the previous letter takes none, most things take
one. We keep one table of those numbers. The two things we are cloning — the
**bash** shell and the **GNU command-line tools** — ship *different* tables, and
they disagree about 626 characters. We currently match bash. **The question is
only which of the two we copy.**

### Your two questions from 2026-09-14, answered first

**"Why wouldn't the table simply report how wide we actually print each
character?"** — It should, and **as of 2026-09-14 it effectively does, which
removes this from the decision.**

When you asked, the answer was bad enough to be its own defect: our terminal
had no notion of width at all. It advanced one column for every character, so
it drew Chinese, Korean, Japanese and emoji one cell wide when they need two —
**185,074** characters where screen and table disagreed, against the 626 this
question is about. That was filed to lane C, who own the terminal.

They have fixed it, and they fixed it the better way round: rather than
rewriting the table to describe the renderer, **they made the renderer read the
table.** A wide character now takes two cells, a combining mark takes none, and
the two can no longer drift apart because there is only one source. **On `main`
since 2026-09-15**, verified here rather than taken from the report.

**So screen-correctness is no longer part of this choice.** Whichever table you
pick, the terminal will draw what the table says. What is left is the narrow
question below: which upstream do we match on 626 characters.

**"Why wouldn't the GNU and bash programs ask how wide a character is, and
adjust?"** — Two separate reasons, and the first is the one that surprises
people:

1. **There is no way to ask a terminal how wide it will draw something.** No
   such query exists in the protocol. The only way to find out is to print it
   and see where the cursor lands — a round trip per character, and impossible
   when the output is a file or a pipe, which is exactly where `ls` decides its
   columns. So *every* implementation everywhere embeds a static table and
   hopes it matches.
2. **bash and the GNU tools are not programs running on SlateOS that could
   consult our table** — they are the two upstreams we are *reimplementing*.
   Our own programs do all ask one shared table, which is the part that works
   as you would expect. The 626 exists because on Linux bash asks the C library
   and GNU coreutils 9.5 deliberately overrides the C library with its own
   newer Unicode tables. We can match one or the other, not both.

### The options

| Option | *What changes:* |
|---|---|
| **(a) Copy the GNU tools' table** *(recommended)* | Our `ls` and `wc -L` match GNU byte-for-byte on those 626; our shell's menu stops matching bash on them. It is a pinned, re-derivable upstream (Unicode 15.1.0); ours came from whatever Python the build machine had. |
| **(b) Keep bash's table — today's behaviour** | Nothing changes. Those 626 stay permanently marked "differs on purpose" in the `ls` harness, which dulls it. |
| **(c) Describe our own renderer instead** | **Withdrawn — overtaken.** It existed to make screen and table agree; lane C achieved that by pointing the renderer at the table, so there is nothing left for it to fix. |

A fourth option — two tables, one for the shell and one for the utilities — is
what `charwidth` exists to prevent: the symptom is a menu and a listing that do
not line up on the same screen. I do not recommend it.

**Why (a):** six utilities consult a width (`ls`, `wc -L`, and `sort`, `pr`,
`df`, `numfmt` when written) against one shell, and gnulib's table is a named
upstream we can re-dump mechanically. **Why I have not just done it:** it
silently changes on-screen layout in six programs to win a byte-diff in one,
and user-visible behaviour is yours.

**If never answered:** safe, and it does not worsen. Today's behaviour is (b).
The cost is two permanently-deferred cases in the `ls` harness, and the 626 are
mostly invisible marks and unassigned code points — nobody has a filename made
of them by accident.

**Where it bites:** `userspace/charwidth/src/lib.rs`;
`userspace/oils/tests/gen_display_width.py`; `userspace/oils/src/width.rs`;
`userspace/coreutils/src/bin/{ls,wc}.rs`; `userspace/column/src/main.rs`;
`scripts/ls-diff.sh` (fixture `y/`). Full technical detail, including the
185,074 measurement and the gnulib source that causes the split, is in
`known-issues.md` → `TD-B-OUR-WIDTH-TABLE-IS-BASHS-AND-COREUTILS-9.5S-IS-NOT`.

## B-Q9 — [B] We wrote our own copy of a shell because we could not build the original. We can now. Keep the copy, or switch to the original? — Status: OPEN

**In short:** the *shell* is the program that runs the commands you type. SlateOS
has one we wrote ourselves, in Rust — a re-creation of an existing open-source
shell called Oils. We re-created it because Oils is written in C++, and at the
time we had no way to build C++ programs for SlateOS. **That is no longer
true**, as of a measurement made today. So the original is now obtainable, and
it comes with a second, more modern command language that our copy does not
have at all. The question is whether to keep our copy, offer both, or replace
ours with the original.

### What changed

Oils ships two languages: **OSH** (compatible with the shell most people
already know) and **YSH** (its newer one, with real lists, dictionaries and
functions). We have a hand-written Rust version of OSH only. YSH has always
been deferred — not for lack of interest, but because building it meant
building C++ for SlateOS, which nothing could do.

Today's check: the compiler we already use for C (`zig`) turns out to build
C++ for our target as well — it is the same program, and we have had it since
July. A C++ test program compiles under our own build settings, and when
linked against SlateOS's own C library **every unresolved name is a C++
standard-library one and none is ours**. So the missing piece is link-line
wiring, not a missing tool.

**Not yet established:** nobody has built genuine Oils, and no C++ program has
been run on SlateOS. This says the *obstacle* is gone, not that the job is
done. Expect the port to be real work — just ordinary work rather than
blocked work.

**How much work, measured and then done since this was written:** all three
remaining link-line gaps were closed the same day. A C++ program using
`<string>`, `<vector>` and a real `throw`/`catch` now **links** for SlateOS
against our own C library, with nothing missing and nothing colliding.

That sharpens the question rather than answering it. The obstacle this entry
was written around is gone, and what is left is the ordinary work of a port:
Oils' build system generates its C++ from Python, and whether that survives
cross-compilation is still unmeasured. **Nothing C++ has been run on SlateOS
yet** — a linked binary is not a working one, and the CPython and bash ports
each sat at exactly this stage before anyone knew whether they ran.

So option (c), "not yet", is now a weaker position than it was: the thing it
was waiting for has happened.

### The options

| | *What changes:* |
|---|---|
| **(a) Keep ours as the default; ship Oils as an optional install** | Typing `sh` still gets our Rust shell. Someone who wants YSH installs a package and gets it. Two shells exist; each keeps working. |
| **(b) Replace ours with genuine Oils** | Typing `sh` gets upstream Oils. YSH is present for everyone. Our Rust shell is deleted, and roughly a year of accumulated behaviour goes with it. |
| **(c) Neither yet — stay as we are** | Nothing changes. No YSH, and our Rust shell keeps needing hand-maintenance to track upstream. |

These are the two the original decision itself left open (`design-decisions.md`
§73), plus the option of not moving.

### A consideration on each side, briefly

**For (b):** our copy will always chase upstream, and any behaviour we have not
re-created is a difference someone eventually trips over. The original is the
definition of correct by construction.

**For (a):** our Rust shell is small, boots early, and has no C++ runtime under
it — which matters for a shell that has to work when little else does. Deleting
it trades a dependable small thing for a faithful large one.

**Against hurrying either:** the measurement says the *tool* exists. Whether
Oils' own build system, which is unusual (it generates C++ from Python),
survives cross-compilation is unmeasured. It would be reasonable to answer this
only after somebody tries the build.

### If this is never answered

Nothing breaks and nothing degrades: option (c) is the status quo and is safe.
The cost is only that YSH stays absent and our shell keeps needing hand-work.
The one thing worth avoiding is leaving the *reason* stale — the project has
already lost ~1,100 commits once to a decision whose premise had quietly
expired, which is why this was checked at all.

## B-Q12 — [B] Should `osh` quote names in its error messages, when bash does not? — Status: OPEN

**In short:** Our shell prints errors like `osh: unset: myvar: cannot unset`,
copying bash exactly. The name in the middle comes from whatever the user
typed. If a user types a name that contains a newline, the second half of it
lands on its own line and looks like a *separate error message the shell never
printed*. Everywhere else in this tree we prevent that by putting quotes round
the name; bash does not, and the whole point of `osh` is to behave like bash.
So: copy bash, or be safer than bash?

**Glossary.** *Forging a line* — making a program appear to print something it
never printed, by hiding a newline inside a value it echoes back. *osh* — our
bash-compatible shell, `userspace/oils`.

**A worked example.** A script does `unset "$name"` where `$name` happens to
hold `foo` followed by a newline followed by `osh: rm: /etc: removed`. Today
the user sees two lines, the second indistinguishable from a real message.
Nothing downstream — a log reader, a test harness, a person — can tell.

**How this came up.** Gate 22 (`scripts/quote-names.py`) was widened on
2026-09-11 to see `format!`, and it then reached `osh` for the first time,
flagging 16 sites. It had never seen them before because `osh` writes through
its own `perrln` rather than `eprintln!`.

**What is NOT at stake, so it does not confuse the decision:**

* *Byte fidelity.* One might expect the names to be raw bytes that `format!`
  mangles. They are not: `format!` needs `Display`, which byte strings do not
  implement, so every one of these values is already text. `osh`'s real
  byte-string debt is elsewhere and is unaffected either way.
* *One of the 16 is not a name at all* — `hash: {opt}:` interpolates a fixed
  `"-d"` or `"-t"`. That one is simply not a defect.

**Options**

**A. Copy bash. Leave the messages exactly as they are.**
*What changes:* nothing; the messages stay byte-identical to bash's.
*For:* `osh` exists to be bash, and a script that greps stderr for a known bash
message keeps working. `osh` already has a documented convention for this —
`perrln`'s doc says shell data is written through unchanged because
"`ls: cannot access 'aÿb'` names the file you can actually `rm`".
*Against:* we knowingly keep a hole the rest of the tree closed, in the one
program most likely to be handed hostile input.

**B. Quote the name, diverging from bash.**
*What changes:* `osh: unset: myvar: cannot unset` becomes
`osh: unset: 'myvar': cannot unset`.
*For:* closes the hole; matches every other program we ship.
*Against:* a real, visible divergence in a compatibility-critical program, and
scripts that match on the exact text break.

**C. Make it a toggle, defaulting to bash's behaviour** — the shape already
used twice here: `OSH_BASH_COMPAT` (§78) and `OSH_UID`/`OSH_EUID` (§79).
*What changes:* nothing by default; an operator who wants the safer behaviour
sets an environment variable.
*For:* no compatibility regression, and the safety is available. The precedent
is this project's own and the operator set it both times.
*Against:* a third knob, and the safe behaviour is off for everyone who does
not know it exists — which is everyone.

**Recommendation: C**, on the strength of the precedent rather than on my own
judgement of the tradeoff — the operator has twice chosen exactly this shape
for exactly this kind of bash divergence in this exact program.

**If it is never answered:** nothing breaks and nothing gets worse. The 16
sites are exempted in the gate's IGNORE table pointing at this question, so the
ledger is honest rather than silently zero. The risk is real but is bash's risk,
which every shell script in the world already runs.

## B-Q11 — [B] 169 command names exist inside other programs and cannot be run. Give them their own programs, or delete them? — Status: OPEN

**In short:** A program can behave as several different commands depending on
the name it was started under — the same file installed as `useradd` and as
`userdel` does two different jobs. We have 169 such extra names, and **not one
of them is installed anywhere**, so the code behind them is finished, tested,
and unrunnable. `useradd` answers to `userdel`, `usermod`, `groupadd`,
`groupdel` and `groupmod`; `systemctl` to 14 more names; `selinux` to 11. The
question is whether to give those names real programs, install one program
under many names, or delete the code.

**Why it is not just a packaging chore.** SlateOS grants permissions
per-program: the kernel decides what a program may do by looking at *which
binary* it is, not at what name it was started under. So one file installed
under six names holds one set of permissions — the union of all six jobs.
`userdel` would run holding everything `useradd` needs, and vice versa. That is
the reason `design-decisions.md` §8 retired multi-name programs in the first
place. §1005 later overruled §8 for the `coreutils` bundle specifically, and
left everything else unstated, which is why this is a question rather than a
lookup.

### The options

**A. One crate per name — 169 new programs.**
*What changes:* `userdel` exists as its own command and can be granted only the
permission to delete a user. Every name gets its own permission set.
Cost: 169 crates to create and keep building; much of each is a thin wrapper
around shared code that already exists.

**B. Install the one program under every name.**
*What changes:* `userdel` runs, and is the same file as `useradd`, so it holds
`useradd`'s permissions too. Cheapest by far — a packaging list, no new code —
and it is how busybox and toybox ship. It gives up per-command permissions for
these 169.

**C. Delete the extra names.**
*What changes:* `userdel` does not exist; deleting a user is whatever
`useradd` itself offers. Removes several thousand lines of working code, and
scripts written for Linux that call `userdel` stop working.

**D. Case by case.**
*What changes:* nothing uniform. Some names get crates (the ones a script is
likely to call), some are deleted (tools for subsystems SlateOS does not have,
like the 11 SELinux ones), some are left. Best end result, most judgement, and
needs a rule for deciding or it becomes 169 separate arguments.

**My recommendation: D, with a default of B for anything kept.** The
permission argument is real but it is not equally real for every name: the
five `useradd` siblings all edit the same two files and would end up with
near-identical grants anyway, whereas `systemctl`'s 14 are genuinely different
jobs. Starting from B costs nothing and can be narrowed to A later for names
where the permission split turns out to matter; starting from A commits 169
crates up front to buy a separation most of them do not need.

**If this is never answered:** nothing breaks and nothing gets worse. The code
is unreachable, so it cannot misbehave; it is dead weight that can drift from
the reachable copy beside it — which has already happened once, where a bug was
fixed in `coreutils`' `logname` and left in the unreachable copy inside
`nproc`. The ledger (`scripts/multicall-aliases-baseline.txt`) only shrinks, so
the number cannot quietly grow while the question waits.

**Where it bites:** `scripts/multicall-aliases.py` and its baseline;
`known-issues.md` →
`TD-B-ONE-HUNDRED-AND-SEVENTY-TWO-COMMAND-NAMES-NOBODY-CAN-RUN`.

## B-Q10 — [B] Your grep's manual and your grep disagree about one flag. Which one is right? — Status: OPEN

**In short:** we are copying your `grep`'s extra features into SlateOS's. One
of them — the `-P` proximity search — behaves differently from the way your
`README.md` describes it, and we found this by running your own program. Before
copying it, we would like to know which of the two you meant, because we will
faithfully reproduce whichever you say.

### What the manual says

> `-P` with a NUM at least as large as the file is exactly equivalent to the
> default whole-file gate. That equivalence is asserted by the test suite.

### What the program does

A three-line file, searched for two words:

```text
line 01 ALPHA
line 02 BETA
line 03 ALPHA
```

| command | prints |
|---|---|
| `grep.py ALPHA -e BETA` (no `-P`) | lines 1, 2, **3** |
| `grep.py -P 100 ALPHA -e BETA` | lines 1, 2 |

100 is far larger than the file, so by the manual these should match. They do
not: line 3 is missing from the second.

### Why

`-P` clears its record of which words it has seen each time it completes a
group. The `BETA` on line 2 is used up by the group that ends there, so the
`ALPHA` on line 3 has no `BETA` left to pair with and is not part of any group.
The no-`-P` path has no such step — once the file is known to contain every
word, it prints every matching line.

### The options

| | *What changes:* |
|---|---|
| **(a) The program is right; the manual is wrong** | Nothing changes in your tool. SlateOS's grep copies the behaviour above, and the README sentence gets corrected. |
| **(b) The manual is right; the program has a bug** | Your `-P` would print line 3 as well, i.e. a word can belong to more than one group. SlateOS's grep copies *that*, and your tool needs a fix. |
| **(c) Both are intended, and the manual means something narrower** | Say what the equivalence is meant to hold for and we will test that instead. |

### If this is never answered

Nothing breaks. We implement **(a)** — the behaviour your program actually has,
since that is what you are used to seeing — and note the divergence from your
manual. The risk of leaving it is only that if you meant (b), we will have
faithfully copied a bug, and it will be harder to change later once scripts
depend on it.

**Not urgent, and not a criticism of the tool.** We only found it because the
port needed the exact rule, and the manual's own example was not enough to
derive it either.


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

## B-Q13 — [B] Two trailing questions the operator asked in their answers, which nobody had picked up — Status: OPEN

**In short:** the operator's answers arrive in
`open-questions-answers.txt` in the integration tree — untracked, on no branch,
named by no script. Lane A found it by accident in `git status` five days late.
Two of the answers end with a question back to us, and neither had been recorded
anywhere. They are reproduced verbatim below so the operator can see we read
them rather than paraphrased them.

### 1. Randomisation shapes

> *(answering the 2026-09-05 question about the test machine's random numbers)*
> "A. By the way, can and should we provide sophisticated randomization options
> such as bell curve, etc.?"

**Can:** yes, and cheaply. A normal (bell-curve) draw is a short transform of
two uniform draws, and the same is true of the other common shapes —
exponential, Poisson, a weighted pick. None needs kernel support beyond the
uniform source that already exists; they are arithmetic on top of it.

**Should — and this is the part worth the operator's judgement.** Where they go
decides whether they are useful or a liability:

| where | good for | bad for |
|---|---|---|
| a library every program can call | simulations, test data, jitter/backoff | nothing much |
| the kernel's random syscall | — | **security.** A non-uniform source is the wrong thing for keys, nonces or ASLR, and putting it beside the uniform one invites picking the wrong one |

**Recommendation:** a userspace library, deliberately *not* reachable through
the same call as cryptographic randomness. The two have opposite requirements —
one wants a named, reproducible-from-a-seed distribution, the other must never
be reproducible — and a single API offering both is a footgun rather than a
convenience.

*What changes if never answered:* nothing breaks. No program in the tree wants
a bell curve today; this is a capability question, not a defect.

### 2. Is "I want the best thing regardless of effort" in SlateOS's CLAUDE.md?

> *(answering B-Q7)* "...I generally want the best thing regardless of how much
> more work it might take (and by the way, is this in Slate OS's claude.md? If
> not, it should be)"

**Checked, and the answer is "yes, but not in the file you probably mean."**

* `E:\visual studio projects\CLAUDE.md` — **has it, in full**, as *"What I
  Optimize For: The End Result, Not Time or Risk"*.
* `E:\visual studio projects\os\CLAUDE.md` — **does not mention it at all.**

The first file covers SlateOS: its own header says it lives at the drive root
rather than in `os/` so that one copy serves all three lane accounts, because a
user-level rule would otherwise need three copies kept in step by hand. So the
rule **is in force**, and every lane reads it.

**We have deliberately not copied it into `os/CLAUDE.md`**, and want the
operator's ruling rather than guessing. Duplicating it would create exactly the
drift the parent file was written to prevent — two statements of one rule, which
is the shape this tree has spent a lot of effort removing elsewhere. The
alternative, if the operator wants `os/CLAUDE.md` to stand alone, is a one-line
pointer to the parent file rather than a second copy.

*What changes if never answered:* nothing. The rule is already being followed;
this is about where it is written down.


## B-Q14 — [B] `logger` writes to the terminal instead of to the log. Which of the two implementations survives? — Status: OPEN

**In short:** `logger` is the command a shell script uses to record a line in
the system log — `logger "backup finished"`. This tree has two of them and they
do completely different things with that line. One **prints it to the screen**;
the other **sends it to the system log** the way every other Unix does. One of
the two is going to be deleted, and which one decides whether a script that logs
a message ends up spraying text over a user's terminal. I do not think I should
pick this one on my own, because it is a user-visible behaviour change either
way and the argument for the current coreutils behaviour cites an architectural
rule that I think it is misreading.

**The two:**

| | `coreutils`'s `logger` | `userspace/logger` |
|---|---|---|
| Where the message goes | **stdout** — the terminal | `/dev/log` socket, or appends to a log file |
| Options | 2 (`-t`, `-p`) | 23 |
| Upstream fidelity | none claimed | "Compatible with POSIX/BSD logger(1)" |

**Why the stdout version exists, and why I think the reason is a
misreading.** Its module doc says: *"Writes a syslog-style text line to stdout
(our OS uses text-based logs, not binary syslog)."* The rule it is pointing at
is real — `CLAUDE.md` says **"No binary logs. Text-based (JSON-lines)
structured logging."** But that rule is about the **format** a log is written
in, not about **where** a log lives. A text log still has a destination. Writing
to stdout does not make the log textual; it means there is no log, and the
message goes to whatever the caller's stdout happened to be.

The practical difference: a cron job or init script that runs
`logger "started"` expects silence on the terminal and a line in the log. With
the stdout version it gets the opposite — nothing logged, and a line of noise
in whatever captured that script's output.

**Options:**

**(a) Keep `userspace/logger`, delete coreutils'.** *What changes:* `logger
"msg"` prints nothing and the line appears in the system log; 21 more options
start working. Pro: matches every other Unix, so existing scripts behave as
written. Con: it is the larger, less-reviewed implementation, and it needs a
log destination to actually exist on SlateOS — if nothing is listening on
`/dev/log`, messages go to a file append or are lost, which is a quieter
failure than printing them.

**(b) Keep coreutils', delete `userspace/logger`.** *What changes:* nothing
today. Pro: the surviving code is small and reviewed, and while SlateOS has no
log service, printing is at least visible. Con: `logger` does not log, which is
the one thing its name promises, and the option surface stays at 2 of 23.

**(c) Merge: coreutils' implementation, `userspace/logger`'s destination.**
*What changes:* same as (a), but the code that survives is the small one, with
socket/file output ported into it. Pro: keeps the reviewed implementation and
fixes the destination. Con: the most work, and it needs the same decision about
what to do when no log service is listening.

**My recommendation is (c)**, with (a) as the fallback if the port is bigger
than it looks. The thing I am least sure about — and the reason this is a
question rather than a judgment call — is whether the operator intended
`logger` to be a terminal tool on this OS. If that was deliberate, (b) is
right and the module doc should say so in those words instead of citing the
binary-logs rule.

**If this is never answered:** nothing breaks and nothing gets worse on its
own. `logger` stays at 2 options and keeps printing to the terminal, and the
duplicate pair stays in `dup-bins-survey`'s table as undecided. It only bites
when something starts relying on the system log actually receiving what was
sent to it.

## B-Q16 — [B] Two decisions of yours are cited 33 times and were never written down. Record them? — Status: OPEN

**In short:** a *design decision* here is a numbered note in
`design-decisions.md` explaining why the code is the way it is. Two of
yours from 2026-09-07 — numbered §1005 and §1006 — are referred to by
name in 33 places across the project's documents, and by me in several
commit messages today, but neither note itself exists. Anyone following
one of those references finds nothing. Nothing is broken in the running
system; what is missing is the written reason behind a rule everyone is
already following.

**How I know they are missing rather than misplaced.** `design-decisions.md`
contains the string `1005` eight times: two are references saying
"SUPERSEDED by §1005", and the other six are font glyph numbers in an
unrelated entry. No heading numbered §1005 or §1006 — or any four-digit
number — exists in any document in the repository. The numbers are inside
lane B's reserved band (§1000–§1099), so they were allocated deliberately
and then the notes were never appended.

**What the references say the two decisions were.** Reconstructed from the
33 citations, not from memory:

| | What the citations say it ruled |
|---|---|
| **§1005** | `coreutils` is the one home for a coreutils command. It resolved the open question "we have two of several commands — which ones do we keep?", superseded an earlier §8, and un-suspended an entry that §8 had put on hold. |
| **§1006** | Described as *your* ruling: "delete every fabricating command" — a command that does not work is deleted rather than kept as a stub that refuses. |

**Update, 2026-09-12: your own words for §1006 exist, and I found them.**
There is an untracked file `open-questions-answers.txt` in the
integration checkout (`E:\visual studio projects\os`), dated 2026-09-07
— the same date as both missing numbers. It answers the lane-B question
"2,288 of the 2,756 commands in `userspace/` report success for work
they never did. Which ones do we keep?" with:

> Why not delet all of them that don't work, rather than just the ones
> that can never work? You said yourself tat a command's existence
> itself is a claim, and it could be misleading not only to scripts and
> installers, but users who see the command's existence. The ones that
> don't work but could work later can simply be added when we actually
> implement them?

That is §1006, in your words rather than my reconstruction of them, and
it says something the 33 citations had lost: the reason is that **a
command's existence is itself a claim**, and the standard is *does not
work* rather than *can never work*. The citations had compressed this to
"delete every fabricating command", which is the same rule with its
justification and its scope removed.

**This changes my recommendation for §1006 but not for §1005.** For
§1006, option 1 is no longer a reconstruction — it is a quotation, and I
would be transcribing rather than paraphrasing you. For §1005 (`coreutils`
is the one home for a coreutils command) that file contains nothing: it
holds only two lane-B answers, and the other is about the random-number
generator. So §1005 remains a reconstruction from citations alone.

**One thing worth your attention regardless of how you answer.** That
file is untracked, so it is in no branch, no lane can see it, and nothing
backs it up. I first wrote here that losing it would lose the answers,
and then checked instead of leaving it asserted: it would not. Every
2026-09-07 answer I sampled — Q46, Q47, Q56, Q57, A-Q3, C-Q6, C-Q7 — is
already relayed into this file's resolved lists, and B-Q8 records your
reply to it in full.

What would be lost is narrower and, on today's evidence, still worth
something: the **verbatim wording**. §1006 is the demonstration. The
decision survived in thirty-three citations; the *reason* ("a command's
existence itself is a claim") and the *scope* (delete what does not
work, not merely what can never work) did not survive the relay into
those citations, and I have been applying the compressed version all
day. A relayed summary keeps the choice and loses the argument for it,
which is exactly what a decision record is supposed to preserve.

I have applied both repeatedly today — deleting `nohup`, `nice` and
`renice` from the `timeout` crate, and deleting `blkzone`, which printed
two hardcoded disk zones for any device on any machine. So the rules are
in force and are doing useful work. Only the record of them is absent.

**Why I am asking rather than just writing them.** The project's own
instruction is that when *you* make a decision, I ask before recording it
in `design-decisions.md` rather than assume. §1006 is explicitly
attributed to you in the text that cites it, and §1005 resolved a
question that had been put to you. Writing up your reasoning from my
reconstruction of it, and signing it `Decided by: Operator`, is exactly
the thing that instruction exists to prevent — the reconstruction above
may be right in substance and wrong in emphasis, and a decision record
that misstates the emphasis is worse than an absent one.

**Options**

1. **I write both entries from the reconstruction above, marked as
   reconstructed, and you correct them.**
   *What changes:* the 33 references resolve to something; the text is
   mine until you edit it.
2. **You dictate the two entries and I paste them.**
   *What changes:* the record is yours, and costs you ten minutes.
3. **Leave them unwritten and stop citing them.**
   *What changes:* commit messages and documents stop referring to §1005
   and §1006 by number, and the rules survive only as practice.

**Recommendation: 1.** The reconstruction is well-evidenced — 33
independent citations agree with each other — and being marked as
reconstructed makes its status honest. Option 3 loses the numbering that
33 documents already depend on.

**If this is never answered:** nothing breaks. The rules keep being
followed because they are written into the code and the commit history.
The cost is that every future citation of §1005 or §1006 points at
nothing, and that a later reader trying to understand *why* a working
command was deleted has to reconstruct the argument as I just did.



## B-Q17 — [B] `sbctl` said it signed your kernel and did not. It refuses now — should the commands be deleted instead? — Status: OPEN

**Added 2026-09-15: this answer governs more than `sbctl`.**

*(Corrected the same day: I first wrote that a gate was BLOCKED on this and
that no lane could get past pre-boot. That was wrong. The audit runs only in
`scripts/pre-boot.py`, which the tree's own comment calls "a ~40-minute local
pre-flight nobody is obliged to run"; `scripts/boot-test.sh`, the shared
blocking gate, does not run it at all. I inferred the blast radius from where
the gate was WIRED rather than from what that wiring does, and one `grep` of
boot-test.sh would have settled it before I raised the alarm. Nothing else in
this entry changes -- the question is as real as it was, just not urgent.)*

`scripts/audit-cli-fabrication.py --check` is red on `main`. It names two
commands. One, `passwd`, is the audit being wrong -- its work is real
and delegated to helper crates the audit does not follow, and lane C has been
told. The other is `unshare`, and it is red for exactly the reason this
question asks about: **it refuses now, and §1006 as quoted says a command that
does not work should be deleted rather than left refusing.**

So the two readings give opposite answers for the same command:

| reading | `unshare` | the gate |
|---|---|---|
| refusing is enough -- the defect was the false claim | keep it | the audit should not flag a refusing command |
| §1006 means delete -- existence is itself a claim | delete it | the audit is right and I should act |

**The commands your answer decides, beyond `sbctl`'s six.** All were made to
refuse on 2026-09-15 for the same reason -- each stated something it had not
done -- and all could work later if the missing kernel support arrives:

| command | what it could not do | could it work later |
|---|---|---|
| `unshare` | create namespaces; `unshare(2)` is not there | yes |
| `nsenter` | enter a namespace, same gap | yes |
| `dbus-daemon`, `dbus-send`, `dbus-monitor` | speak to a bus that does not exist | yes |
| `lp`, `lprm` | reach a print spooler | yes |
| `eject` | tell a drive to open | probably not |

**Your words in `open-questions-answers.txt` point at deletion**, and I want to
be sure I am reading them the way you meant, because they were about the 2,288
fabricating commands rather than about this narrower set: *"The ones that don't
work but could work later can simply be added when we actually implement
them?"* Taken literally that settles it -- delete all of the above. I have not,
because deleting eleven more commands on my reading of a sentence written about
a different set is exactly the kind of inference worth checking first.

**If you do not answer:** the audit stays red in the optional pre-flight,
which costs whoever runs it one failed line in a report and blocks nothing. I
can clear it by teaching the audit that "refuses" is not
"states a fact it did not measure" -- those are genuinely different things and
its own error text says the first. That leaves the deletion question open
rather than answering it by default, which is why I would rather do that than
pin either command.

**In short:** `sbctl` is the tool that manages Secure Boot — the firmware
feature that refuses to start a kernel unless it carries a cryptographic
signature the machine recognises. Ours reported creating those signing keys,
and reported signing kernel images, and did **neither**: not one byte was ever
written by it. I have made those commands stop and say why. Your own rule
§1006 says a command that cannot work should be **deleted** rather than left
refusing, and I want to check you meant that here before removing six
subcommands from a security tool.

**What was happening, exactly.** `sbctl sign /boot/vmlinuz` printed
`Signing '/boot/vmlinuz'` and left the file byte-for-byte unchanged.
`sbctl create-keys` printed six lines naming key files it did not write.
`sbctl enroll-keys` printed `Proceed? [y/N]` and then never read the answer —
it "proceeded" regardless of what you would have typed. None of this is
detectable from the output; it is discovered by the firmware refusing to boot,
later, by someone with no reason to suspect this tool.

**Two different things are missing, with different prospects.**

| commands | blocked on | can it ever work here? |
|---|---|---|
| `enroll-keys`, `reset` | a way for ordinary programs to reach the kernel's key store, which exists and is real but has no door to userspace | **yes** — I have asked lane A for the door |
| `create-keys`, `sign`, `rotate-keys`, `bundle` | RSA and X.509 (the maths and the certificate format that make a signature), plus Authenticode (the specific way Windows-style binaries are signed) | **not without a cryptography library this project does not have and has not planned** |

**The options**

1. **Leave them refusing** (what I have done).
   *What changes:* `sbctl sign foo` prints `sbctl: cannot sign 'foo': this
   system has no RSA or X.509 implementation` and exits non-zero. The command
   still appears in `--help`.
2. **Delete the four that need cryptography, keep the two waiting on lane A.**
   *What changes:* `sbctl sign` becomes an unknown subcommand. `--help` gets
   shorter. Someone reading the help is never told about a capability we do
   not have.
3. **Delete all six.**
   *What changes:* `sbctl` becomes a read-only tool — `status`, `verify`,
   `list-files` — which is the half that genuinely works today.

**My recommendation: 2.** It follows §1006 exactly where §1006 clearly
applies — a command that cannot work is not kept — while not deleting two
commands that are one lane-A change away from working. The reason I am asking
rather than just doing it is that deleting subcommands from a security tool
changes what a user is told the system can do, and that is your call rather
than mine.

**If this is never answered:** the current state is safe. Nothing claims to
sign anything any more, and the refusals name what is missing. The cost of
leaving it is only that `sbctl --help` continues to advertise four commands
that cannot work on this system.

**Where it bites:** `userspace/sbctl/src/main.rs`; `roadmap.md:3835`, which
claimed this was done and now says `[~]`;
`requests/b-a-sbctl-needs-a-userspace-door-to-fs-secureboot.md`.
## B-Q18 — [B] My roadmap list is down to three huge ports. Which one, and is now the time? — Status: OPEN

**In short:** The list of jobs assigned to me has run out, except for three
very large ones. Each is "take a big program other people wrote and make it run
on SlateOS", and each is weeks of work rather than hours. I have been working
from the bug list instead, which is not empty and is producing real fixes — but
nobody has decided which of the three big jobs comes next, or whether any of
them should start yet. I would rather you picked than have me pick for you,
because the three lead the project in genuinely different directions.

**What is actually left.** `roadmap.md` has exactly three unstarted items
tagged for my lane:

| | what it means in plain terms | where it leads |
|---|---|---|
| **Rust toolchain** | SlateOS can compile its own kernel, on itself | the machine stops needing Windows to rebuild itself |
| **fastpy compiler** | the Python-to-native compiler runs on SlateOS | already part-built (initiative F); this is the rest of it |
| **WINE** | Windows programs run on SlateOS | a large existing app library, at once |

Everything else assigned to me is either done or is a bug, and bugs I can pick
up without asking.

**Why I am asking rather than choosing.** The standing rule is that I should
just start the next task, and for anything ordinary I do. These three are the
named exception: each is a *giant external port*, each takes the project
somewhere different, and the cost of starting the wrong one is weeks, not
minutes. It is also possible the right answer is "none yet" — see below.

### The options

**A. Rust toolchain first.**
*What changes:* you could rebuild the kernel from inside SlateOS instead of
from Windows. Today the OS cannot reproduce itself; after this it can.
Self-hosting is also the usual milestone at which an OS stops being an
experiment.

**B. fastpy compiler first.**
*What changes:* programs written in Python compile to native code *on* SlateOS.
This is the least risky of the three because roughly half of it already exists
and works — the cross-compiler, the linker step and the C runtime are done and
tested. It is finishing something rather than starting something.

**C. WINE first.**
*What changes:* a large body of existing Windows software becomes runnable. It
is the biggest single jump in what the OS can *do* for a user, and by far the
largest and least predictable of the three — WINE leans on a great deal of
Linux behaviour we have only partly built.

**D. None of them yet — keep working the bug list.**
*What changes:* nothing visible; I carry on fixing defects. Today that has
meant `patch` and `diff`, both of which were giving wrong answers on ordinary
files. There is no shortage of this work, and it is what makes the ports
land on solid ground when they do start.

**My recommendation is B, then D as the standing default.** B is half-built
and its remaining half is the part that unblocks writing OS components in
Python at all, which the design spec already assumes. A and C both rest on
libc and kernel surface that is still gaining features weekly — starting either
now means porting against a moving target, and re-porting later.

### If this is never answered

Nothing breaks and nothing is blocked. I will keep working the bug list, which
is option D, and the three ports stay unstarted. The cost of leaving it is not
risk but direction: the project keeps getting more correct without getting
more capable, and at some point that becomes the wrong trade. There is no
deadline on answering.


## B-Q19 — [B] Two lanes hit the same editing mistake six times in one day. Add a standing rule, and if so which? — Status: OPEN

**In short:** When we change code we usually tell a script "find this text and
replace it". Six times today, across two of the three Claude sessions, that
found *different* text than intended — or found it in three or four places when
we meant one — and the wrong edit landed silently. Both lanes independently
arrived at the same two habits that catch it. The question is whether those
habits should become a written rule all three lanes follow, which only you can
decide: rules like that live in `CLAUDE.md`, and that file is yours.

**The shortest evidence is that filing this question tripped its own rule.**
The first anchor I used to insert it matched **eight** places in this file; the
count check stopped the edit, and a more specific anchor matched one. The habit
caught its own proposal before the proposal was written down.

**The two habits.** Neither needs new tooling.

1. **Assert the match count before replacing.** A script that means to change
   one place checks that exactly one place matched, and stops otherwise. This
   caught an edit of mine today whose anchor appeared **four** times in the
   file; it would have modified an unrelated test. It also caught the filing of
   *this question* — my first anchor for it matched 8 places.
2. **Do not let the explanation and the implementation be the same action.**
   All four cases where one of us wrote a comment explaining a trap *and
   simultaneously fell into it* happened in a single pass. Both cases we caught
   had something run in between — a test, a gate, a merge — so we returned to
   the code as a reader rather than as its author. Operationally: write the
   comment, run *something*, then read it back. The run need not be related; it
   only has to cost enough attention that you come back cold.

**Why this is yours.** Lane C offered to write it down and asked whether I had
a natural home for it. The natural home is `CLAUDE.md`, and I am told not to
edit that file except when you tell me to make a specific change — a peer
suggesting it is explicitly not that. So the proposal comes here rather than
being applied. I have not filed it elsewhere either: a working-practice rule
scattered through three lanes' commit messages is how it gets re-derived next
month.

| Option | *What changes:* |
|---|---|
| **A. Add both to `CLAUDE.md`** *(recommended)* | All three lanes follow the same two habits; a wrong edit is caught by the script rather than by whoever happens to read the diff. |
| **B. Add only the count assertion** | The mechanical half becomes standard and the attention half stays folklore. Cheaper to state, and it is the half with hard evidence — six incidents, each caught or missed by exactly this. |
| **C. Leave it unwritten** | Each lane keeps its own habit. That has worked twice today and failed four times, and a new session starts with neither. |

**If this is never answered:** nothing breaks. Both lanes already use the
habits, and the incidents are recorded in commit messages. The cost is that a
future session — including a future me, with no memory of today — starts
without them and re-derives them from its own wrong edit.

**Where it bit today:** `userspace/ar/src/main.rs` (anchor matched four places,
caught), `userspace/oils/src/interp.rs` (a comment about a timing trap written
in the same pass as a smaller version of that trap, not caught until lane C
reported it), and four more in lane C's tree.

## B-Q20 — [B] `shred --random-source` is refused. Making it work means changing how the file is overwritten — which way? — Status: OPEN

**In short:** `shred` destroys a file by overwriting it several times. There is
an option to say "take the random bytes from this file instead of generating
them", and ours currently refuses that option outright rather than pretending.
Making it work properly would change the *order* in which we overwrite, and
because the tool exists to destroy data, that order matters if the machine dies
partway through. The question is which of two ways you want.

**Why it refuses today rather than ignoring the flag.** The option was parsed,
stored, and read by nothing — and its advertised default (`/dev/urandom`) was
wrong too, because nothing here opens that device. `shred` destroys the file, so
a user who asks for a particular source of random bytes and silently gets a
different one has already lost the data by the time they could notice. Refusing
before the first overwrite is the only outcome that leaves them a choice.

**Why it is not a small fix.** Our overwrite scheme is: even passes are random
bytes, and each odd pass is the **bitwise complement** (every 1 becomes a 0 and
vice versa) of the pass before it. We can produce the complement cheaply
because we generate the random bytes from a formula and can re-run it from the
same starting point. Bytes read from a *file* cannot be re-run: you would have
to either keep them or re-read them, and the usual sources (`/dev/urandom`, a
pipe) cannot be rewound.

| | *What changes* |
|---|---|
| **A. Overwrite in pairs, a chunk at a time** | Same passes, same bytes on disk at the end. What changes is the order during the wipe: we would write a chunk's random pass and its complement together before moving on, instead of sweeping the whole file once per pass. Memory stays small (one chunk). **If the power fails mid-wipe, the file is partly-wiped in a different pattern than today** — early chunks fully done, later ones untouched, rather than every chunk one pass deep. |
| **B. Keep sweeping whole passes, buffer the pass** | Nothing observable changes about order or result. **A 4 GB file needs 4 GB of memory**, so it works on small files and fails on exactly the large ones people shred. |
| **C. Leave it refused** (today) | `shred --random-source=FILE` exits 1 and the file is untouched. Everything else about `shred` works. |

**One thing I will not do without you saying so.** A fourth option is to use
the file to *seed* our formula rather than consuming it as the byte stream.
That keeps the current scheme and costs nothing — but it is **not what GNU
shred does**, and a user who supplied a specific stream of bytes would get
different bytes on disk than they asked for. Silently diverging from the
reference on a data-destruction tool is the kind of surprise this whole entry
exists to avoid, so it is listed here and not taken.

**My recommendation: A.** The memory bound in B is not a detail — it fails on
the large files that are the reason anyone shreds rather than deletes. A's cost
is a different partial-wipe pattern after a power cut, and I think that is the
lesser harm: in both cases an interrupted wipe leaves recoverable data, so
neither is safe to rely on, and A at least leaves *some* chunks completely
destroyed rather than all of them one pass deep.

**If this is never answered:** nothing breaks and nothing gets worse. `shred`
works; only `--random-source` is unavailable, and it says so plainly instead of
lying. This is a missing feature with an honest refusal, not a defect sitting
in the tree. It is in your queue because the fix has a security dimension and a
user-visible change of behaviour, not because anything is on fire.

Recorded in `known-issues.md` as
`TD-B-SHRED-RANDOM-SOURCE-IS-REFUSED-NOT-HONOURED`, which had the analysis but
was not in this file — so it was never actually in front of you.

## B-Q21 — [B] 203 of the 278 programs we have written are never installed. Should they be? — Status: OPEN

**In short:** we have written 278 small programs for this OS. 75 of them end up
on the disk image that boots; the other 203 are built, tested, and then left
behind. The reason is size: together they are bigger than the image we build.
The question is whether to make the image bigger, pick a subset deliberately,
or leave things as they are.

*(Corrected 2026-09-16: this first said "211 of 214", which counted CRATES and
called them programs. One crate — `coreutils` — holds 83 of the programs, so
counting crates understates what ships by a lot. The image also carries 14
compiled-Python utilities promoted by the fastpy block — `cat`, `ls`, `grep`,
`mv` and others — so `/bin` holds about 89 commands we wrote, not three. The
decision below is unchanged; the scale of it is not what I first said.)*

**The numbers, measured 2026-09-16** (alias lines of the form `ranlib = ar`
resolved to their producer, so these count crates rather than names).
`scripts/rootfs-bin-manifest.txt` has 75 entries:

| producer | names it supplies |
|---|---|
| `coreutils` | 71 (including `awk`) |
| `ar` | 3 (`ar`, `ranlib`, `strip`) |
| `logrotate` | 1 |

So **3 of 214 `userspace/` crates reach `/bin`**, and `/bin` is the only
place userspace binaries land: the rootfs script's only other destinations
are `/tests`, `/lib` and `/usr/share/make`, with no `/sbin` or `/usr/bin`.

**Correction, 2026-09-16, made before you read this.** An earlier version of
this entry said `awk` had *no producer anywhere in the tree*. That was my
measurement being wrong, not the tree. `awk` is
`userspace/coreutils/src/bin/awk/` — cargo's directory form for a
multi-file binary (`main.rs`, `lex.rs`, `parse.rs`, `interp.rs`, and four
more), and it passes 171 differential cases against GNU awk. My scan looked
only at `src/bin/*.rs` files and did not know about `src/bin/<name>/main.rs`,
so it reported a working implementation as absent. The count of crates
reaching `/bin` is unaffected — `awk` ships from `coreutils`, which was
already counted.

**Why, and it is a real constraint rather than an oversight.** All 276 built
binaries come to 204 MiB against a fixed 384 MiB image that already carries
~127 MiB of fastpy test fixtures. They do not fit. `IMG_SIZE` is a variable in
`scripts/create-ext4-rootfs.sh` and nothing outside that script reads it.

| | *What changes* |
|---|---|
| **A. Raise `IMG_SIZE` and stage everything that builds** | Every utility we write is on the machine and can be run. The image grows past 384 MiB — roughly 600 MiB to hold all 204 MiB with headroom. Boot-test download/copy times grow with it. |
| **B. Curate: decide which utilities earn their bytes** | Someone picks a list; the rest stay unshipped. The image stays small. Requires a judgement per program, and the list needs maintaining as programs are added. |
| **C. Leave it** (today) | `coreutils` and a couple of others ship. Everything else is a library that compiles and a test suite that passes, reachable only by a developer. |

**What you may actually be deciding.** Not disk space — it is a VM image and
the host has room. It is whether "we wrote a `logind`" means a user has one.
Today it does not, and nothing in the tree says so at the point where someone
would look; I found it only by grepping the manifest for a program I had spent
a day improving.

**My recommendation: B, but A first as a stopgap** if you want the question
answered later rather than now. A costs bytes on a VM image, which is cheap,
and buys the ability to *run* what we build — which is currently untested for
almost everything. B is the right long-term answer and needs a criterion, and I
do not think I should invent that criterion on your behalf: "which utilities
earn their bytes" is a question about what this OS is for.

**If this is never answered:** nothing breaks. The build stays green, the tests
stay green, and the work keeps accumulating out of reach. The cost is invisible
and compounding — it is effort spent on programs no one can run, and the longer
it runs the larger the pile of code whose first real execution is still ahead of
it.

**Related but different:** `deferred-questions.md` DQ1 asks which *implementation*
(fastpy or Rust) a stock install should prefer once one is proven better. That
assumes both ship. This asks whether they ship at all. Recorded in
`known-issues.md` under the image-staging entry, which names "which utilities
earn their bytes" as a real question and correctly declines to answer it — but
named it there rather than here, so it has never been in front of you.


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
