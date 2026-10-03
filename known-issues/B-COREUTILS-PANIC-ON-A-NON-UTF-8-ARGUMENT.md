## B-COREUTILS-PANIC-ON-A-NON-UTF-8-ARGUMENT (lane B, 2026-08-22) — OPEN

**In short:** On this OS a filename may contain any byte except `/` and NUL —
that is a deliberate design decision, written down in `design.txt`. But 49 of
our 84 core utilities read their command line with a Rust function that
*crashes* when an argument is not valid text. So `rm` on a file whose name
contains such a byte does not delete it, does not report an error, and does
not even reach its own code: it dies with a Rust crash message. The same is
true of `cp`, `mv`, `ls`, `find`, `grep` and most of the rest.

**Where it lives:** the first line of `main` in each of these files:

```rust
let args: Vec<String> = env::args().collect();
```

`std::env::args()`'s iterator is `self.inner.next().map(|s| s.into_string().unwrap())`
— a literal `unwrap` in std, documented to panic. The fix in each case is
`env::args_os()` plus carrying `OsString`/`&[u8]` through to wherever the
argument is used, which for a filename means all the way to the syscall. The
tree already has the pieces: `coreutils::quote::os_bytes`, `quotef_os`,
`quote_os`, and `getopt`'s byte-based error constructors.

**How to reproduce (needs QEMU — see below):** create a file whose name
contains byte `0x80`, then `rm` it.

**Scale, measured 2026-08-22 by `scripts/argv-utf8.py` (see the gate section
below):** 49 of 84 bins, plus `examples/extfloat-probe.rs` — 51 findings across
50 files, since `sh` carries two of them.

```
basename cal chmod chown cmp cp dd df diff dirname du echo ed fetch
find free grep id ln logger ls md5sum mkdir mkfifo more mv
nice nohup patch ps readlink realpath renice rm rmdir sed sh sha256sum sleep
stat strings tar tee time_cmd touch tty which xargs yes
```

**Burn-down progress — 2026-08-30: four findings left, in four files.** Naming
what is *left* is now shorter than naming what is done, so this paragraph names
that instead:

```
diff  logger  patch  ps
```

`fetch` and `sh` were the last two unblocked ones and are now done, which is
what took the count from seven to four — `sh` carried two findings, argv and
the environment.

**This paragraph used to say "Everything remaining is blocked on B-Q7, so
there is no unblocked work left in this entry; a reader looking for the next
thing to do should look elsewhere." B-Q7 was answered on 2026-09-07 and the
sentence stood for five days after.** §1005, `Decided by: Operator`:
**coreutils is the one home**, the better half of each duplicate pair survives
inside it, the duplicate crate is deleted. So the decision these four were
waiting on has been made, and "look elsewhere" was sending readers away from
work that was ready.

`diff`, `logger`, `patch` and `ps` each have a second implementation outside
`userspace/coreutils/`. Under §1005 that is no longer a question, it is a
measurement: run the pair's harness, keep the better half inside coreutils,
delete the crate. As of 2026-09-12 `diff` and `patch` are done, `ps` is
measured (coreutils' 22 passed / 0 differed against the standalone's 0 / 12),
and `logger` is the one genuine hold-out — not on B-Q7, but on **B-Q14**,
because its two implementations disagree about *where a logged message goes*,
which a differential test cannot settle.

`scripts/check-stale-blockers.py` now cross-references answered questions as
well as landed requests, so the next sentence of this shape is caught by a
gate rather than by someone wandering past.

The live count is whatever `python scripts/argv-utf8.py --check` prints; the
baseline shrinks by one line per conversion and never grows, so this paragraph
cannot silently go stale in the dangerous direction — if it disagrees with the
tool, the tool is right.

**Every conversion has uncovered unrelated bugs in the `main` it
replaced** — three in `rm`, four in `mv`, six in `cp`, four in `ln`, four in
`mkdir`, nine in `ed`, and five in `sudo` (2026-09-06, outside coreutils but
the same pattern — see
`TD-B-SUDO-DIED-BEFORE-ITS-FIRST-STATEMENT-ON-A-NON-UTF-8-COMMAND-LINE`), none of
them about UTF-8. In the first five, all of them were in code that no test touched. `ed` broke that half of the pattern and kept the other half — it had 35 unit tests and all nine defects survived them (see its section below) — which sharpens the claim rather than weakening it: the marker is not "no tests", it is *no test of the observable behaviour*. `sudo` is the sharpest instance so far, because three of its five are exploitable by an unprivileged local user. That is the argument for converting these
files properly rather than mechanically swapping `env::args()` for
`env::args_os()`: the defect is a marker for *untested `main`*, and the panic
is only the part of that a checker can see.

`env`, `kill`, `hostname` and `uname` were on this list and were fixed the same
day, which is why they are absent above. `stat`, `chmod`, `chown` and `tar`
were rewritten this week for other reasons and use `os_bytes` internally but
still *read* argv as `String`, so they remain on it.

`bc` was on this list too, and is the reason the count moved from 50 to 49.
The original figure came from `grep -l '^[^/]*env::args()' src/bin/*.rs`, which
counts a *comment* as a hit whenever it is not the first thing on the line —
and `bc` had already been converted, so its only surviving `env::args()` are
the two comments explaining what `env::args_os()` replaced. The tool disagreed
with the grep, and the tool was right, for exactly the reason its self-test
rule 3 exists: a file being *fixed* is the likeliest place in the tree for the
broken call to appear in prose.

**The correlation is the whole argument for how to fix this.** Of the 35 bins
that are already clean, **24 use `coreutils::getopt`**; of the 49 dirty ones,
**none do** — not one. `getopt` is byte-based, so a bin that goes through it
never had a reason to reach for `String` in the first place. That is a
structural cause, not a coincidence, and it means finishing the `getopt`
migration fixes the class, whereas patching 50 `main`s independently fixes 50
instances and leaves the next new bin free to reintroduce it.

