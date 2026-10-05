## §322 — One in-tree regex engine, shared as a crate, and BRE reaches it by translation

**Date:** 2026-08-16
**Decided by:** Claude (autonomous)

**In short:** five programs in this tree match regular expressions — the shell's
`[[ =~ ]]`, and `grep`, `sed`, `awk`, `expr`. The shell had a real engine; the
other four searched for the pattern as a literal substring, so `grep '^posix'`
found nothing and `sed 's/^/E:/'` changed nothing. The decision is *where the
one real engine should live*: it moves out of the shell into a `userspace/ere`
crate that all five use, rather than being copied, re-implemented, or replaced
by a crate from crates.io. A second decision rides along: the four utilities
mostly want POSIX **Basic** regular expressions, which the engine does not
speak, and BRE is handled by **translating it to Extended** rather than by
writing a second parser.

### Why it had to be one engine and not four

`[[ $f =~ ^a ]]` in a script and `grep '^a'` on the same file, on the same
machine, giving different answers about whether the file starts with `a` is not
a cosmetic inconsistency — it is the shell and the utilities disagreeing about
what a pattern *means*, in a system where scripts routinely use both to ask the
same question. Bracket expressions, anchors, interval syntax, the exact set of
`[:classes:]`, whether `.` matches an undecodable byte: every one of those is a
place two implementations drift, and none of them announces the drift.

This is the same move `tzrules`, `yamldoc`, `textfmt`, `textfind` and `byteread`
already made here, and the reasoning is the strongest instance of it. In each of
those cases several consumers had grown a subtly-wrong copy of one component; in
this case four of the five had not grown one at all, which is worse and was
harder to see.

### Options

**A — Leave the four as they are.** *What changes:* nothing; `grep '^a'` keeps
finding nothing. Rejected on sight. Recorded only because it is what the tree
did for months without anyone noticing, and the reason is worth remembering:
each program passed its own tests, because every test asserted the substring
behaviour the program had.

**B — Have `coreutils` depend on `oils`.** *What changes:* `grep` links the
whole shell — lexer, parser, interpreter, job control — to get a regex.
Rejected: the dependency edge points the wrong way (a utility should not depend
on the shell that calls it), it drags osh's build time and binary size into
every coreutil, and it makes any change to the shell a rebuild of `grep`.

**C — A third-party regex crate (`regex`, `regex-lite`).** *What changes:*
patterns follow Rust's regex dialect rather than POSIX's. Rejected on
*semantics*, not on the dependency: `regex` implements Perl-ish syntax with
leftmost-first alternation, and POSIX requires leftmost-**longest**; it has no
BRE mode, no POSIX `[[:class:]]`-with-collation handling, and its `.` is defined
over `char`, not over bytes-that-may-not-decode — which is exactly what this
tree's path model requires, since a path admits every byte but `/` and NUL. We
would have had to write a translation layer whose job was to hide a dialect
mismatch, which is more subtle work than owning the engine. Adopting it would
also mean a shell whose `[[ =~ ]]` silently changed behaviour.

**D — Extract osh's engine into `userspace/ere` and depend on it from both.**
*What changes:* one Pike VM, one character model, one answer to `[a-z]`.
**Chosen.**

### Why the extraction is safe to make

The engine being moved is not a sketch: it is a Pike VM (Thompson NFA
simulation) with captures, `O(len(input) × len(prog))` and immune to
catastrophic backtracking, already defined over a byte-safe character model
(`Ch` — a decoded scalar *or* one undecodable byte) and already carrying a test
suite that checks it against what glibc accepts and rejects. The move is a
`git mv` plus an import change.

The character model went with it, and that is the load-bearing part: `osh`'s
`bytes` module now **re-exports** `ere::ch` rather than defining its own `Ch`.
Had it kept a copy, the shell and the utilities would have had two definitions
of "what is one character" underneath one definition of "what does `.` match",
which is the drift this exists to prevent, reintroduced one level down.

### `posix`'s `regcomp`/`regexec` deliberately does *not* join

