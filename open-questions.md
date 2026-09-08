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

### My recommendation

**A.** The two search tools had to be unified because they are one feature with
two implementations — a disagreement with no upside. Backup is not that: its
dialect is the right one for its job and matches what every developer already
knows from `.gitignore`, where `[` is likewise not special in the common case.
The gain from B is small (a shorter way to write a few exclude patterns) and the
cost lands on data that already exists and that nobody will re-read to check.

If you prefer B, the change itself is small — `apps/backup` would call
`globmatch`'s class parser for the segment-matching step — and the real work is
deciding whether to warn about existing patterns containing a `[`.

### If this is never answered

Nothing is blocked and nothing degrades. The two dialects are documented in
`design-decisions.md` §555 and in `apps/globmatch`'s module docs, so the split is
a recorded decision rather than an accident. The only ongoing cost is that a user
who learns one pattern language may assume the other works the same way.

## C-Q10 — [C] In the light theme, small grey text on a shaded card is too faint to meet the readability standard, in about 850 places. Fixing it changes how the whole light theme looks. Which way? — Status: OPEN

**In short:** The desktop has a light theme and a dark theme. In the light one,
the smaller grey text — the second line of a list row, a caption under a
heading, a hint — is *too faint* wherever it sits on a shaded box rather than
directly on the page. There is a published standard for how far apart text and
its background have to be to count as readable (4.5, on a scale where 1 is
invisible and 21 is black-on-white). This text measures 3.4. It happens in
roughly 850 places across the settings screens, the launcher, the network and
sound panels, and more. The dark theme is fine. Fixing it means changing
colours that every screen and every application uses, so it will visibly change
what the light theme looks like — which is why I am asking rather than picking.

**Glossary, because the options below need three terms:**
- **Contrast ratio** — how far apart two colours are in lightness. 1 = identical
  (invisible), 21 = black on white. The standard asks 4.5 for normal-sized text.
- **Card** — any box drawn slightly shaded against the page, to group things:
  a settings row, a search result, a panel section. The theme has four shades
  of card, from barely-there to noticeably grey.
- **Secondary text** — the smaller, greyer text: captions, second lines, hints.
  Deliberately quieter than the main text, and that is the point of it.

**How this was found.** The problem was already logged, but only half of it: the
old note measured the *main* text colour and found two of the four card shades
slightly under the line. A measuring tool built on 2026-09-03 checked every
piece of text the desktop actually draws, against whatever is actually behind
it. The real table (light theme only; bold = below the 4.5 standard):

| ink | on the page | palest card | … | greyest card |
|---|---|---|---|---|
| main text | 7.06 | 5.17 | | **3.69** |
| secondary text | **4.64** | **3.40** | | **2.42** |
| accent (the themed blue) | **4.63** | **3.39** | | **2.42** |

Main text is mostly fine. Secondary text passes *only* on the bare page, at
4.64 — because that is the one place it was ever checked when it was chosen.
Put it on any card and it fails.

### The options

| | *What changes* | Cost |
|---|---|---|
| **A. Darken the greys** (recommended) | Captions and hints in the light theme look a bit darker and less delicate. Nothing moves; only three colours change. | The light theme reads slightly heavier. Contrast between "main" and "secondary" text shrinks, so the visual hierarchy is a little flatter. |
| **B. Lighten the cards** | Cards in the light theme become fainter — the shading that separates a settings row from the page gets subtler. | At the pale end the cards may stop being visible as cards at all, which is its own legibility problem (a different one, about structure rather than text). |
| **C. Forbid text on the darker cards** | Nothing changes colour. Panels would have to stop using the two greyest card shades behind text, and about 190 places would be re-laid-out. | The most work by far, and it constrains every future panel. But it is the only option that changes nothing a user has already got used to. |
| **D. Do nothing** | Nothing. | The text stays measurably below the standard, and it gets *wider* every time a new panel puts a caption on a card, because nothing stops it. |

**Why A is my recommendation.** It is three colour values, it fixes all four
card shades at once, and it is the same move already made once for this exact
palette: the secondary grey was *already* darkened, in June, to get it from 4.37
to 4.64 — but only ever checked against the bare page, which is why it fails on
cards now. Option A is finishing that job properly rather than starting a new
one. B fights the purpose of the cards, and C is real work that also permanently
narrows what a designer may do.

**If you would rather not decide:** say so and I will take A, since D is the
only option that leaves a known accessibility defect shipped, and A is the
cheapest of the three that fix it. It is fully reversible — three constants.

**What happens if this is never answered:** nothing breaks and nothing gets
worse on its own, but the light theme keeps shipping text below the readability
standard, and the count grows slowly as panels are added. The measuring tool is
in place either way, so whatever is decided can be verified rather than assumed.

**Where it bites:** `gui/appearance/src/lib.rs` — the light role table
(`LIGHT_SUBTEXT0`, `LIGHT_SUBTEXT1`, the `LIGHT_*` accents, and the
`LIGHT_SURFACE*` ladder). Full measurements and the module-by-module counts are
in known-issues.md under
`TD-C-TEXT-ON-THE-LIGHT-THEMES-TWO-PALEST-SURFACES-IS-BELOW-THE-CONTRAST-FLOOR`.