**`hostname` is worth copying as the pattern**, because its fix cost nothing
extra: host names are ASCII by RFC 1123, so validating the argument *as bytes*
rejects a non-UTF-8 byte under the same rule that rejects a space. Wherever a
utility already validates its argument, doing that validation on bytes removes
the panic for free — no separate UTF-8 handling is needed at all.

**Why it survived:** the same reason as everything else in this audit. The
development host is Windows, where argv arrives as UTF-16 and a test cannot
easily produce an invalid argument; and `cargo test` never runs the binaries
at all, only their internal functions. A `main` is the least-tested line in
every one of these files. This is now the **sixth** consecutive entry arguing
for a filesystem-level harness that runs the shipped binaries inside QEMU.

**Priority:** the file-touching ones first — `rm`, `mv`, `cp`, `ln`, `ls`,
`find`, `touch`, `mkdir`, `rmdir`, `du`, `readlink`, `realpath` — because for
those the panic happens on data the *user does not control and cannot see*: a
single oddly-named file in a directory is enough to make `rm -r` abort
part-way, and a backup or a cleanup script that dies half-done is worse than
one that refuses to start.

### A gate now exists, before any of the 50 are fixed (2026-08-22)

`scripts/argv-utf8.py` is pre-push gate 4, with a 51-entry baseline recording
exactly the backlog above. It is deliberately built *first*, because fixing 49
`main`s fixes 49 instances and does nothing about the fiftieth — and the
"why it survived" paragraph above is precisely an argument that a convention
will not hold here. Nothing on the development host can produce a triggering
argument, so the only thing standing between a reintroduction and silence is a
machine that looks.

Building it first also paid for itself immediately, before fixing anything: it
is what caught `bc` being counted as broken when it had already been fixed —
the grep behind the original figure could not tell a call from a comment about
that call. A burn-down driven by that list would have spent a pass "fixing" a
file that was already correct.

It reports four spellings of the one defect — `env::args()`, `env::vars()`,
`.into_string().unwrap()`, `.to_str().unwrap()` — keyed per file *and per
rule*, so a file carrying two of them does not go green when the first is
fixed. That is not hypothetical: `sh` is the 51st finding against the 50th
file, because it reads both argv and the environment as `String`, and a
baseline keyed on the path alone would have stopped watching it the moment
argv was fixed. `env::var()` is deliberately absent: it returns `Err(NotUnicode)`
rather than panicking, so it is a behaviour bug at worst and lumping it in
would bury the one that crashes. Comments and string literals are excluded by
importing `raced-globals.py`'s Rust lexer rather than copying it — a file being
*fixed* is the single likeliest place for `env::args()` to appear in a comment
about what it replaced, and that same tool once spent a whole pass reporting a
global whose name occurred only in prose about it.

**Scope is stated as a number, not left as a silence.** The gate covers
`userspace/coreutils/` — 51 findings. The ~2750 single-file stub crates under
`userspace/*/` are *not* gated (a 2750-line baseline is a baseline nobody
reads) but they are counted and printed: **2746 findings in 2736 files**. The
reasoning is the one this whole class of tooling rests on — a checker that
quietly narrows its own scope reports a clean tree, and a clean report is the
one outcome that must never be produced by accident. That survey prints on a
bare run and under `--write-baseline`, and *not* under `--check`: it costs 30
seconds against the gated tree's 2, and in a push hook it buys four lines
nobody acts on. A gate slow enough to be resented is a gate that gets
uninstalled, which is the same silence by a longer route.

Two things were tried and removed, both worth recording because both looked
right:

- **A substring prefilter to skip lexing.** The natural literal for the first
  rule is `env::args` — which is a substring of `env::args_os`, the fix. It
  therefore admits every *converted* file too, and measured, it let 2857 of
  2902 files through. A prefilter that cannot distinguish the defect from its
  own remedy buys nothing, and its only possible error is the silent one.
- **A raw-source (unlexed) prefilter**, which would have been tighter but is
  not sound: blanking a comment can *create* a match — `env::args/*x*/()`
  becomes `env::args      ()` — so it could invent findings as well as miss
  them.

`--selftest` pins seven rules, run before `--check` in the hook and on every
invocation here. Six classify synthetic files; the seventh asserts the gated
tree is really there and non-empty, because none of the other six would notice
if `GATED` pointed at a renamed directory — the walk would return nothing,
`--check` would find nothing new, and the gate would pass forever while looking
at an empty set. Each rule was verified capable of failing by breaking what it
guards and confirming that rule and no other went red. The gate itself was
verified end to end by appending a `to_str().unwrap()` to a clean bin: `1 not
in the baseline`, exit 1, with the fix hint printed; green again on removal.

One design note that cost a debugging pass: findings are keyed
`<path>:<rule>` and split on the last colon, so the first draft's rule names —
`env::args` and `env::vars` — silently took a piece of the path with them and
produced entries like `…/rm.rs:env::21 [args]`. Not a crash: a plausible-looking
report plus a baseline that could never match. The names are now hyphenated and
a module-level `assert` forbids a colon in a rule name, so the mistake cannot
recur.

**The backlog stood at 51 findings across 50 files when the gate landed** —
that commit added no fix, only the guarantee that the number cannot grow. The
burn-down starts with the file-touching bins listed above.

### `rm` converted, and the three further bugs the rewrite uncovered (2026-08-22)

