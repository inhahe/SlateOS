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
**delete the entry from here**, and add one line to the `# Resolved` index at
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

## B-Q8 — [B] Two of the programs we copy disagree about how wide 626 characters are. Which one do we copy? — Status: OPEN

**In short:** Text on a terminal is laid out in fixed cells, and every program
that lines things up in columns has to agree on how many cells each character
takes — a Chinese character takes two, an accent mark that sits on the previous
letter takes none, most things take one. We keep one table of those numbers and
every one of our programs reads it. The trouble is that the two programs we
copy from — the shell **bash** and the **GNU command-line tools** — disagree
with each other about 626 characters, and we can only match one of them. Today
we match bash. Matching bash means our `ls` puts a filename in the wrong column
for those characters; matching the GNU tools means our shell's menus do.

**Why we cannot just measure what we draw (the operator's question, 2026-09-07,
and it turned out to be the good one).** The natural answer is "have the table
report how wide *we* actually print each character, and have every program ask
it" — and that is half-true already: our programs do all ask one table. Two
things stop it from settling the question, and the second is a genuine gap
nobody had written down:

1. **A program cannot ask the terminal.** There is no query for "how wide will
   you draw this?" The only way to find out is to print it and ask where the
   cursor ended up — a round-trip per character, over a link that may be a
   network, and impossible when the output is a file or a pipe, which is where
   `ls` and `wc -L` also decide their columns. So every implementation
   everywhere embeds a static table and hopes it matches the terminal.
2. **Our own terminal does not consult our table.** `userspace/charwidth` is
   depended on by exactly two crates — `userspace/coreutils` and
   `userspace/oils`. The GUI terminal that actually draws the glyphs (lane C)
   is not one of them. So "how wide we actually print" is currently decided by
   the renderer's font advance, independently of the table that every layout
   decision is made from. They have never been checked against each other.

### The question

Our table lives in `userspace/charwidth` and is the only such table in the
system — deliberately, because `ls`, `wc -L` (longest-line), `expand`, `fold`,
`nl`, `column` and the shell's `select` menu all draw onto the *same* screen,
so two of them disagreeing is not a difference of opinion, it is a crooked
screen. The table was built to match bash 5.2.37 and was checked against it at
1701 places, so today it is bash's answer.

On Linux, though, bash and the GNU tools do not get their numbers from the same
place. bash asks the C library (glibc). GNU coreutils 9.5 ships its own table
(from the "gnulib" support library) and **deliberately overrides the C
library's** in any UTF-8 setting — its own source comment says the system's
answer is not Unicode-aware enough. Coreutils 9.4 did not do this; 9.5 does.
Measured here, exhaustively over all 1.1 million characters, the two tables
disagree on **626 characters in 71 stretches**. Examples:

| Character | bash / glibc | GNU 9.5 / gnulib | Why they differ |
|---|---|---|---|
| U+00AD soft hyphen (an invisible "you may break the word here" mark) | 1 cell | 0 cells | A rule disagreement: gnulib gives *every* invisible formatting mark 0; glibc makes this one an exception |
| U+D7B0–U+D7FB (extra Korean vowel/consonant pieces that fuse onto the letter before them) | 1 cell | 0 cells | Same rule disagreement, applied to a newer Korean block |
| U+0600–U+0605 (Arabic marks printed *before* the number they belong to) | 0 cells | 1 cell | gnulib carves these out because they really do occupy a cell |
| U+1F203, U+1FA75, U+4DC0–U+4DFF, … | varies | varies | Different Unicode releases; and gnulib rounds *unassigned* characters inside East-Asian blocks up to 2 cells, we do not |

This is visible today: our `ls`-versus-GNU byte-diff harness has two cases that
differ for exactly this reason and no other.

### Options