`posix/src/regex.rs` stays a separate implementation. It is `no_std` with
fixed-size buffers and a C ABI, and it answers to a different specification —
POSIX's, byte for byte, including which error code comes back for which
malformed pattern. Merging it would mean either giving `ere` a `no_std`
fixed-buffer mode that no Rust caller wants, or giving the C ABI an allocator it
must not need. Two implementations are correct here because they answer to two
specifications; the four that were merged answered to one.

*What this costs:* a C program and a Rust program on this machine could still
disagree about a pattern. That is a real residual risk and is accepted, because
the alternative degrades the component with the harder constraints. If it ever
bites, the fix is a shared *test corpus* run against both, not a shared
implementation.

### BRE by translation, not by a second parser

`grep`, `sed` and `expr` use Basic regular expressions unless told otherwise.
BRE is **not** a subset of ERE — `a+b` is three literal characters in one and a
repetition in the other, `\(x\)` groups in BRE and is literal in ERE, `*` is a
literal where nothing precedes it — so the difference has to be handled
somewhere.

*Option:* a second parser producing the same AST. *What changes:* two parsers
that must agree about everything except the handful of places they are supposed
to differ.
*Option, chosen:* `ere::bre::to_ere` rewrites the pattern and hands it to the
one engine. *What changes:* one parser, and the BRE/ERE difference becomes a
translation table a reader can check line by line.

Two consequences of the choice, both deliberate:

* **What the engine cannot express is refused by name, not mistranslated.**
  Backreferences (`\1`–`\9`) cannot be evaluated by a Pike VM at all — that is
  the price of the no-backtracking guarantee — and word boundaries (`\<`, `\>`,
  `\b`, `\B`) are not in the engine. Left alone, both would have fallen through
  the translator's escape rule and become *literals*: `\1` matching the digit
  `1`, silently. They are errors instead. A pattern that is refused is a bug
  report; a pattern that quietly matches the wrong thing is not.
* **A backslash inside a bracket expression is doubled on the way out.** POSIX
  says there are no escapes inside `[...]`, so a backslash there is a literal
  backslash; the engine does have escapes there. Without the doubling, the
  translation would change the meaning of a pattern that never asked for an
  escape.

### The cap that came with the move

`MAX_REPEAT` bounded a single `{m,n}` at 1000, and its comment claimed that
bounded compilation. It did not: intervals multiply under nesting, so
`((a{1000}){1000}){1000}` asks for ~10⁹ instructions — tens of gigabytes — from
24 bytes of pattern. `MAX_PROG` (65 536 instructions) now bounds the program
itself, and the expansion loops test it so a refusal costs the cap rather than
10⁹ iterations.

*Why it belongs in this entry:* the sharing is what made it urgent. Inside the
shell the pattern came from a script or a variable. `grep -f patterns.txt` and
`sed -f script.sed` read the pattern from a **file** — it is now as much
untrusted input as the subject is, in four more programs.

### The correction: the engine was leftmost-*first* too

Option C above was rejected partly because `regex` alternates leftmost-first
where POSIX requires leftmost-**longest** — and then the extraction found that
the engine being extracted did the same thing. A priority-ordered Pike VM stops
at the first thread to reach `Match`, so `a|ab` against `ab` answered `a`. That
is right for Perl and wrong for `grep`, and it was invisible: the shell's
`[[ =~ ]]` only asks *whether* a pattern matched, so nothing in osh's suite
could see which of two possible matches it had chosen. The four utilities do
ask — `grep -o` prints it, `sed` replaces it — so the extraction would have
shipped the defect into the programs where it is observable.

The search is now two passes over one program: an unanchored priority-ordered
pass to find the leftmost *start* (a thread that entered the pattern earlier
outranks one that skipped further first, so the first `Match` is leftmost), then
an anchored pass from that start which does not stop at `Match` and keeps the
last one — the longest end. The second pass is seeded at a single position, so
it ends as soon as its threads die; on the patterns anyone writes that is a few
characters.

*What it still does not do:* full POSIX **submatch** rules, which require
longest-first at every level of nesting rather than only for the whole match.
Group captures stay greedy-by-priority. GNU's engines do not implement the full
rules either, and none of the five callers asks; recorded here so the gap is a
known one rather than a surprise.