**A smaller, separate question found alongside it, same file:** if a user picks
a *custom* accent colour rather than one of the fourteen presets, it is used
exactly as given — the presets get a light-mode variant, a custom colour does
not, and nothing checks it is legible. So a user who picks a pale pink in the
light theme gets accent text at about 1:1, i.e. invisible. Should a custom
accent be (i) adjusted for the mode like the presets are, (ii) accepted but
warned about in the picker, or (iii) left exactly as chosen on the grounds that
the user asked for it? I lean (i), matching what the presets already do.

## Which group is a user in? Two files answer, and nothing keeps them agreeing. (lane B, 2026-09-06)

**In short:** "Alice is in the `audio` group" is written down in two separate
places on this machine: once in `/etc/group`, which lists the members of each
group, and once in Alice's own account record in `/etc/users.yaml`, which lists
the groups she is in. Nothing makes the two agree — they are simply two copies
of the same sentence, and copies drift. We already fixed exactly this problem
for *user accounts* (`design-decisions.md` §353: one file is the truth, the
others are generated from it). The question is whether to do the same for
groups, and if so which of the two copies survives.

**How it bites today:** a program that asks "what groups is Alice in?" gets a
different answer depending on which file it happens to read. `id` and `chown`
read one, the graphical settings app reads the other. If they disagree, a file
Alice should be able to open looks closed to one tool and open to another. This
is the same class of defect that had `sudo` and `doas` disagreeing about who
was an administrator, which is what §353 was decided to end.

**What has been done so far (2026-09-06):** `useradd`/`usermod`/`groupadd`/
`groupmod`/`groupdel` are one binary, and it now updates *both* copies through
a single set of methods, so it cannot change one and forget the other. That
closes the hole for the only tool that currently writes groups. It does not
close the hole — the next writer has nothing stopping it, and a hand-edit of
either file makes them disagree immediately.

### The options

| Option | *What changes:* | |
|---|---|---|
| **A. `/etc/groups.yaml` is the truth; `/etc/group` and `/etc/gshadow` are generated from it** (recommended) | Nothing visible day to day. Editing `/etc/group` by hand stops sticking — the next account change overwrites it — exactly as editing `/etc/passwd` already stops sticking. | The same answer §353 gave for users, for the same reasons, and it reuses the machinery that already exists. It is also the most work: a new file, a new parser, and the group half of `useradd` rewritten. |
| **B. Keep two files but make the account record's `groups` list the only writable one, and generate `/etc/group` from the accounts** | Also nothing visible. No new file: the group's member list becomes a view of who claims membership. | Cheaper than A. But a group has facts of its own — its gid, its password, its administrators (`/etc/gshadow`) — that no account record has anywhere to put, so those would still need a home. That is how §353's rejected option B failed. |
| **C. Drop the `groups` list from account records; `/etc/group` is the only answer** | The graphical settings app has to read `/etc/group` instead of the account file it reads now. | The smallest change, and it puts the fact in the file the POSIX world expects. It contradicts `design.txt`'s "configuration files will be yaml" for one of the two most security-sensitive files, which is the objection that sank the same option for users. |
| **D. Leave it. Keep both copies and keep them in step by hand.** | Nothing. | Free today. The cost arrives with the second writer, and it arrives as a security bug rather than a visible breakage. |

### If it is never answered

Nothing gets worse on its own, and nothing is blocked: `useradd` keeps both
copies in step, and it is the only tool that writes groups. The risk is a
future one — the *next* program that writes a group membership starts the drift,
and it will be found the way the `sudo`/`doas` split was found, by reading the
code rather than by anything failing.

The full context, including the non-atomicity of a save across the two stores,
is in `todo.txt` under "`/etc/group` and `/etc/gshadow` are still hand-written,
not generated".

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
- It would **not** have caught the `authlib` → `init/login` one, because
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
| 1 | `authlib` drops `with_stores`'s second argument (§353) | `init/login` | lane B, later | believed the caller list complete; had grepped `userspace/*/Cargo.toml`, which covers neither `apps/` nor `init/` |
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
lockscreen then stood between lane B's own `init/login` fix and a green `main`.
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
`init/login`, two lanes, neither caught by anything but a person looking), and
once from my own `guitk::Event` addition, which a boot test caught thirty
minutes in, and once again from that same variant in lane B's tree, found by
the run measuring what a gate would cost. All four are fixed; the mechanism
that let them through is untouched.

The cost grows with the number of app crates, which is growing.

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
- Q56 [Linux ABI exempt from native file-permission checks] — answered A
  2026-09-07: suspend-and-prompt or ahead-of-time grant; per-account
  default-grant policy. Operator follow-up pending lane A response.
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
  (§921): D: already excluded; likely CPU saturation + backup jobs; re-measure
  on E:.

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