`rm` is the first of the 49 and sets the pattern for the rest: `env::args_os()`
into a `Vec<OsString>`, a hand-written byte-wise option loop over
`coreutils::getopt` (which supplies the diagnostics and the long-name
abbreviation resolution, not the iteration), and `OsString` operands carried
untouched all the way to `fs::remove_file`. Gate count 51 → 50; the baseline
was shrunk in the same commit so the file cannot regress.

**Converting it also turned up three real bugs in the lines being replaced.**
None of them are about UTF-8; all three were invisible because the old file's
tests covered `parse_args` only and there were *zero* tests of the removal
path. This is the second time in this audit that rewriting a `main` for the
argv defect has surfaced unrelated defects underneath it, which is an argument
for doing the conversions properly rather than mechanically swapping the one
function call.

1. **`--` was not an end-of-options marker.** `rm -- -foo` answered `unknown
   option: --` instead of deleting the file named `-foo`. `--` is the only way
   to name a file whose name begins with `-`, so the utility had no way at all
   to delete one.
2. **`-f` suppressed *every* error, not just absence.** POSIX says `-f`
   ignores a *nonexistent* operand; ours ignored a permission denial, a
   non-empty directory, a read-only filesystem — anything. `rm -f important`
   on an undeletable file printed nothing and exited 0, i.e. reported success
   for work it had not done. That is the worst shape a bug can have in a
   delete utility: silence that reads as completion.
3. **Symlinks were followed.** The old code used `Path::exists` and
   `Path::is_dir`, which both *resolve* the link. So `rm dangling-link` said
   "No such file or directory" about a link that was plainly there and could
   have been unlinked, and `rm -r link-to-dir` chose its recursive branch by
   looking at the *target's* type. Now `fs::symlink_metadata`, which stats the
   link itself.

**What `rm` still does not do**, deliberately kept out of the conversion
commit so that one commit is one logical change:

- **No root failsafe.** GNU `rm` refuses `rm -rf /` unless
  `--no-preserve-root` is given. Ours has nothing: `--preserve-root` and
  `--no-preserve-root` are accepted by the option parser (so that abbreviation
  resolution stays correct) and then rejected as `not implemented`. Until this
  lands, the single most destructive typo in Unix has no guard on this OS.
  Fixing it needs a canonicalised-path comparison against `/`, which needs
  `realpath` semantics that are themselves on the burn-down list — so it is
  sequenced after them rather than forgotten.
- **Seven GNU options unimplemented:** `-d`, `-i`, `-I`, `-v`,
  `--interactive[=WHEN]`, `--one-file-system`, `--preserve-root[=all]`. They
  are *rejected by name* rather than ignored. That choice matters for `-i`
  specifically: silently ignoring a request to be asked before each deletion
  would convert it into deleting without asking, which is the one direction a
  user of this utility cannot afford to be surprised in. An error costs a
  retype; a wrong default costs the files.

**On testing the fix from Windows.** The obvious regression test — an operand
containing byte `0x80` — is `#[cfg(unix)]` and therefore does not run on the
development host, which is precisely the blind spot that let the bug live. So
`rm` also carries `#[cfg(windows)]` twins keyed on an **unpaired surrogate**
(a UTF-16 code unit in `0xD800..=0xDFFF` with no partner, which Windows will
hand you in `argv` and which no `String` can represent). `OsString` stores it
as WTF-8, `to_str()` returns `None`, and `env::args()` panics on it — the same
`unwrap`, in the same std function, reached by a different route. Both twins
pass here, so the fix is genuinely covered on the machine the work is done on.
**Every remaining conversion should carry the same pair**; a `#[cfg(unix)]`-only
regression test for this defect is a test that never runs.

### `mv` converted, and the four further bugs the rewrite uncovered (2026-08-22)

Same shape as `rm`: `env::args_os()`, a byte-wise option loop over
`coreutils::getopt`, `OsString` operands carried to `fs::rename`. Gate count
50 → 49; baseline shrunk in the same commit. 38 tests, up from 12, and the
move path — which had **no tests at all** — now has fourteen.

Also `cfg(unix)`+`cfg(windows)` twins of the non-UTF-8 regression test, per the
rule established for `rm` above, and the binary was compiled for the *real*
target (`cd userspace && cargo +nightly build -p coreutils --bin mv`) because
the target is `unix` and the development host is not — so the `#[cfg(unix)]`
half of the source is never compiled by `cargo test` here. That step belongs to
every remaining conversion; without it a typo in a `#[cfg(unix)]` arm is
invisible until it breaks the boot test for all three lanes. (The `+nightly` is
load-bearing and undocumented outside lane C's
`TD-C-A-ZONE-BUILD-FAILS-UNLESS-YOU-KNOW-TO-SAY-NIGHTLY` below; a plain `cargo
build` in a zone fails with a message naming a flag `CLAUDE.md` tells you not
to pass.)

**The four bugs**, in the lines the rewrite replaced:

1. **`--` was not an end-of-options marker.** `mv -- -foo bar` answered
   `unknown option: --`, so a file whose name begins with a dash could not be
   moved at all.
2. **`-f` suppressed the diagnostic but not the failure.** The `-f` branch
   skipped the `eprintln!` and *still* set the exit status to 1, so `mv -f a b`
   on a failure printed nothing and exited non-zero: the caller was told
   something went wrong and given no way to find out what. `-f` has never meant
   that anywhere. In GNU `mv` it suppresses the *prompt* that `-i` would raise
   before overwriting; this `mv` never prompts, so `-f` is now accepted and
   inert — which is exactly GNU's behaviour in the absence of `-i`, and is why
   it now records no flag at all.