**(a) Follow the GNU tools (gnulib's table).** Regenerate `charwidth` from the
reference implementation itself — we already have an exact dump of all 1.1
million answers, taken by calling GNU 9.5's own width routine.
*What changes:* our `ls` and `wc -L` line up with GNU's byte for byte on those
626 characters; our shell's `select` menu stops lining up with bash's on them.
- **Pro:** matches the six utilities that consult a width at all (`ls`, `wc -L`,
  and `sort`, `pr`, `df`, `numfmt` when we write them) against one shell.
- **Pro:** it is a *named, pinned* source — Unicode 15.1.0, one file, and we can
  re-dump it at will. Our present table came from whatever Unicode version the
  Python on the build machine happened to ship.
- **Con:** it breaks a passing test. `userspace/oils/tests/gen_display_width.py
  --diff-osh` compares our shell's menu against real bash at every table edge
  and currently agrees everywhere; it would start reporting 626 disagreements.
- **Con:** gnulib's table is *newer*, not *agreed*. Terminals have their own
  tables too, and nothing says gnulib's matches the terminal we will ship.

**(b) Keep bash's table (what we do today).**
*What changes:* nothing observable; our `ls` keeps putting those 626 characters
one cell off from GNU's.
- **Pro:** no change, and the one end-to-end byte-diff we have that involves a
  human-visible layout (shell menu vs bash) keeps passing.
- **Con:** every `ls` case containing one of those characters stays permanently
  marked "differs on purpose" in the harness, which dulls the harness.
- **Con:** we are copying the *older* of the two answers on the characters where
  they differ for a Unicode-version reason.

**(c) Two tables — the shell reads one, the utilities the other.**
*What changes:* both byte-diffs pass; the shell and `ls` can disagree by one
cell about the same filename on the same screen.
- **Pro:** maximum fidelity to both upstreams.
- **Con:** this is precisely the thing `charwidth` exists to prevent, and the
  symptom (a menu and a listing that do not line up) is the one a user actually
  sees. I do not recommend it.

**(d) Make the table describe our own renderer, and pin both to one source.**
Derive `charwidth` from the width the GUI terminal actually advances by, so the
table is a *description* of what we draw rather than a *prediction* of what
someone else draws; both upstream harnesses then show the 626 as deliberate
differences from *both* references.
*What changes:* our screens are internally correct by construction — the shell
menu and `ls` line up with each other and with the glyphs, which is the only
thing a user of SlateOS can actually see. Both byte-diff harnesses gain
permanent expected-difference lists.
- **Pro:** it is the only option whose correctness does not depend on a third
  party. bash and gnulib are both *guessing* at the terminal; we do not have to.
- **Pro:** it answers the question the other three cannot — which of the two is
  right *here* — because on SlateOS neither is authoritative.
- **Con:** it is cross-lane. The renderer is lane C's; the table is lane B's.
  It needs an agreed interface (a shared width source, or a generated table
  checked by a gate on both sides) rather than one lane editing the other.
- **Con:** it gives up byte-fidelity to *both* upstreams on those 626, so
  neither harness can be read as pass/fail on them again.
- **Unknown until measured:** whether the renderer and the table agree today.
  Nobody has compared them; the answer decides whether (d) is a change or
  merely a written-down invariant.

### If never answered

Safe, and it does not get worse on its own. Today's behaviour is (b). The cost
is confined to two permanently-deferred cases in the `ls` harness and to the
626 characters themselves, which are mostly invisible marks and unassigned
code points — nobody has a filename made of them by accident.

### Claude's recommendation

**(a)**, but not strongly enough to do it without you: the deciding fact for me
is that the count is six utilities to one shell and that gnulib's table is a
pinned upstream we can re-derive mechanically, whereas ours is not. What stops
me from just doing it is that it silently changes the on-screen layout of the
shell and five other programs to win a byte-diff in one — a user-visible
behaviour change, which is yours. Meanwhile I have kept (b) and isolated the
divergence in the harness (fixture `y/`) so it costs two cases and not twenty.

### Where it bites

`userspace/charwidth/src/lib.rs` (`ZERO_WIDTH`, `WIDE`, and the doc comment
that says the tables were measured against bash);
`userspace/oils/tests/gen_display_width.py` (the generator and its `--check` /
`--diff-osh` measurement against bash); `userspace/oils/src/width.rs`;
`userspace/coreutils/src/bin/ls.rs` and `wc.rs`; `userspace/column/src/main.rs`;
`scripts/ls-diff.sh` (fixture `y/`, two `!` cases);
`known-issues.md` → `TD-B-OUR-WIDTH-TABLE-IS-BASHS-AND-COREUTILS-9.5S-IS-NOT`.

---

## C-Q9 — [C] The backup tool and the search tools read the same-looking patterns by different rules. Should they be made the same? — Status: OPEN

**In short:** when you tell the backup program which folders to skip, you type a
pattern like `*.tmp` or `build/**`. When you search for a file, you type a
pattern that looks the same. They are not the same: the search tools understand
`[a-z]` to mean "any lowercase letter", and the backup tool understands it to
mean "a folder whose name literally contains a square bracket". Both behaviours
are defensible. The question is whether to make the backup tool agree with the
search tools — which would change what the exclude lists people have *already
written* actually exclude.

**Where it bites:** `apps/backup/src/main.rs` (`glob_matches`,
`glob_match_recursive`, `glob_match_simple` — lines 40–215) versus
`apps/globmatch/src/lib.rs`, which `apps/indexer` and `apps/filesearch` now
share. The reasoning behind the split is `design-decisions.md` §555.

### What is actually different

The desktop used to contain four separate pattern matchers. The two that are the
same feature seen twice — the search window and the background indexer — have
been merged into one shared implementation, because they disagreed with each
other on 646 of 730,236 test patterns and no test either of them had could ever
have noticed. That part needed no decision; it was a bug.

The backup tool is the case that does need one. It is not a broken copy of the
search matcher; it is a different pattern language, the one `.gitignore` uses:

| | search tools (`apps/globmatch`) | backup (`apps/backup`) |
|---|---|---|
| `*` | matches any run of characters, including `/` | stops at a `/` |
| `**` | nothing special — two stars, same as one | spans folder boundaries |
| `?` | any one character | any one character except `/` |
| `[a-z]` | any lowercase letter | the six characters `[`, `a`, `-`, `z`, `]` |
| works on | text, character by character | raw path bytes |

The first three rows are *correct as they stand*: an exclude list matches paths,
so `*.tmp` should not match `logs/a.tmp`, and `build/**` should. Nobody is
proposing to change those. The only row in dispute is the fourth.

### The options

**A — Leave it. Two dialects, written down.** *What changes:* nothing. Backup
patterns keep treating `[` as an ordinary character; search patterns keep
treating it as the start of a character class. Someone who learns one and
assumes the other is surprised once.

**B — Teach the backup tool character classes.** *What changes:* an existing
exclude line like `cache[1]/` stops excluding the folder literally named
`cache[1]` and starts excluding a folder named `cache1`. Everything else keeps
working. Gains: one rule to learn instead of two, and `*.[ch]` becomes a way to
skip C sources. Costs: a silent change of meaning in data users already wrote —
the backup that quietly starts including a folder it used to skip is not an error
anyone will see.

**C — Teach the backup tool character classes, but only for new patterns.**
*What changes:* the same as B for anything written from now on; existing exclude
files are read under the old rules until edited. *What it costs:* a file format
that means two different things depending on its age, which is the kind of thing
that is impossible to explain and impossible to remove later. Listed for
completeness; I do not recommend it.

### Your two questions, answered — 2026-09-07

You asked (A) what is normal, and (B) how likely a user is to really benefit.

**(A) What is normal: character classes are standard in *both* families.** This
is the answer I got wrong the first time round, and it reverses my
recommendation, so it is worth being blunt about.

| tool | family | `[a-z]` means |
|---|---|---|
| `.gitignore` | the exclude-list family | a character class |
| `rsync --exclude` | same | a character class |
| `tar --exclude` | same | a character class |
| POSIX `fnmatch`, every shell glob | the search family | a character class |
| **our `apps/backup`** | exclude-list | **five literal characters** |

My original recommendation said backup's dialect "matches what every developer
already knows from `.gitignore`, where `[` is likewise not special in the
common case". **That is simply false.** Git matches with `fnmatch(3)` and
`FNM_PATHNAME`, and `[a-z]` is a class there exactly as it is in a shell. I
asserted it without checking, and the whole case for option A rested on it.

So "no character classes" is normal for **neither** family. It is not a
deliberate dialect we chose for good reasons; it is a feature the backup
matcher never grew. The first three rows of the difference table *are* a real
dialect and are right as they stand — `*` stopping at `/`, `**` spanning
directories, matching on raw bytes. The fourth row is not a dialect; it is a
gap.

**This also inverts the direction you were leaning.** Removing classes from
search and indexing to match backup would make SlateOS the only system a user
has ever met in which `[a-z]` in a pattern means five literal characters —
including different from its own shell. It would mean deleting a working,
carefully-tested feature (`apps/globmatch` handles the awkward POSIX corners:
`[]]` as the one-element class containing `]`, a trailing `-` as a literal)
in order to match the one tool that is the outlier.

**(B) Would a user really benefit: rarely — but that is the wrong axis.**
Direct benefit is small and I will not overstate it. Real exclude lists are
overwhelmingly `*.tmp`, `node_modules/`, `build/`, `.cache/`. A pattern with a
bracket in it is uncommon.

The asymmetry is in what happens when one *does* appear, because **both
behaviours are silent**:

- A user pastes a `.gitignore` into the backup exclude list — far and away the
  most likely way a `[` arrives, since that is where such lists come from.
- `[Tt]humbs.db` then excludes a file literally named `[Tt]humbs.db`, which
  does not exist, so it excludes **nothing**.
- The backup silently contains files the user believed they had excluded. No
  error, no warning, and nothing they would ever think to check.

So the question is not "how often is a class useful" but "what does it cost
when the two languages differ" — and that cost is a wrong backup that looks
right.

**Your remark that decides it.** You said you have no rules using `[]`. Option
B's only real cost was the one in its own Cost line: *"a silent change of
meaning in data users already wrote"*. If that data does not exist, B costs
nothing and the objection is gone. (One clarification in case it changes your
answer: this question is about **SlateOS's own `apps/backup`**, not your
backup program on `D:` — so the installed base is whatever exclude lists exist
inside this OS, which is currently none.)

### My recommendation, revised: **B**

Teach `apps/backup` character classes, by calling `apps/globmatch`'s existing
class parser for the segment-matching step. It is one matcher changed rather
than two, it moves toward both traditions instead of away from both, it turns
a silent-wrong-answer case into a correct one, and the migration cost that was
the sole argument against it does not apply.

If you would rather not add the feature, the fallback is **not** option A but
"make backup **reject** a pattern containing an unescaped `[`" — that keeps
the languages apart while making the disagreement loud instead of silent,
which is the only genuinely bad property of the current state.

### If this is never answered

Nothing is blocked and nothing degrades. The two dialects are documented in
`design-decisions.md` §555 and in `apps/globmatch`'s module docs, so the split is
a recorded decision rather than an accident. The only ongoing cost is that a user
who learns one pattern language may assume the other works the same way.

## C-Q11: Should something build every crate before a merge? (raised by lane C, 2026-09-06)

**In short:** The lock screen — the program that asks for your password when
the machine is locked — was broken for a day and nobody knew, because nothing
in this project ever tries to build it. Somebody changed a shared library, the
lock screen still referred to the old version, and no test anywhere failed. It
was found by accident. There are 142 more programs in the same position
(143 counted, one broken). The question is whether to add a slow check that compiles everything
before work is merged, and if so, what shape it takes — because whatever we
pick, all three lanes have to live with it.

### What happened

`5264cba7a` (lane B) removed a function argument and a module from `authlib`,
a shared login library. Its commit message says "no caller changes", which was
true of every caller *that lane can see*. `apps/lockscreen` is a caller in lane
C's tree. It stopped compiling and stayed that way until an unrelated tidy-up
happened to run clippy on it.

Nothing caught it because nothing builds `apps/`:

| What runs today | Why it misses this |
|---|---|
| the boot test | builds for the bare-metal target; `apps/*` are not in `default-members` there |
| each lane's own `cargo test -p …` | a lane only builds what it touched |
| `check-window-wiring.py`, `check-gates-are-wired.py` | read source text; they never invoke the compiler |
| CI | there is none |

A `cargo check --workspace --target x86_64-pc-windows-gnu` would have caught it.
Nobody has a reason to run one.

### A second instance, 2026-09-09 — and this one was not cross-lane

The case above is a lane boundary problem: lane B could not see lane C's caller.
This one has no lane in it at all.

`design-decisions.md` §826 changed four colour constants in `gui/appearance`.
Lane C ran `cargo test -p settings`, which passed, and merged. The change also
broke `a11y::tests::no_high_contrast_colour_is_a_palette_role` in `gui/desktop` —
pure black had become the light theme's text colour, and that test asserts no
high-contrast colour is also a palette role. It sat red on `main` until it was
found by accident a day later, while doing something else.

So the gap is not only "a lane cannot see another lane's callers". It is
**anyone editing a crate that four others depend on and testing only the crate
they edited**, which is the ordinary way to work and is what the per-crate
instruction in `CLAUDE.md` asks for. A palette is exactly the shape of thing
that has many dependents and no obvious blast radius.

**One practical finding that changes the cost estimate below.**
`cargo check --workspace --target x86_64-pc-windows-gnu` **exits 0** on the
current tree and takes minutes on a cold cache, seconds warm. The `build`
spelling does *not* — it fails trying to link the kernel for the host target,
which is its own trap (`known-issues.md`
`TD-C-CARGO-BUILD-WORKSPACE-ON-THE-HOST-TARGET-FAILS-ON-THE-KERNEL`). If the
answer here is yes, the gate should be spelled `check`, not `build`.

Whether the gate should also run `cargo test --workspace` is a separate and much
more expensive question: `check` would **not** have caught this second instance,
because a broken test compiles fine. It would have caught the first.

### Why this is yours and not mine

Any answer gates all three lanes' merges, and the cost lands on whoever is
merging — not on me proposing it. It is also genuinely slow: a cold check of
the whole workspace is minutes, and it is minutes *added to every merge*.

### Options

**A. A pre-merge `cargo check --workspace` for the host target.**
*What changes:* merging to `main` takes a few minutes longer, every time, and a
lane cannot merge while any crate in the tree is broken — including one broken
by a different lane.
The strongest guarantee, and the harshest: lane A is blocked by lane C's typo.
That is already true of the boot test, which builds the whole workspace, so
this is a difference of degree.

**B. A nightly (or once-per-session) sweep that only reports.**
*What changes:* nothing blocks; a broken crate is found within a day instead of
within a chance encounter, and lands as a `known-issues.md` entry or a
`requests/` file for whoever owns it.
Cheap and unintrusive. Does not stop a breakage being merged, only shortens how
long it lives.

**C. Require the grep before the claim.**
*What changes:* nothing mechanical; the convention becomes that a commit
message may not say "no caller changes" about a shared library until
`grep -rl <symbol> apps/ gui/ net/` has been run.
Costs nothing and would have caught this exact case, but it is a rule enforced
by remembering it, which is the kind that decays.

**D. Do nothing.**
*What changes:* nothing. Broken app crates accumulate silently and are found
one at a time by whoever next touches them.

### Measured — then measured wrong, twice. Read this before the recommendation

I first recommended the cheap option (B) on the assumption that a full check
costs "minutes added to every merge". I then measured, got 58 seconds, and
reversed to the gate (A). **Both of my numbers were about the wrong thing, and
two other lanes found the holes.** The corrected position is below; the history
is kept because the *shape* of the error matters more than the number.

**What I actually measured.** `cargo check` over the 143 crates in `apps/`,
enumerated by name and passed as `-p` flags, on an otherwise idle machine:
58 seconds warm, exactly one broken (the lockscreen, since repaired).

**Hole 1 — that is not the command the gate would run.** The gate's mechanism
is `cargo check --workspace` for the host target. On a tree lacking
`services/hello/target/.../hello`, that does not merely take longer — it *fails
outright*, because `kernel/src/container.rs` embeds that artifact and it is not
built by anything cargo runs. My `-p apps/*` enumeration skipped the kernel and
so never met it. So "58 seconds, all clean" is a number about a command nobody
would run, and it is the reassuring half. (Found by lane B. Details in
`known-issues.md` →
`A-THE-KERNEL-EMBEDS-A-BUILD-ARTIFACT-NOTHING-BUILDS-AND-NOTHING-TRACKS`;
lane A is fixing the build-graph edge, so this is a prerequisite for the gate,
not a permanent obstacle.)

**Hole 2 — an idle number is the wrong number for a shared machine.** This
machine saturates on a *single* cargo run; that is measured, not folklore. So
the cost of a gate that fires on every merge is not its own wall clock — it is
its wall clock *under contention*, **plus the degradation it imposes on the
other two lanes for the duration**. For a gate that runs many times a day
across three lanes, that second term may well dominate the first.

That distinction is the same one lane A's case for the SSD migration turned on
(18 random reads/sec contended against 100–150 idle — a 5–8× gap that becomes
~40× under three-lane load). I read that argument, agreed with it, and then
failed to apply it to the thing I was proposing. An idle measurement is the
optimistic half of any question about a machine three agents share.

### The measurement, taken properly (lane B, 2026-09-06)

`cargo check --workspace` for the host target, on `E:/os-lane-b` at
`b9b7c61df`, machine **idle**:

| | |
|---|---|
| **139 s** | full check from cold — the kernel had never been checked in that tree |
| **15 s** | immediate re-run, nothing changed — the no-op cost |
| **14 s** | after touching one source file — **the number this entry should quote** |

The gate's cost is the third row: a merge is warm, because the lane just built
and tested the thing it is merging, and something has changed. **Fourteen
seconds.**

Lane B stated the cold/warm spread explicitly — 139 → 15 — for the reason that
my 58 s was misread for want of exactly that context. Quoting 14 s as a
from-cold figure would be off by two minutes.

**Still to come:** the same measurement under contention, which is the one the
recommendation should turn on (see Hole 2). Lane B is taking it during lane A's
boot test.

### Scoped versus whole, measured — and my prediction was wrong

The comparison above (39 s scoped against 14 s whole) was invalid: mine was
**cold** for ~156 crates, lane B's was **warm incremental**. Lane B then took
the missing measurement, both sides warm-incremental and **both under identical
contention** (during lane A's boot test):

| | one app file touched | no-op |
|---|---|---|
| **scoped** (158 `-p` flags) | **8 s** | 7 s |
| **whole** (`--workspace`) | **15 s** | 16 s |

I predicted the enumerated form would lose, because naming 158 packages makes
cargo do work proportional to the set *named* rather than the set that
*changed*. It wins, about 2:1. Recorded because this entry has a running theme
and I am not exempt from it.

**Why, and it is worth knowing independently of the gate.** Neither lane B nor I
had checked what `--workspace` actually enumerates before reasoning about it.
Both of us pictured "the 158 apps, plus the kernel". Counted from the manifest
globs:

| glob | crates |
|---|---|
| `apps/*` | 143 |
| `gui/*` | 15 |
| `init/*`, `net/*` | 4 |
| **`userspace/*`** | **2,759** |

So `--workspace` is roughly **2,900 members**, not 160. Scoped checks 158
things and whole checks about 2,900; the subset being cheaper is not a
surprise once the number is in front of you. It was in front of neither of us.

### What that means: cost is not the axis, coverage is

At 8 s against 15 s, **both under contention**, cost cannot decide this. Seven
seconds is inside the noise of a merge. So the scope should be chosen for what
it *covers*, and there the two differ sharply (lane B's argument, and I think it
is right):

- A gate scoped to `apps/` + `gui/` catches a breakage whose **victim** lives in
  those trees. It would have caught my `guitk::Event` one.
- It would **not** have caught the `authlib` → `init/loginmgr` one, because
  `init/` is outside the scope — and that is the same commit that started this
  entry.

Victims can be anywhere, so only whole-workspace covers cross-lane API changes,
which is the case C-Q11 exists for. **Do not scope for cost.** If a scope is
ever wanted, it must be justified by coverage, and this one cannot be.

That also disposes of the `include_bytes!` prerequisite as a scoping argument:
it is a real bug and worth fixing on its own merits, but it is not a reason to
reduce the gate's coverage, and 15 s says the coverage need not be traded for
cost.

### The scenario that matters, now measured: worst case 49 seconds

Every figure above is **"one file touched"**. That is not when a gate fires.

`CLAUDE.md` step 1 requires `git fetch origin && git merge origin/main` before a
lane starts work, and the merge-up happens at the end. So the gate runs on a
tree that has just absorbed *another lane's* commits — which on a bad day means
a shared crate (`guitk`, `guiremote`, `authlib`) changed, and everything
downstream of it rebuilds. Downstream of `guitk` is 158 crates; downstream of
`authlib` is most of `userspace/`.

Lane B measured it — touch each shared crate, re-check the whole workspace,
under contention:

| crate touched | dependents | whole-workspace re-check |
|---|---|---|
| `authlib` | 12 | 19 s |
| `posix` | 13 direct | 22 s |
| `guitk` | 144 | 26 s |
| **`quoting`** | **773** | **49 s** |

**Worst case in the tree is 49 seconds.** It never approaches the 139 s cold
figure because `cargo check` does no codegen: a cold run pays to *compile* the
dependency graph, a post-merge re-check only pays to re-read it — about 63 ms
per downstream crate.

**Two things I had wrong here, both the failure this entry keeps cataloguing.**

*First:* I wrote that "downstream of `authlib` is most of those 2,759". It is
**12**. That is wrong by two orders of magnitude and it mattered, because the
whole expensive-case worry rested on the intuition that a shared-crate change
rebuilds most of the tree. It does not. **The blast radius that justified this
entire question was twelve crates.** What made it serious was not that there
were many victims but that they were in a *different lane* from the author —
which is the thing a gate fixes and a bigger number would not have made truer.

*Second:* the widest shared crate is not `guitk`, `posix` or `authlib`. It is
**`quoting`, at 773 dependents**, and it appeared in none of the three candidate
lists either of us was reasoning from. Lane B measured it precisely so the worst
case would not be merely the worst of the ones we happened to name. Same shape
as the six embedded artifacts and the `--workspace` member count: enumerate
first, then reason.

Dependent counts verified independently here (`grep -rl` over every
`Cargo.toml`): `quoting` 773, `guitk` 144, `posix` 13, `authlib` 12. Member
counts: `userspace/*` 2,759, `apps/*` 143, `services/*` 76, `gui/*` 15,
`init/*` 2, `net/*` 2.

### Recommendation

**Adopt C now, regardless. Defer the A-versus-B choice to one number that does
not exist yet.**

**Adopt C as well — it is free — but it is not the answer and should not be
credited as one.** The convention: a commit message may not claim "no caller changes" about a
shared library until `grep -rl <symbol> apps/ gui/ net/` has been run. It costs
nothing and needs no infrastructure.

What I originally wrote here was that it "would have caught this". That is true
of the first two breakages and **not** of the third, which I caused myself a few
hours after writing this entry — see the third row below. I added a variant to a
shared enum in `guitk`, updated the three consumers I was thinking about, and
did not grep for the rest. At that moment I had the failure mode more firmly in
mind than anyone in this project has ever had it: I had just written up two
other lanes' instances of it, in this file, arguing for a gate to catch it.

So the honest assessment of C is that it is a *discipline*, and this project now
has one clean experiment on whether discipline is sufficient here. It is free,
it costs nothing to keep, and it will catch the cases where the author pauses to
think. It will not catch the cases where the author is confident — which are the
same cases, because confidence is what stops you grepping.

### The four breakages, all one shape

| # | Change | Consumer missed | Found by | Author's state |
|---|---|---|---|---|
| 1 | `authlib` drops `with_stores`'s second argument (§353) | `init/loginmgr` | lane B, later | believed the caller list complete; had grepped `userspace/*/Cargo.toml`, which covers neither `apps/` nor `init/` |
| 2 | the same change | `apps/lockscreen` | lane C, by accident, a day later | same commit, same belief — its message says "no caller changes" |
| 3 | `guitk::Event` gains `SettingsChanged` | `apps/stickynotes`, `apps/explorer` | lane A's boot test, 30 min in | lane C — me — hours after writing this entry |
| 4 | the same variant, unmerged in lane B's tree | the same two crates | lane B's own C-Q11 measurement run, rc=101 | lane B, *while measuring the cost of the gate that catches it* |

A type or a signature changes, some consumers are updated, others are not, and
nothing notices until a boot test half an hour in or a person happens to look.
**Four times in one day, by all three lanes, in three different subsystems** --
and the last two by the two people who at that moment were most alive to the
risk.

**Numbers 3 and 4 are the same failure by the two people least able to make
it, and together they say more than any timing here.** Number 3 was the author
of this proposal committing the failure hours after writing the argument for a
gate. Number 4 was the person pricing that gate committing it during the
measurement. Neither of us failed for want of knowing — we had the failure mode
in mind more firmly than anyone in this project ever has.

The mechanism that refutes is *priming*, not memory. The grep in option C is a
step you take when you doubt yourself, and neither of us doubted. A convention
that fires on doubt cannot cover the confident case, and the confident case is
most of them. That is why C is kept below as free-and-worth-having rather than
as a control anyone should rely on.

**Number 3 carries one extra piece of evidence the others do not**, and it bears
on what a gate is worth. The two crates that broke were the two matching their
events *exhaustively* — `explorer` names every event it declines, `stickynotes`
matches every variant. Roughly 140 other apps end with a catch-all arm and
accepted the new variant in silence. So adding to a shared enum punishes exactly
the consumers that opted into being told, and rewards the ones that opted out.
Compiler exhaustiveness is the closest thing to a free gate this codebase has,
and it only fires where someone chose to leave it armed.

**The dangerous property is discarding, not catching** (lane B and lane A, and
this corrects my first statement of it). A catch-all that *forwards* the value —
`Err(e) => report(e)` — still surfaces a new variant as its own text, without
the crate being recompiled against the new definition. It is
`_ => {}` that swallows it. So "140 crates end in a wildcard" is the wrong
count and would condemn arms that are fine; the count that matters is
catch-alls that *drop* the value, which is not a thing grep can tell you.

That has a direct consequence for what a gate is worth: **a compiling workspace
is a floor on correctness, never a proof.** A check cannot distinguish a
forwarding catch-all from a discarding one without reading it, so the gate
guarantees only that every crate still builds — which is exactly the guarantee
being priced here, and worth not overselling.

**Between A and B, here is the decision rule rather than a verdict**, so that
the answer follows from lane B's measurement instead of from my instinct:

| If the contended workspace check costs… | Then |
|---|---|
| under ~1 minute | **A** — a fair price for "the tree compiles", and the only option that *stops* a breakage rather than shortening its life |
| a few minutes, with the other lanes degraded throughout | **B** — the nightly sweep. The guarantee is no longer cheap, and a standing tax on every merge stops being worth it |

**Every figure is now in**, and they all land in the first row:

| scenario | cost |
|---|---|
| no-op / one file touched, idle or contended | 14–16 s |
| after a merge touching `authlib`, `posix` or `guitk` | 19–26 s |
| **after a merge touching the widest crate in the tree (`quoting`, 773 dependents)** | **49 s** |

Nothing costs a minute, including the worst case, including under contention.

**I recommend A, whole-workspace, and I have dropped the hedge.** I said I
would drop it if the worst realistic case came in under a minute; it is 49
seconds. Everything that was uncertain when this entry was written has since
been measured, and every measurement moved toward A:

- the price is seconds, not minutes, at every point in the range;
- contention costs almost nothing (14 s idle against 15 s contended);
- the failure went from "one, found by accident" to **four in one day, by all
  three lanes**;
- and the two most recent were committed by the person who wrote the argument
  for a gate and the person measuring its cost — which is what convinced me the
  convention in C cannot be the answer.

The standing caveat is unchanged and the operator should still apply it: **I
have been wrong on this entry's numbers repeatedly** — first guessing minutes,
then measuring the wrong command, then quoting an idle figure for a shared
machine, then predicting the scoped/whole comparison backwards, then putting
`authlib`'s blast radius at ~2,759 when it is 12. Every one of those was
corrected by another lane rather than by me. What that argues, though, is not
that the recommendation is unreliable — it is the single best argument *for* the
recommendation. Five wrong numbers from someone paying close attention, caught
only because two other agents happened to check, is precisely the case for a
mechanism that fires without being invoked.

**The objection to A, restated in the better form lane B gave it.** I had
written it as "the gate lets one lane block another's merge", and answered that
this is already true of the boot test. Lane B's version is stronger: *the
coupling exists whether or not there is a gate.* Today it ran in both
directions — lane B's `authlib` change broke lane C's lockscreen, and that
lockscreen then stood between lane B's own `init/loginmgr` fix and a green `main`.
The gate does not create that coupling. It moves discovery from "another lane
trips over it days later" to "the lane that caused it, at the moment it caused
it". The cost lands on whoever merges next *without* the gate; with it, it lands
on whoever broke it.

**One condition on A if it is chosen.** Scope it to the members that genuinely
check cleanly on the host, and scope it for *that stated reason* — never to
route around a member that is broken. A gate excluding the kernel because the
kernel does not build is a gate that has hidden the defect it should have
reported. (Lane B's phrasing; it is the right test for whether a scoped gate is
honest.)

### If this is never answered

The current state is safe but degrading, and nothing is blocked: the lockscreen
is repaired, `main` is green, and adopting C costs nothing and needs no answer
from you. Everything this question needed measuring is measured; it is waiting
only on you.

What stays open is the gap. Every shared-library change is another chance for a
breakage that nothing reports and that is found weeks later by somebody who did
not cause it. **On the day this was raised it happened four
times** — twice from `authlib`'s single-store change (`apps/lockscreen` and
`init/loginmgr`, two lanes, neither caught by anything but a person looking), and
once from my own `guitk::Event` addition, which a boot test caught thirty
minutes in, and once again from that same variant in lane B's tree, found by
the run measuring what a gate would cost. All four are fixed; the mechanism
that let them through is untouched.

The cost grows with the number of app crates, which is growing.

## A-Q8 — [A]+[C] Desktop icon layout exists in two places: `fs::deskicons` (kernel) and `gui/desktop/src/icons.rs` (shell). Which is the authority? — Status: OPEN

**In short:** A user's desktop icons have positions on screen. Two independent
modules model that layout: `kernel/src/fs/deskicons.rs` (Lane A, marked done in
the roadmap, exports via `/proc/deskicons`) and `gui/desktop/src/icons.rs`
(Lane C, never wired, a pinned island). Lane C cannot wire its module without
duplicating the kernel's state, and cannot delete it because it is marked done
and in Lane A's tree. Nothing is broken — the `icon_size` appearance setting is
inert, exactly as it has always been.

**Options:**

- **(A) The kernel one is the model; the shell consumes it.** `icons.rs` is a
  duplicate to delete. The shell reads icon positions from `/proc/deskicons`.
  *What changes:* the shell becomes a renderer for state the kernel owns.
- **(B) Layout belongs to the shell; the kernel one is persistence only.** Wire
  `gui/desktop/src/icons.rs` as the layout authority. `fs::deskicons` is either
  demoted to a read-only persistence layer or marked as tech debt to remove.
  *What changes:* icon layout moves to userspace where the microkernel rule says
  it belongs; `icon_size` becomes a live setting.
- **(C) `fs::deskicons` predates the microkernel split and should not be in the
  kernel at all.** Delete it, wire the shell module, persist positions in a
  dotfile or YAML. *What changes:* one fewer kernel module, one fewer `/proc`
  entry, icon state lives in userspace end to end.

**Claude's recommendation:** B or C. The microkernel rule is unambiguous — icon
layout is a userspace concern, not a scheduler/MM/IPC/cap/interrupt concern.
B is the minimum viable move; C is the clean one.

**If never answered:** the `icon_size` setting stays inert, Lane C does not wire
`icons.rs`, and both modules continue to exist without either being used. No
degradation, but the duplicate grows harder to resolve over time as either side
accumulates callers.

**Filed by:** Lane A (2026-09-07), prompted by Lane C's
`c-a-two-desktop-icon-models-and-mine-cannot-be-wired-until-we-pick.md`.
Response at
`a-c-deskicons-is-a-persistence-layer-the-shell-is-the-layout-authority.md`.

---

## A-Q9 — [A] Networking now exists twice: inside the kernel, and as an ordinary program. The second one is finished and switched off. Should it become the default? — Status: OPEN (raised 2026-09-09)

**In short:** this system can do its networking two ways. The way it uses today
runs inside the kernel — the innermost, most privileged part of the system,
where a bug can take down or take over the whole machine. The other way runs it
as an ordinary background program, so a bug there can only break networking.
The second way is **built, working, and tested**, but it is switched off unless
you ask for it. The question is whether to make it the one everybody gets. It is
not a small flip: the two are not equally mature, and the new one has one known
rough edge.

**Why this is the operator's call and not mine.** The project's own design rule
says networking belongs outside the kernel, so on principle the answer is yes.
But the version inside the kernel has had months of features and fixes poured
into it, and the replacement has not. Choosing the architecturally-correct
option over the better-tested one is a judgement about what this system is for
right now — a decision about risk appetite, not about code.

**Glossary.** *Kernel* — the core of the OS; code there can do anything, so a
fault is fatal or exploitable. *Daemon* — an ordinary program running in the
background with no special powers. *Socket* — the handle a program uses to talk
over the network. *Default* — what you get without setting anything.

### Where things stand

| | in the kernel (today's default) | the daemon (built, off by default) |
|---|---|---|
| matches the design rule | **no** | **yes** |
| a bug there can crash the machine | **yes** | no — only networking stops |
| maturity | months of work: congestion control, retransmission, out-of-order reassembly, fragmentation, keepalives | newer; feature parity reached, less mileage |
| tested | every boot | every boot, *when switched on* |

Everything a program needs has been brought across and each piece is checked on
every boot with the switch on: connecting out, listening for incoming
connections, IPv4 and IPv6, TCP and UDP, non-blocking mode, and name lookup.
Real programs have driven it end to end — small stock-Linux test programs fetch
a web page and resolve a name through it and exit 0.

**The one known rough edge.** A *server* — a program accepting several incoming
connections at once — currently serves them strictly one at a time through a
single shared channel to the daemon. So one slow or stalled client can hold up
the others. Clients (a browser, a package download) are unaffected; this only
bites a program accepting connections. It is a known interim design, recorded as
`D-NETSOCK-SYNC`, and removing it needs an asynchronous rewrite of that path,
not a patch.

### The options

**A. Flip the default to the daemon now.**
*What changes:* every program's networking goes through the background program
instead of the kernel. A networking bug stops networking instead of stopping the
machine. A server handling several connections at once gets slower under load
until the rough edge above is fixed.

**B. Leave the kernel one as the default; keep the daemon opt-in.**
*What changes:* nothing visible. The daemon stays exercised only on boots that
ask for it, which means it drifts from reality at whatever rate the two diverge.

**C. Flip the default, but fix the server rough edge first.**
*What changes:* nothing visible for now; later, the same as A but without the
slowdown. Costs an asynchronous rewrite of the server path before anything
changes.

**D. Flip the default and delete the in-kernel one.**
*What changes:* the same as A, plus there is no way back without reverting the
deletion. Removes ~40 files from the kernel and makes the design rule true
rather than merely available.

**My recommendation: C, then D.** A is the right destination and B is how a
finished migration quietly rots, but flipping while a server can serialise its
own clients would trade a correctness win for a visible performance regression —
and the first person to hit it would reasonably read it as "the new networking
is slow" rather than "one known interim edge". C removes that objection before
anyone can form it. D should follow C rather than accompany it, because keeping
the old path for one release is what makes C reversible if something unmeasured
turns up.

**If this is never answered:** nothing breaks, and that is the trap. The kernel
keeps serving the network and the daemon keeps passing its tests on the boots
that enable it — so the cost is invisible and compounding: two networking stacks
to keep working, a design rule the project states but does not follow, and a
finished migration whose value is not being collected. The work is done. Only
the decision is missing.

**Where it bites:** `kernel/src/net/` (the resident stack, ~40 files),
`kernel/src/net/netstack_client.rs` (the kernel's client for the daemon),
`kernel/src/net/socket.rs` (the socket objects), `services/netstack/`
(the daemon), and the `net.userspace` boot switch read by
`fs::kernparam::is_set`. Roadmap: "TCP/IP stack" → Phase 5, increment 5.7.
Prior decisions: `design-decisions.md` §63 (Path B chosen), §66 (staged
cutover), §71 (Q23, shared session for server sockets).

---

## C-Q12 — [C] There are two system trays, and neither can do what the spec asks. Which one is the real one? — Status: OPEN

**In short:** the little row of icons at the right-hand end of the taskbar —
the clock, the volume and network icons, the icons programs put there when they
tuck themselves away — exists twice in this codebase, built two different ways,
and the two halves cannot see each other. One of them draws the clock but has
no way to hold a program's icon. The other holds program icons but is a
separate program of its own. `design.txt` asks that you be able to **drag icons
into and out of the tray**, and the code for that drag-and-drop is written —
1,184 lines of it — but it lives with the half that has no icons to drag. So
the feature cannot be finished without first deciding which half is the real
tray. Nothing is broken today; the drag-and-drop simply does nothing, because
nothing constructs it.

**The two halves.**

| | `gui/desktop` (the shell's taskbar) | `apps/systray` |
|---|---|---|
| What it is | Part of the desktop shell, drawn into the taskbar the shell already owns | A standalone program, 3,715 lines, with its own window |
| What it draws | Clock, notification bell, virtual-desktop indicator | Tray icons with badges and tooltips, quick-settings flyout, volume popup, network popup, calendar popup |
| Icons a program can add | **None.** There is no list of them anywhere in the shell | Yes — `TrayIconId`, add/remove/show/hide |
| Drag-and-drop | `tray_dnd.rs`, fully written, **constructed by nothing** | None |
| Reached by anything today | Yes, the shell runs it | **No.** Nothing launches it |

**What the spec says.** `design.txt` line 710 lists the tray's contents as
"optional icons on taskbar like Windows: clock, wifi, ... volume", and 714–716
add "a system tray like on Windows / can drag and drop icons into and out of
the system tray / apps have the option of starting in system tray or minimizing
to system tray". It reads as one thing, in the taskbar. It does not say whether
the program that *draws* it must be the shell.

**The options.**

**A — the tray belongs to the shell; fold `apps/systray` into it.**
*What changes:* the taskbar grows a real icon list and the popups that go with
it; `apps/systray` stops existing as a program.
Pros: one taskbar drawn by one program, so the icons and the clock cannot
disagree about where the tray starts or how wide it is; `tray_dnd.rs` is then
in the right place and can be wired as written; dragging an icon *out of* the
tray and onto the taskbar is a move within one program rather than a protocol.
Cons: the largest of the three — 3,715 lines to merge into a shell that is
already the biggest thing in `gui/`; a crash in a tray popup takes the taskbar
with it.

**B — the tray is its own program; move `tray_dnd.rs` to it.**
*What changes:* `apps/systray` gets launched and given a strip of the taskbar
to draw into; the shell reserves the space and stays out of it.
Pros: smallest change to what already exists, and the two halves are already
split this way; a misbehaving tray icon cannot take the taskbar down; matches
the microkernel instinct of the rest of the project.
Cons: needs a protocol the shell does not have — the shell must tell the tray
how much room it has and where, and the tray must tell the shell when it wants
more, on every clock tick that changes the clock's width. Dragging an icon from
the tray to the taskbar crosses a process boundary. Two programs must agree on
the theme, the scale factor and the autohide animation, all of which the shell
currently owns outright.

**C — leave it, and delete `tray_dnd.rs`.**
*What changes:* nothing a user sees; 1,184 lines of unreachable code go.
Pros: honest about the fact that neither half is finished; nothing pretends to
work.
Cons: throws away written, tested code for a feature the spec explicitly asks
for, and the decision still has to be made the day anyone wants tray icons.

**My recommendation: A.** The reason is not size but the one thing neither
option can fake — the tray and the taskbar share a *layout*. The tray's width
is computed from its contents (the clock alone roughly triples in width when
the date is switched on, which already had to be handled), and the taskbar's
window buttons shrink to fit what is left. Under B that arithmetic spans two
programs and has to be renegotiated on every change, which is the kind of seam
that produces a tray overlapping its neighbours in one theme and not another.
Under A it stays one function. The crash-isolation argument for B is real, but
it is an argument for isolating *tray icon plugins* — which is a separate
mechanism either way, since a third-party icon should not run in-process under
A *or* B.

**If this is never answered:** nothing degrades and nothing breaks. The tray
keeps showing a clock, a bell and a desktop indicator; no program can put an
icon in it; `tray_dnd.rs` stays unreachable. The cost is only that the
"minimize to tray" feature in `design.txt` cannot be started, since it needs
somewhere to minimise *to*.


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

# Resolved

**The body above holds OPEN questions only.** When the operator answers one,
write it up in `design-decisions.md` as a `Decided by: Operator` entry,
**delete the entry from the body**, and add one line here. That is the whole
point of the file: it is scanned for what still needs a decision, so an
answered question left in the body is pure cost — and, being older, it sorts
*first*, right where it is most in the way. (Why this is not append-only:
`design-decisions.md` §437.)

## Resolved — lane A

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