3. **A source ending in `..` moved something the user never named.** The target
   was `dest.join(src.file_name().unwrap_or_default())`, and `Path::file_name`
   is `None` when the last component is `..` — so `unwrap_or_default()` gave an
   *empty* name, `dest.join("")` collapsed back to `dest` itself, and
   `mv a/.. dst` asked the kernel to rename `a`'s **parent directory** onto
   `dst`. If `dst` was an empty directory that succeeds: the user asks to move
   something *into* `dst` and instead the directory they were standing in is
   moved *onto* it. Reachable from an ordinary glob (`mv */.. dst`). Now
   refused with a diagnostic, with a test asserting the parent is still there.
4. **The cross-filesystem fallback silently turned a symlink into a copy of its
   target.** When `rename` fails with `EXDEV` (the kernel refusing to rename
   across a filesystem boundary), `mv` must copy and then unlink. The fallback
   used `fs::copy`, which *follows* symlinks — so moving a symlink across a
   boundary read the file it pointed at, wrote those bytes at the destination
   as an ordinary file, and deleted the link. A symlink went in and a full copy
   came out, with no message. It now recreates the link with `symlink(2)` and
   only then unlinks. A *dangling* symlink hit the same path and failed with
   `No such file or directory` naming the link — which reads as "the link is
   missing" when the link was right there.

   The fallback is also no longer entered for *every* rename failure, only for
   a genuine cross-device one (`EXDEV` / `ERROR_NOT_SAME_DEVICE`, checked by
   errno first and `ErrorKind::CrossesDevices` second — errno first because our
   own target's libstd may not map the variant yet, and a rename that *is*
   cross-device must not become a hard failure because a classification is
   missing). Previously `mv nonexistent dst` failed `rename`, fell through to
   `fs::copy`, and reported the *copy's* error, which happened to read the same
   but need not have.

**What `mv` still does not do:**

- **Moving a directory across a filesystem boundary.** It reports that this is
  not implemented rather than attempting a partial job. Doing it properly needs
  a recursive copy preserving modes, symlinks and hard links; doing it badly
  loses data quietly, which is why the honest error is the interim answer. This
  was already the old behaviour and is unchanged.
- **Twelve GNU options**, all rejected by name rather than ignored: `-b` /
  `--backup`, `-i` / `--interactive`, `-n` / `--no-clobber`, `-t` /
  `--target-directory`, `-T` / `--no-target-directory`, `-u` / `--update`,
  `-v` / `--verbose`, `-S` / `--suffix`, `-Z` / `--context`, `--debug`,
  `--exchange`, `--strip-trailing-slashes`. Silently ignoring `-n` would
  overwrite a file the user asked to be left alone, and ignoring `-i` would
  skip a confirmation they asked for; for this utility both mistakes are
  unrecoverable, and an error costs only a retype. All twelve stay in the
  long-option table so abbreviation resolution keeps working — there are tests
  that `--v` is ambiguous between `--verbose` and `--version`, and `--n`
  between `--no-clobber` and `--no-target-directory`, and those are the tests
  that fail if someone prunes the table to what is actually handled.

### `cp` converted, and the six further bugs the rewrite uncovered (2026-08-22)

Same shape again: `env::args_os()`, a byte-wise option loop over
`coreutils::getopt`, `OsString` operands, `cfg(unix)`+`cfg(windows)` twins of
the non-UTF-8 regression test, and a build for the real target
(`cd userspace && cargo +nightly build -p coreutils --bin cp`). Gate count
49 → 48. 36 tests, and — as with `mv` — the copy path had **no tests at all**
before, so all of its coverage is new.

Two of the six are severe enough to state first: **`cp -r` could not terminate**
on a directory containing a symlink to an ancestor, and **`cp -r a a` copied a
directory into itself without limit.** Both fill the disk.

**The six bugs**, in the lines the rewrite replaced:

1. **The recursive walk followed symlinks, so `cp -r` did not terminate.**
   `copy_dir_recursive` asked `src_path.is_dir()` — which follows a symlink —
   and then handed non-directories to `fs::copy`, which also follows. So
   `ln -s .. loop` inside a copied tree made the walk descend into the parent,
   find `loop` again, and descend again, for ever, writing a copy of the whole
   tree at each level until the disk filled. Without a loop it was still wrong
   in a quieter way: every symlink came out as a full copy of the file it
   pointed at, and a *dangling* one aborted the copy with an error naming the
   link rather than what it pointed at. The walk now uses
   `DirEntry::file_type`, which is the call that does **not** follow, and
   recreates links with `symlink(2)`.

   The behaviour now matches GNU: with `-r`/`-R` and none of `-H`/`-L`/`-P`,
   symlinks are not dereferenced anywhere, *including* the operands named on
   the command line — but plain `cp link dst` with no `-r` still dereferences,
   because that is the one case where GNU does. There is a test for each half,
   since "matches GNU" is otherwise indistinguishable from "we happened to pick
   the same answer".
2. **`cp -r` would copy a directory into itself, without limit.** `cp -r a a`
   and `cp -r a .` both resolve to a destination *inside* the source, so the
   walk kept finding the copies it had just written. GNU refuses this by name
   (`cannot copy a directory into itself`) and now so does this one. The check
   resolves both paths first, so it also catches the spellings that are not
   textually equal — `cp -r a ./a/../a` is the same directory and is refused,
   which is what `is_inside_sees_through_a_different_spelling` pins.
3. **A copied directory came out world-readable.** `create_dir_all` applies the
   process umask, so a source directory mode 0700 — the mode that means "only I
   can look in here" — produced a copy at 0755. Copying a private tree into a
   shared location therefore published it, with no message. The mode is now
   carried over, and applied **after** the directory has been filled rather
   than before: a source mode of 0500 applied first would lock the copying
   process out of the directory it is still writing into.
4. **`--` was not an end-of-options marker** — same as `rm` and `mv`, same
   consequence: a file whose name begins with a dash could not be copied.
5. **A source ending in `..` or `/` copied into the wrong place.** The same
   `file_name().unwrap_or_default()` shape as `mv` bug 3, with a different
   outcome: `dest.join("")` collapsed to `dest`, so `cp -r a/.. dst` merged the
   *contents* of `a`'s parent into `dst` instead of creating anything under it.
   **The old test suite asserted this**, in a case named
   `target_source_with_no_filename_into_dir`, which is why it survived: the
   behaviour was pinned, so it read as intentional. A test can preserve a bug
   as effectively as it can prevent one; the fix included deleting that
   assertion and writing `a_source_with_no_file_name_is_refused_not_collapsed`
   in its place.
6. **One unreadable file abandoned the rest of the copy.** The walk propagated
   the first error with `?`, so a single permission-denied file part-way
   through a large tree stopped everything after it — and the files already
   written stayed, so the result was a silent partial copy with a non-zero
   status and one message. It now continues past each failure, reports every
   one, and returns non-zero if any failed. (Same anti-silence direction as the
   batch-error rule in `CLAUDE.md`: report the worst error, do not stop at the
   first.)

**What `cp` still does not do:**

- **Every option but `-r`/`-R`**, all rejected by name rather than ignored:
  `-a`, `-b`, `-d`, `-f`, `-H`, `-i`, `-l`, `-L`, `-n`, `-p`, `-P`, `-s`, `-S`,
  `-t`, `-T`, `-u`, `-v`, `-x`, `-Z` and their long spellings. Ignoring `-p`
  would silently drop the permissions the user asked to preserve; ignoring `-n`
  would overwrite a file they asked to leave alone; ignoring `-i` would skip a
  confirmation. All of GNU's 31 long options stay in the table so abbreviation
  resolution stays correct — including the **deprecated `--path` alias**, whose
  only remaining job is to keep `--pa` ambiguous with `--parents`. Tests pin
  that, and that `--r` is ambiguous between `--recursive` and `--reflink`; they
  are what fails if someone prunes the table to what is implemented.
- **Hard links are not preserved.** A tree containing two names for one inode
  copies as two independent files, silently doubling its size. GNU needs `-a`
  or `-d` for link preservation, neither of which exists here, so this is a
  missing feature rather than a deviation — but it is the kind that is only
  noticed by whoever runs out of disk.
- **No `-p`**, so ownership, timestamps and the setuid/setgid bits are not
  carried over on *files*. Directory modes are (bug 3), because leaving those
  wrong is a confidentiality bug rather than a fidelity one.

### `ln` converted, and the four further defects the rewrite uncovered (2026-08-22)

Gate count 49 → 47 (`cp` took it to 48). 32 tests, up from 10, and — as with
`mv` and `cp` — the *action* path had none at all before, so all of its coverage
is new.

**Unlike `rm`, `mv` and `cp`, none of `ln`'s defects produced a silently wrong
result.** Every one was a refusal of a valid command or a mangled message, not a
link pointing somewhere the user did not ask for. That is worth recording rather
than dressing up: it is the first of the four conversions where the argv defect
was not also a marker for something that loses data, and it is evidence about
what the remaining 47 are likely to hold — not every one of them is `cp`.

**The four defects**, in the lines the rewrite replaced:

1. **No long option worked at all, including `--help`.** The parser treated any
   argument beginning with `-` as a bundle of short options and iterated its
   characters, so `--help` tripped on its own second `-` and answered
   `unknown option: --` — an option nobody typed, and no hint that `--help`,
   `--version` and `--symbolic` were simply unreachable. The same line is why
   `--` was not an end-of-options marker, so a file whose name begins with a
   dash could not be linked either.
2. **Filenames went into diagnostics unquoted**, between two literal `'` marks,
   so a file called `a⏎ln: /etc/shadow: Permission denied` made `ln` appear to
   print a second line it never wrote. Same fix, same reason, as the tree-wide
   sweep that put file names through `quote`.
3. **Three of GNU's four operand forms were refused.** Only
   `ln TARGET LINK_NAME` worked; `ln TARGET` and `ln TARGET... DIRECTORY` both
   answered `expected exactly two arguments`. The third form is not exotic —
   `ln -s ../lib/libfoo.so .` is it — and a user who typed it was told they had
   made a mistake. Forms 1–3 work now; the fourth (`-t DIRECTORY`) needs `-t`,
   which is rejected by name.
4. **A relative symlink target was misjudged on Windows.** The `#[cfg(windows)]`
   arm has to decide at creation time between a file link and a directory link,
   and asked `Path::new(target).is_dir()` — resolving `target` against the
   *current* directory. A symlink's text is resolved against the *link's own*
   directory, so `ln -s ../thing sub/link` asked the wrong question. Host-only
   (the shipping target takes the `#[cfg(unix)]` branch), but a bug in code that
   exists, and the fix was one `join`.

**What `ln` still does not do:**

- **Every option but `-s`/`--symbolic`**, all rejected by name: `-b`, `-d`/`-F`,
  `-f`, `-i`, `-n`, `-r`, `-t`, `-v`, `-L`, `-P`, `-S`, `-T` and their long
  spellings. `-n` is the one that would be dangerous ignored: it asks for a
  `LINK_NAME` that is *itself* a symlink to a directory to be treated as a plain
  file, so ignoring it puts the new link inside the pointed-at directory instead
  of replacing the link — a link somewhere the user never named, with no message.
- **The `-t DIRECTORY` operand form**, which depends on `-t`.

**Two things this conversion did better than the three before it, and which the
remaining 47 should copy:**

- **The GNU behaviour was measured, not recalled.** Every diagnostic and every
  operand form above was read off GNU coreutils 9.4 through WSL before being
  implemented, including the long-option table's *declaration order* — obtained
  with the instrument documented in `getopt::Program::resolve_long`, namely that
  an empty prefix matches every option, so `ln --=x` prints the whole table in
  order. Three ambiguities are pinned by tests against the measured strings:
  `--s` between `--suffix` and `--symbolic`, `--n` between `--no-dereference`
  and `--no-target-directory`, `--v` between `--verbose` and `--version`. The
  first is the one that matters — a table pruned to the single implemented
  option would turn `ln --s` from an error into a symbolic link.
- **The target-side check is `cargo +nightly clippy --all-targets`, not
  `cargo +nightly build`.** `build` does not compile `#[cfg(test)]` code, so the
  `#[cfg(unix)]` *tests* — which are the regression tests for the whole
  burn-down, and which never run on this Windows host — were still never
  type-checked by the step introduced for `mv`. `clippy --all-targets` compiles
  the test harness for `x86_64-slateos` and so covers them. Use it in place of
  the plain build from here on.

### `mkdir` converted, and the four further defects the rewrite uncovered (2026-08-22)

Gate count 47 → 46. 27 tests, up from 7, and — as with `mv`, `cp` and `ln` — the
*action* path had none at all before, so all of its coverage is new.

Like `ln`, and unlike `rm`, `mv` and `cp`, **none of these defects made `mkdir`
create a directory in a place the user did not ask for**. All four were refusals
of valid command lines, or messages in the wrong shape. That is now two
conversions in a row where the argv defect was not a marker for something that
loses data.

**The four defects**, in the lines the rewrite replaced:

1. **No long option worked, including `--help` and `--parents`, and `--` was not
   an end-of-options marker.** `parse_args` compared each whole argument against
   the literal string `"-p"` and treated everything else beginning with `-` as
   unknown. So the long spelling of the one option the program *has* was refused;
   `--help` was refused; a directory whose name begins with a dash could not be
   created at all, there being no way to stop option parsing; and short options
   could not be bundled, so `mkdir -pv` failed as one unknown option.
2. **The failure message used the wrong quoting style** — `quoteaf_os`, straight
   `'a'`, where GNU `mkdir` uses locale quoting, curly `‘a’`. See the section
   below; this is the finding worth carrying forward, not the bug.
3. **`missing operand` carried no `Try 'mkdir --help' for more information.`**,
   which was the only pointer a user had to a `--help` that — see defect 1 — did
   not work anyway.
4. **Unknown options were reported in a shape no other utility uses**:
   `mkdir: unknown option: -q`, against GNU's `mkdir: invalid option -- 'q'`
   plus the referral. Going through `getopt` fixes this for the same reason it
   fixes ambiguity handling — the wording is the library's, measured once,
   rather than each bin's own guess.

**The quoting style is a property of the individual message, not of the utility,
and `mkdir` is the odd one out.** This is the reusable finding. Measured under
`LANG=C.UTF-8`, GNU coreutils 9.4:

| Message | Marks | Our function |
|---|---|---|
| ``mkdir: cannot create directory ‘a’: File exists`` | curly | `quote` / `quote_os` |
| ``mkdir: created directory 'v1'`` (`-v`) | straight | `quoteaf` / `quoteaf_os` |
| ``rmdir: failed to remove 'nosuch'`` | straight | `quoteaf_os` |
| ``cp: cannot stat 'nosuch'`` | straight | `quoteaf_os` |
| ``rm: cannot remove 'nosuch'`` | straight | `quoteaf_os` |
| ``touch: cannot touch '/nope/x'`` | straight | `quoteaf_os` |
| ``ln: failed to create hard link 'g'`` | straight | `quoteaf_os` |

The two `mkdir` rows are the point: one program, two styles, in the same run.
`quoteaf_os` was not a careless choice in the old file — it is what all five of
`mkdir`'s nearest neighbours use, which is exactly why it is easy to get wrong
in both directions. **The rule for the remaining 46 is to measure the specific
message, not to copy the utility next door**, and `mkdir.rs` now carries a test
(`the_failure_message_uses_curly_marks`) that fails if someone harmonises it.

**What `mkdir` still does not do:**

- **Every option but `-p`/`--parents`**, all rejected by name: `-m`/`--mode`,
  `-v`/`--verbose`, `-Z`/`--context`. `-m` is the one that would be harmful
  ignored — `mkdir -m 700 ~/.ssh` would silently produce a 0755 directory, i.e.
  a directory the user asked to be private, made world-readable with no message.
  Implementing it properly needs a symbolic-mode parser (`u=rwx,go=`); the only
  one in the tree is private to `chmod.rs`. **Lifting that into a shared
  `coreutils::mode` module is the prerequisite**, and it unblocks `-m` in
  `mkdir`, `mkfifo` and `install` at once, so it is worth doing once rather than
  copying the parser a third time.
- `--verbose` is unimplemented only for scope discipline; it is safe to ignore
  and trivial to add, and it is refused rather than ignored purely so that no
  option in this program is silently dropped.

**Measured, not recalled**, per the rule the `ln` section sets out. The whole
long-option table came from `mkdir --=x` and is pinned by a test asserting the
exact string, so the *order* — which is observable, and which the ambiguity
messages depend on — cannot drift:

```text
mkdir: option '--=x' is ambiguous; possibilities: '--context' '--mode'
'--parents' '--verbose' '--help' '--version'
```

The ambiguity that matters is `--v` between `--verbose` and `--version`: a table
pruned to the single implemented option would turn `mkdir --v` from an error
into a version banner. `--p` is measured as *unambiguous* (`mkdir --p q` works),
and there is a test for that too, since it is the abbreviation a user is most
likely to type.

### The option tables were wrong in five bins, and now there is a gate (2026-08-22)

**In short:** Each converted utility carries a copy of the real GNU program's
list of long options (`--verbose`, `--parents`, …). The copy has to be exact,
including options we deliberately do not implement, because that list — not the
set of options we handle — is what decides whether a shortened option like
`--v` is an error or a command. We had been writing those lists from memory.
Five of them were wrong, in five different ways, and none of the mistakes was
visible by reading the file. There is now a script that asks the real utility
for its list and compares, and a push gate that runs it.

**Why the list must include options we do not implement.** `getopt_long` lets a
user shorten a long option to any prefix that names exactly one entry. Whether
`--v` names exactly one entry depends on *every* entry, so an option missing
from the list does not merely go unrecognised — it stops making its neighbours
ambiguous. Drop `--verbose` from `rm`'s list and `rm --v file` no longer fails;
it matches `--version`, prints a banner, and deletes nothing.

**How they were wrong.** All five were found by measurement, none by review:

| Bin | The mistake | What a user would have seen |
|---|---|---|
| `mv` | had `--exchange`, which is a *newer* upstream's option; lacked `--no-copy`, which the reference has | `mv --no-c` acted on `--no-clobber` where GNU calls it ambiguous |
| `cp` | had `--keep-directory-symlink`, which is a **`tar`** option and has never been a `cp` one | an accepted abbreviation of an option `cp` does not have |
| `rm` | lacked `-presume-input-tty` | `rm ---p` unrecognised where GNU resolves it |
| `split` | lacked `-io-blksize` | `split ---i` unrecognised where GNU resolves it |
| `csplit` | exactly the right names, in an order no getopt produces | `csplit --s` listed `'--suffix-format'` first where GNU lists `'--silent'` |

Two of those deserve enlarging on.

**Option names may begin with a hyphen.** GNU's `rm` table literally holds the
name `-presume-input-tty`, so it is typed `---presume-input-tty`, with three
dashes. `split` has `-io-blksize` the same way. These are internal knobs
deliberately made awkward to spell, and the leading hyphen is not a typo in
this file — it is the name. They cannot collide with any ordinary `--name`
(nothing else starts with `-`), so listing them changes nothing except that a
`---` prefix now resolves as GNU resolves it. `rm.rs` carries a test in each
direction: `---p` reaches the hyphen-named option, and `--p` still means
`--preserve-root` alone.

**Order is observable, not cosmetic.** glibc reports `pfound` — the *first*
table entry that matched — so two tables holding the same names in different
orders name different options in their diagnostics. `csplit`'s list had been
sorted into a plausible-looking order rather than GNU's, which changed nothing
about *what* resolved and everything about what the error message said.

**The gate.** `scripts/getopt-ambiguity-check.py` runs two comparisons per bin:

1. **The table.** `<util> --=x` prints GNU's whole table — the empty prefix
   matches every entry, so the ambiguity message lists all of them in
   declaration order. Ours is compared to that as a *sequence*.
2. **Every abbreviation.** For each distinct proper prefix of each name, our
   verdict (resolves / ambiguous / unrecognised) is compared with GNU's.

The second cannot find everything on its own, which is why the first exists:
the prefixes it probes are generated from *our* names, so an option GNU has and
we lack is never typed and never measured. That blind spot is exactly what hid
`rm`'s and `split`'s missing entries through an earlier, prefix-only version of
the script — it reported zero disagreements on all 24 tables.

Run it with `python scripts/getopt-ambiguity-check.py [util…]`. It needs a GNU
userland, finds WSL itself on this host, and exits 0 with a note where there is
none. **Unlike the other pre-push gates it has no baseline**: all 24 tables
agreed once these five were fixed, so it starts at zero and is simply strict.
It is pre-push gate 5, scoped to the bins a push actually rewrites (~2s each)
unless `getopt.rs` itself changed, in which case it sweeps all of them (~35s).

### `getopt` called an alias ambiguous against itself (2026-08-22)

**In short:** Some utilities have two spellings of one option — `rmdir --path`
and `rmdir --parents` are the same thing. Our option matcher treated them as two
different options, so it refused `rmdir --p` as "ambiguous" when the real
`rmdir` accepts it. Fixed, with the fix driven by measurement rather than by
guessing what "ambiguous" ought to mean.

**What glibc actually does.** It judges ambiguity by `struct option`'s `val`
field, not by name: two spellings sharing a `val` are one option, and a prefix
matching only those resolves. Three measurements pinned the behaviour, and each
one contradicted a plausible guess:

| Measured | Result | The guess it kills |
|---|---|---|
| `rmdir --p` | resolves | "two matching names is ambiguous" |
| `cp --p` | `ambiguous; possibilities: '--parents' '--preserve'` | "aliases are hidden" — `--path` matched, and is simply not a second *candidate* |
| `rmdir --pa=1` → `'--path'`, `cp --pa=1` → `'--parents'` | disagree | "the alias resolves to its target" |

That last row is the subtle one. The two utilities name *different* spellings
for the same abbreviation, purely because their tables are in different orders:
glibc returns the first entry that matched and never the alias's target. So the
fix must **not** canonicalise — reporting the target would be one line shorter
and would silently name an option the user did not type.

`Program::resolve_long_aliased` takes an extra `&[(&str, &str)]` mapping a
spelling to the option it *is*, compares each later match against the first
one's identity (which is what glibc does, and is what decides the shape of the
list), and returns the entry it matched. `resolve_long` delegates to it with an
empty map, so the 23 call sites with no aliases needed no change and there is
still only one implementation. `cp` was the one live bin affected; `rmdir` would
have been wrong the same way the moment it was converted.

### `ed` converted, and the nine further defects the rewrite uncovered (2026-08-30)

Gate count 8 → 7. Commit `0a337ffdd`. 44 tests, up from 35 — and **35 is the
number that matters here**, because this is the first conversion where the
replaced `main` was not untested. See "what the 35 tests were" below.

**For an editor the argv panic was the smaller half.** The buffer was
`Vec<String>` filled by `fs::read_to_string`, so a file holding one byte that is
not valid UTF-8 could not be opened at all: the read returned `InvalidData`,
which the old `main` could not distinguish from "no such file", and answered by
printing `0` and presenting an **empty buffer**. A subsequent `w` then truncated
the file to nothing. That is silent data loss on a file the user asked to edit,
and it is why this went all the way to `Vec<Vec<u8>>` — argv, the name handed to
the syscall, the buffer, the substitution and stdout are all bytes now — rather
than swapping one `env::args()` call. GNU `ed` is byte-clean throughout;
measured, a name and a content both holding `0x80` round-trip unchanged.

So this conversion lands on the `rm`/`mv`/`cp` side of the split, not the
`ln`/`mkdir` side: the argv defect was again a marker for something that loses
data.

**The nine defects**, one line each; the full write-up with the measurements is
in `userspace/coreutils/src/bin/ed.rs`'s module header, which is the copy to
keep current:

1. **No options at all**, `--help` and `--version` included — the first argument
   was the file name whatever it looked like, so `ed -s f` opened a file called
   `-s`.
2. **Every diagnostic went to stdout and the status was always 0.** GNU splits
   them (`?` and the `-v` explanation to stdout, the OS's own complaint to
   stderr) and grades 0 / 1 / 2. A script could not tell success from failure.
3. **`,p` printed one line.** A leading `,` was read as "no address", so the
   documented "print all lines" printed the current one. `%` was not understood.
4. **`=` printed the line *count*, never the addressed line.**
5. **An out-of-range or reversed address was silent** — `0p`, `9p`, `4,2p` all
   printed nothing and continued, where GNU answers `?` and stops.
6. **`s` echoed the line it changed**, so a script's `1,$s/…/…/` got the whole
   file printed back at it.
7. **A trailing `\r` was stripped from every line unconditionally**, so `w`
   silently rewrote a CRLF file as LF. GNU does this only under
   `--strip-trailing-cr`.
8. **A file whose last line had no newline was miscounted** and got no
   `Newline appended` notice.
9. **A command suffix was ignored rather than refused** — `1pX` printed the line.

**What the 35 tests were, and why they caught none of this.** Every one of them
called a helper directly: `nth_line_basic`, `insert_at_out_of_range_clamps_to_end`,
`substitute_global`, `parse_sub_escape_delim`, `parse_range_with_dollar`, and so
on. Not one ran a command through the dispatch loop, and not one looked at
stdout, stderr, the exit status, or the file on disk. The helpers were correct;
all nine defects live in the layer above them. **A utility's tests can be
numerous, passing and entirely beside the point** — the shape to look for is a
test module that never produces the program's own output.

**The harness compares four observables, not three.** `scripts/ed-diff.sh` runs
140 cases against GNU ed 1.20.1 under WSL and checks stdout, stderr, the exit
status **and the bytes left on disk**. The fourth is not belt-and-braces: the
truncation bug at the top of this section agreed with GNU on all three of the
usual observables and disagreed only on the file. Any harness for a program that
*writes* wants that column.

Every case runs in both stdin kinds, because GNU's governing predicate is
`is_regular_file(stdin)` and **not `isatty`** — it decides the `script, line N:`
prefix, whether `ed` stops at the first error, and whether a bad operand is
fatal. Measuring rather than recalling also produced four findings that no
amount of reading would have: `p`/`n`/`l` are an *additive bitfield* rather than
three styles (`1nl` prints numbered *and* listed); the `l` fold margin is 72
printed columns tested *before* each escape, with no look-ahead; an unterminated
`s` implies print; and a failed open and a failed read are graded differently (a
directory operand opens, then fails to read — two lines, status 1).

**The harness had a bug of its own, worth carrying to the next one.** It built
each script through `$(...)`, which strips trailing newlines — and for `ed` that
changes the exit status (`printf '1d\nq\n'` exits 1; the same text without the
final newline exits 2). It manufactured 14 differences in code that was already
correct. Scripts are now passed down `%b`-escaped and expanded at the point of
use. **`$(...)` is not a faithful carrier for test input**, and the failure is
worst where the trailing newline is semantic, which for a line editor it is.

**Still missing: regular expressions.** `s` matches a literal string and there
are no `/RE/` addresses, no `g`/`v`/`G`/`V`. That is
`TD-B-ED-HAS-NO-REGULAR-EXPRESSIONS`; the fix is `ere::bre`, which `sed` already
uses, and the 14 cases that will turn `KFIXED` when it lands are already in the
harness, so the entry cannot be silently outlived.
*(Superseded 2026-08-30: regular expressions landed, that entry is closed, and
the harness cases became ordinary `run_pipe` comparisons. What is still missing
is eight commands — `TD-B-ED-IS-MISSING-EIGHT-COMMANDS`.)*
The scope calls — what this
`ed` refuses rather than guesses at, and why it prints file names raw instead of
quoted — are `design-decisions.md` §713.

**`ed` is also the first bin whose option table the gate could not read**, since
GNU `ed` links its own argument parser rather than glibc's `getopt_long` and so
prints no `possibilities:` list. That is written up under
`scripts/getopt-ambiguity-check.py`'s `OWN_PARSER`, with the cross-validation
that made the second measurement path sound: `--help` is a **one-sided** readout
of an option table — checked on six glibc utilities, it never named an option the
parser lacked, but routinely omitted ones it had.
