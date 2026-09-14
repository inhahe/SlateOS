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
not answered it. Two entries below (B-Q8, C-Q9) are open precisely because the
operator *did* reply and asked for a clearer explanation — which is a reply, and
which is invisible from this file alone.

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

**If this is never answered:** networking keeps working exactly as well as it does
today, which is one connection at a time. Nothing breaks that was not already
broken, and no data is at risk. What stays blocked is anything needing two at once —
a server accepting while serving, or a program fetching two things in parallel — and
the concurrency fix in `known-issues.md` `D-NETSOCK-SYNC` cannot be proven at all,
because the test that would prove it needs two sockets.

*Filed 2026-09-14 by lane A. Root cause and evidence are in `known-issues.md` under
the head-of-line witness entry: `socket.rs:356`, `netstack_client.rs:158`, and
`services/netstack/src/main.rs:2594`.*

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

**MEASURED 2026-09-12 (lane B) — and it changes the question.**

Option (d) below ended with *"Unknown until measured: whether the renderer and
the table agree today. Nobody has compared them."* They have now been compared.
**They disagree about 185,074 characters.** The dispute this question is about
covers 626. So the thing nobody had checked is 296 times larger than the thing
being asked.

The cause is simple and is visible in eleven lines of code. Our terminal
(`apps/terminal`) has **no notion of character width at all**: when it places a
character it moves the cursor one column, unconditionally, for every character
there is. It never consults `charwidth`, or any other table — it does not
depend on the crate. So "how wide we actually print" is, today, **one cell for
everything**:

| what the table says | how many characters | what the terminal does |
|---|---|---|
| two cells wide | 182,712 | one cell |
| zero cells (invisible marks) | 2,362 | one cell |

Concretely, and these are the recognisable ones rather than the obscure
corners: **every one** of the 20,992 Chinese characters, **every one** of the
11,172 Korean syllables, the Japanese kana, the fullwidth forms, and the 80
emoticon emoji are marked two cells wide in our table and drawn one cell wide
by our terminal. In the other direction, 1,281 combining marks — the accent in
a decomposed `é`, which is the letter `e` followed by a separate mark — are
marked zero cells and are given a cell of their own, so a decomposed accented
letter takes two cells on screen where every layout calculation reserved one.

**What this does to the question being asked.** Options (a) and (b) are a
choice between bash's answer and the GNU tools' answer on 626 characters. On
SlateOS *both* of those answers are wrong against our own screen for 185,074
characters, including every character in Chinese, Japanese and Korean. That
does not make the choice pointless — it is still the right choice for matching
upstream byte-for-byte, which is what the differential harnesses measure — but
it does mean **the choice is not what makes our screens correct**, and the
entry previously read as though it were.

**The operator's instinct was right, and following it is what found this.**
"Why wouldn't the table simply report how wide we actually do print each
character" is option (d), and it could not be evaluated before because nobody
had looked at what we print. Now that someone has: (d) is not a matter of
adjusting 626 entries to match the renderer. Taken literally today it would
mean *setting the whole table to one*, which would make our `ls` disagree with
every real terminal on Earth while agreeing with ours. The renderer is the
thing that is wrong, not the table.

**This is a defect in its own right and is not yours to decide.** A terminal
that draws Chinese, Japanese, Korean and emoji one cell wide cannot display
those languages correctly no matter which table the utilities read — text
overwrites itself and every column is off. It is filed to lane C, who own the
terminal, as `requests/b-c-the-terminal-gives-every-character-one-cell.md`. It
is a separate problem from this question and neither blocks the other.

**And the second half of the operator's question — "why wouldn't the programs
just ask?" — they do.** Every one of our programs asks one table; that part
already works as you would expect. Two things stop it from settling anything,
and they are the two listed above: no program can ask the *terminal* how wide
it will draw something, because terminals expose no such query; and on Linux,
bash and the GNU tools ask two different tables, which is the entire origin of
the 626.

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

### A fourth incident, 2026-09-13 — and the first where the tooling caught it

Lane C added a variant to `guitk::Event` (a tray-icon click). The five
crates that changed were tested and green. `apps/explorer` and
`apps/stickynotes` match that enum *exhaustively*, so both stopped
compiling — two crates nobody had named, in the lane's own tree.

What is new is how it surfaced. `scripts/workspace-test.py` builds and runs
every target, and it reported:

```
[workspace-test] targets passed: 0
[workspace-test] runner exited 101 with no failing test — build or launch error.
```

Note the shape: **zero targets and no failing test**, which is what a build
break looks like from inside a test runner. A filtered log would have looked
identical to a clean one.

**What this is and is not evidence for.**

| | |
|---|---|
| Does a full-workspace build catch real breaks? | Yes, demonstrably, four times now |
| Was it cross-lane this time? | **No** — both broken crates were lane C's own |
| Did it block a merge? | No. It was run voluntarily, before pushing |
| Would option C (grep before the claim) have caught it? | **No.** No symbol was removed; a variant was *added*, and exhaustive matches break on additions. There is nothing to grep for |

That last row is the part worth weighing. Option C was the cheap answer
that would have caught the first incident; it cannot catch this one even in
principle, because the failure is an addition rather than a removal and the
broken code names nothing that changed. A convention about what to grep
before claiming "no caller changes" does not help when the claim was never
made.

It also weakens the case for A over B slightly, in an honest direction:
the run that caught this was **voluntary**, not a gate, and it was run
because the lane's habit is to run it before pushing. One data point is not
a policy, and the habit is exactly the kind of thing that decays when the
session changes — which is the objection this document already raises
against option C.

### Option A is already being run, by one lane, and here is what it costs

**Lane C has run the whole-workspace build and test before every merge to
`main` for some time, voluntarily.** That is not quite option A -- it is option
A plus running the tests, so it is the *expensive* end of the range -- but it
answers the part of the question that estimates could not: what happens when
somebody actually does this.

**It covers the incident that raised this question.** `Cargo.toml` lists
`"apps/*"` among the workspace members, so `cargo test --workspace` builds
`apps/lockscreen`. The change that broke it would have gone red here before it
reached `main`.

**Measured 2026-09-14, eight consecutive runs on the shared machine, under
whatever contention the other two lanes were producing at the time:**

| | seconds |
|---|---|
| runs | 8 |
| each | 336, 339, 342, 351, 375, 422, 446, 578 |
| mean | ~399 s, a little under seven minutes |
| total for the day | ~53 minutes of machine time, for one lane |

**The 578 is the number to look at, not the mean.** It is the slowest of the
eight by 30%, and it is slow for a knowable reason: lane A started a full boot
run while it was going. That is the contention this entry's earlier
measurements kept flagging as the missing figure, caught here by accident
rather than by design -- one lane's gate and another lane's boot are the two
heaviest things on this machine, and nothing schedules them apart. Three lanes
each running a seven-minute gate before each merge will not cost 3 x 7.

**And what it caught in those eight runs: nothing real, twice.** Two of the
eight went red. Both were the same lane-B timing test (`oils`
`a_poll_before_the_grace_does_not_lose_the_exit_forever`), which measures a
real clock against a 20 ms grace and misses its window when two dozen test
binaries share the machine. Neither was a product defect. Each cost a six-minute
re-run before anything could be merged.

**Which cuts both ways, and the second way is the one worth weighing.** The
gate plainly works -- it builds what nothing else builds, it is affordable, and
lane C has absorbed it without complaint. But on the day it was measured its
whole observed output was two false alarms, and a gate whose red means "either
something is broken or the machine was busy" is one that teaches its readers to
re-run first and think second. Lane A reports the same thing from the other
end: of four breakages it inherited from `main` on 09-12, each costing a boot
run, only one was a compile error `cargo check` would have caught.

So the cost of option A is not really the six minutes. It is that mandating a
gate across three lanes *at the suite's current flakiness* buys a signal the
lanes will learn to discount -- and a discounted gate catches nothing at all,
which is strictly worse than the honest "we do not build `apps/`" we have now.
If the answer is A, it is worth pairing with a rule that a test which cannot
tell a scheduling accident from a defect gets fixed or quarantined.

*(Recorded by lane C, which is the lane paying this cost, and is therefore the
least neutral party to report it. The numbers are wall-clock from
`scripts/run-timeout.py` and can be re-derived from any day's transcript.)*

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

### The operator's question, answered with the frequency term (lane A, 2026-09-13)

*How much time would C+B save over A, and what is the harm in catching an error
a day later?*

**The term nobody had measured is how often the gate fires.** `git log
origin/main --merges`: **489 merges in 10 days**, 86 of them yesterday. So:

| | per run | per day |
|---|---|---|
| **A**, whole-workspace, warm, contended | 15 s | ~12 min at 49 merges/day, ~21 min at 86 |
| **A**, scoped (158 `-p`), same conditions | 8 s | ~7 min / ~11 min |
| **B**, one sweep, schedulable when idle | ~15 s warm | ~2 min, near-zero contention |
| **C**, a grep on shared-library commits | ~1 s | negligible |

Direct saving of C+B over A: **10-20 minutes of machine time a day**. Counting
Hole 2 -- this machine saturates on a single cargo run, so each firing also
degrades the other two lanes for its duration -- plausibly **25-60 minutes of
aggregate lane time a day**.

**The harm of a day's delay is real, and I can price it, because it happened to
me four times on 2026-09-12.** A breakage on main propagates: every lane that
merges inherits it and spends a boot cycle (10-30 min) rediscovering it.

**But only one of those four was a compile error.**

| inherited breakage | would `cargo check --workspace` catch it? |
|---|---|
| `stdin-hang-sweep.sh` SC2046 | no -- shell lint |
| `getent` `unwrap_or_default` | no -- check-read-defaults |
| `fio` reading absent `procinfo` fields (E0609) | **yes** |
| `check-dead-code-allows` missing `newline=` | no -- text-mode gate |

So A addresses about a quarter of the observed breakage traffic, at a cost paid
on all ~50-90 merges a day. The boot test already catches all four categories;
A only moves one of them earlier. On this evidence the trade is unfavourable:
12-22 min/day of machine time to save perhaps one 30-minute rediscovery,
before counting what it does to the other two lanes.

**Caveats, stated because one day is a small sample.** If compile breakages are
commoner than 1-in-4, the arithmetic moves. And A is not currently possible at
all: `cargo check --workspace` fails outright on a clean tree because the kernel
embeds `services/hello`'s artifact and nothing builds it (Hole 1) -- that has to
be fixed before A is even an option, whereas B and C could start today.

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

### A hazard in the scoped form itself

Before that 8 s counts in the scoped form's favour, somebody has to answer a
question neither measurement asked: **how was the list of 158 `-p` flags
built?** That is the only part of this still unrecorded, and it is lane B's to
answer.

`-p` takes a *package* name, and nine directories under `apps/` and `gui/` are
not named after their package. Three of the nine -- `backup`, `indexer`,
`sysinfo` -- resolve to a **different crate that really exists**, in
`userspace/`; the other six error. See
`known-issues.md` -> `TD-B-FIVE-CRATES-CANNOT-BE-REACHED-BY-THEIR-DIRECTORY-NAME`
(filed 2026-09-10, gated as Gate 18 of the boot test). Not repeated here.

So: if the 158 came from `cargo metadata`, the number stands. If from directory
names, the run could not have completed unless the six erroring names were
special-cased and the three silent ones were not -- in which case the 8 s
measured three of the wrong crates and skipped three of the right ones.

**Why this belongs in the decision and not only in the measurement.** A gate
built out of `-p` flags carries that failure permanently; a `--workspace` gate
cannot, because it names nothing. That is a point on the coverage axis, not the
cost one. Lane C hit the live version on 2026-09-14: `cargo test -p sysinfo` on
a crate in `apps/` ran `userspace/sysinfo`'s tests and printed "26 passed",
which is a true sentence about tests that really ran and no answer at all to
the question asked.

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

## C-Q15 — [C] Under the optional "Filled" theme, should the shaded boxes be made paler? — Status: OPEN

**In short:** you chose outlined boxes as the normal look and kept the older
filled-box look as an option people can switch to. In that filled look, boxes
are shaded grey, and text on a grey box is harder to read than text on the
white page. I have just made the *text* darker automatically so it is always
readable — so nothing is broken either way. The question is whether you would
also like the grey boxes themselves made a little paler, which would let the
text stay closer to the colour you actually picked.

**Glossary.** *Contrast* here is the standard accessibility measure of how far
apart two colours are in brightness; 4.5 is the minimum for ordinary text.
*Accent* is the one colour you pick that appears throughout the interface —
currently the blue-green `#00688B`.

**Why this is not urgent.** Every combination now clears 4.5 automatically, in
both themes, for all fourteen accent choices and any custom one. This is purely
about how much the accent has to change when the filled theme is switched on.

**How much it changes today.** Under the normal outlined theme, almost nothing —
at most six units of colour, which nobody can see. Under the filled theme it is
visible: a green accent renders as a noticeably deeper green on a card than the
swatch you chose it from.

**The options**

**A. Leave the greys as they are.**
*What changes:* nothing. Under the filled theme the accent renders deeper than
its swatch; under the normal theme it is unchanged.
For: no work, and the automatic adjustment already guarantees readability.
Against: someone who picks a bright accent and then switches themes sees it go
muted, with no explanation on screen.

**B. Make the shaded boxes paler, so the accent moves less.**
*What changes:* cards, selected rows and sidebars become lighter greys in the
filled theme; accents render closer to the swatch you picked.
For: the accent you chose is more nearly the accent you see.
Against: paler cards are harder to tell apart from the white page — which is
the entire job of a filled theme — so this trades one visible defect for
another, and there is not much room: a card must stay distinct from the page.

**C. Show the resolved colour on the settings swatches.**
*What changes:* the accent swatches in Settings are drawn in the colour that
theme will actually use, so the picker and the desktop agree.
For: removes the surprise without touching any grey. Cheap.
Against: the fourteen swatches would look different under the two themes, which
some people would read as a bug rather than as honesty.

**My recommendation: C, and then A.** The complaint B addresses is really "the
swatch lied", not "the accent is wrong" — and C fixes exactly that, for the
cost of drawing the swatches through the palette instead of from the constants.
B spends the one thing the filled theme cannot spare, which is the distance
between a card and the page. If after seeing C you still find the deeper accents
muddy, B is still available and nothing about C forecloses it.

**If it is never answered:** option A happens, and nothing degrades. Text is
readable in every combination today. The only cost is the mild surprise
described above, and only for people who switch to the optional theme.

## C-Q16 — [C] Should the games follow the desktop theme, or keep their own colours? — Status: OPEN

**In short:** about forty small games ship with the system — chess, solitaire,
minesweeper, tetris and so on — and each one has its colours written into it
rather than taking them from your theme. Twelve *applications* have the same
problem and that is plainly a bug: a file manager should be light when you
choose the light theme. For the games I am not sure it is a bug, and I would
rather ask than decide it with a script. A chess board's light and dark squares
are the game's own look, the way a photograph in an image viewer is not
something the theme should tint.

**What is definitely being fixed either way:** every game's *chrome* — its
menus, score panels, dialogs, and the window background behind the board. Those
are interface and they should follow your theme. The question is only about the
playing surface itself: the board, the pieces, the tiles, the cards.

**The options**

**A. The board keeps its own colours; only the chrome follows the theme.**
*What changes:* a chess board looks the same in light and dark mode; the menu
bar and score panel around it change.
For: a game's board is artwork, and forty games designed around their own
palettes will not all survive being recoloured. Solitaire's card backs,
minesweeper's numbered tiles and tetris's seven piece colours are conventions
people recognise. Against: a dark-theme user gets forty bright rectangles.

**B. Everything follows the theme, boards included.**
*What changes:* a chess board is drawn in two shades from your palette; tetris
pieces take palette hues.
For: complete consistency, and dark mode is genuinely dark.
Against: tetris's pieces are *identified* by colour (the standard seven), and
minesweeper's numbers 1–8 have fixed colours that players read at a glance.
Recolouring those makes the games worse, not just different.

**C. Per-game, decided by whether the colour carries meaning.**
*What changes:* minesweeper and tetris keep their colours (they mean
something); chess, checkers and solitaire's felt take the theme (they are
decoration).
For: the only option that respects both arguments.
Against: forty individual judgements, and someone has to make them.

**My recommendation: A**, with C available later for the handful where the
board is obviously just decoration. A gets dark-mode chrome everywhere for a
modest, mechanical change, and it cannot make any game worse — which B
demonstrably can. The cost of deferring C is nothing: it is the same work,
game by game, whenever anyone cares.

**If it is never answered:** I do the chrome (which A and C agree on) and leave
the boards alone. Nothing breaks; the games simply keep their current
appearance, which is what they have today. This question is genuinely safe to
leave — it is here because forty crates is too many to change on my own guess
about taste, not because anything is blocked.

## C-Q17 — [C] Five finished features are built into the system but cannot be used. Wire them up, or delete them? — Status: OPEN

**In short:** five applications each contain a complete, tested feature that no
part of the program can reach — the code is compiled into the system and there
is no button, menu or keystroke that leads to it. Between them that is 327 KB
of code and 214 tests, all passing. I can wire them into their applications or
remove them, and those are very different amounts of work, so I would rather
ask than guess.

**What they are**

| where | what it does | size |
|---|---|---|
| the installer | configures the GRUB bootloader | 48 KB |
| the image viewer | plays video | 77 KB |
| the process explorer | click a window to find its process; show what a process is waiting on and detect deadlocks; set CPU affinity and priority; browse a process's memory map and environment | 83 KB |
| system information | queries hardware details | 73 KB |
| settings | a remote-settings page | 46 KB |

**Why nobody noticed.** Each has its own tests and they all pass, because a
test calls the code directly — it does not have to find a way in through the
interface. This is the pattern `known-issues.md` records as lesson 47, and the
sharp version of it: the process explorer's *own source* quotes that lesson
while this module sat beside it.

**The options**

**A. Wire them up.** *What changes:* the installer can set up a bootloader, the
image viewer plays video, the process explorer gains six tools, and so on.
For: the code appears finished, and someone wrote and tested all of it. Against:
it is the largest of the three options, and each one needs interface design —
a menu item, a panel, a keyboard shortcut — that does not exist yet.

**B. Delete them.** *What changes:* nothing a user can see; the system gets
327 KB smaller. For: honest — the tree stops claiming to have features it
cannot offer. Against: throws away working code, including a deadlock detector
and a bootloader configurator that are not trivial to rewrite.

**C. One at a time, by value.** *What changes:* the installer's bootloader gets
wired up because an installer that cannot install a bootloader is a real gap;
the rest are judged individually. For: puts the effort where it matters.
Against: needs a judgement per module rather than one decision.

**My recommendation: C, starting with the installer.** An installer that cannot
configure a bootloader is a different severity of problem from a process
explorer without a window picker, and treating them as one question gets the
installer either over- or under-served. I would not delete anything until each
has been looked at — deletion is the only irreversible option here.

**Since this was filed, the ongoing cost stopped being hypothetical.** On
2026-09-13 two of the five were converted to the user's colour palette --
the image viewer's video module and the process explorer's features module --
because a tree-wide sweep found them and there is no way for a sweep to know
that nothing runs them. That is 106 constant uses and two `&Palette`
parameters threaded through code that cannot be reached, done on the
reasoning that it is independent of this answer: it compiles and is tested
either way, and if it is ever wired up it now arrives with the right
colours. The same will be true of the next sweep, and the one after.

**A seventh, found on 2026-09-13.** `gui/desktop/src/launcher.rs`'s
`LauncherState` is constructed only inside its own test module. The shell
imports two *types* from that file -- `AppEntry` and `Category` -- and nothing
else; the launcher that actually runs is `apps/launcher`, which has its own
state machine of the same name. It was found by trying to wire an
accessibility setting into it and noticing the setter would never be called.
That is the second time in one day that this class has been found by *nearly
doing work inside it*, which is the ongoing cost this question is about.

**And the same shape turned up outside the five.** The desktop's icon layer
(`DesktopIconLayer`, `gui/desktop/src/icons.rs`) is named nowhere but its own
file, and it is what would read the *desktop icon size* setting -- so that
preference has a working control in Settings and no consumer that runs.
`cursor_size` and `cursor_scheme` are in the same position. Those are not
part of this question and are logged separately, but they say something about
it: **an answer of "delete" would need a rule, not a list**, because the
list keeps growing as people look.

**If it is never answered:** nothing breaks and nothing degrades; the system
keeps carrying code it cannot run. The cost is ongoing rather than sudden —
every sweep, every conversion and every audit pays attention to these files.
I spent real effort on one of them tonight before discovering it was
unreachable.

## C-Q18 — [C] Nothing draws the mouse pointer. When we start, what happens to fullscreen video and games? — Status: OPEN

**In short:** SlateOS does not draw a mouse pointer. On the development
machine you see Windows' arrow, borrowed from the host; on real hardware there
would be no pointer at all. The system already works out *which* pointer to
show — an I-beam over text, arrows on a window edge — and then draws none of
them. Starting to draw one is straightforward except in a single case:
fullscreen video and games currently take a shortcut that skips drawing
altogether, and a pointer cannot be painted on top of a frame that is never
painted. What you are choosing is what happens in that case.

**Glossary.** *Direct scanout* is the shortcut: when one window covers the
whole screen and is fully opaque, its picture is handed to the display exactly
as the program drew it, with no copying. It is what makes fullscreen video and
games cheap. A *hardware cursor* is a pointer the graphics chip draws for
itself, from a small image the display controller holds separately — it costs
nothing per frame and does not disturb the picture underneath.

**Where it bites:** `gui/compositor/src/lib.rs` — `compose_frame`'s
`direct_scanout_window` bypass, and `cursor_shape`, which is computed on every
pointer move and read by two tests.

**What is already true, measured 2026-09-13:** `CursorShape` has ten members
and the compositor picks the right one continuously. Across all three
presenters there is exactly one line of cursor code — `LoadCursorW(IDC_ARROW)`
in the Windows host window class — so the shape is chosen and discarded. The
kernel has cursor-plane support (`kernel/src/drm/`); the compositor's DRM
presenter never reaches for it.

**The options**

**A. Draw the pointer in software, always — fullscreen loses its shortcut.**
*What changes:* the pointer appears everywhere, and fullscreen video and games
go back to being composited frame by frame.
For: one code path, correct on every backend, and the pointer is never missing.
Against: it spends a measured performance feature on a 32×32 image. The
shortcut exists because copying a 4K frame is expensive, and this would pay
that cost on every frame of every film.

**B. Draw it in software, except over fullscreen content.**
*What changes:* the pointer appears everywhere except on top of a fullscreen
video or game, where it disappears.
For: keeps the shortcut, and for games it is arguably *right* — a game hides
the pointer itself. Costs nothing new.
Against: a fullscreen video player with on-screen controls becomes unusable,
because you cannot see what you are pointing at. "The pointer vanishes
sometimes" is a hard thing for a user to form a rule about.

**C. Ask the graphics chip to draw it, with software as the fallback.**
*What changes:* the same as A from the user's side — a pointer that is always
there — with fullscreen keeping its shortcut on real hardware.
For: it is what the hardware is for, and it is the only option where nothing
is given up. The kernel already has the plane support.
Against: the most work by a wide margin, and the development host has no such
plane, so the software path has to exist anyway and B or A is what a developer
would see. Two paths mean the one you test is not the one that ships.

**My recommendation: C, built as B first.** The software renderer is needed
either way — it is the fallback, and it is what the dev host will use — so the
first commit is the same under all three answers. The question is only what
happens when it meets a fullscreen window, and B is a safe place to stand while
the hardware path is built, because it is the one answer that gives nothing up
today. What I would not do is A: spending direct scanout permanently, to solve
a case that C solves properly, is the kind of trade that is easy to make and
hard to take back.

**If it is never answered:** there is no pointer on real hardware and Windows'
arrow on the development host, which is also what makes the current state easy
to miss. Three settings stay inert — `cursor_size`, `cursor_scheme` and the
whole `CursorShape` vocabulary — and every accessibility question about pointer
size stays unanswerable. Nothing degrades with time; it simply does not exist.
## C-Q20 — [C] Four lists of "which programs are installed", and nothing can read the others. Which is the real one? — Status: OPEN

**In short:** four different parts of the system each keep their own list of
what programs exist on the machine, and the lists disagree. No program can read
another's. The visible consequence today: the Settings app cannot offer you a
choice of web browser, because it has no way to find out what browsers are
installed — so that screen shows a placeholder. The question is which list
should become the one everybody reads.

**The four lists**, with what each knows:

| where | holds | who can read it |
|---|---|---|
| `kernel/src/fs/appregistry.rs` | 9 built-in apps, categories, MIME types | `/proc/appregistry`, as a **human-readable report** -- see the correction below |
| `gui/desktop/src/launcher.rs` | 22 apps with real binary paths, drives the start menu | the desktop shell only |
| `apps/fileassoc` | 8 apps and which file types they open | nobody -- it is a program, not a library |
| `gui/desktop/src/default_apps.rs` | a third app list plus per-role defaults | nobody -- 2,325 lines no menu opens |

They are not copies of one list. Until 2026-09-14 the `fileassoc` one named
eight programs that **do not exist in this tree** (`textedit`, `photoviewer`,
`browser`, `office`…) while the shell's named the real ones; that half is fixed,
but the disagreement was invisible for as long as the lists were.

**What it blocks right now.** `design-decisions.md` 815 (the operator's answer
to C-Q6) says screens you *open* move into the Settings app and the shell's
copies are deleted. `default_apps.rs` is one of those screens. Porting it needs
a list of installed programs to choose between, and the Settings app can reach
none of the four. So a decided piece of work is stopped on this.

### Options

**A. The kernel's registry is the authority; give its view a form a program
can rely on.** `/proc/appregistry` already exists and already lists every app's
name and binary path. What it does not have is a shape anything but a person can
depend on: it opens with "Apps: 9/4096" and groups entries under category
headings, which a program would have to scrape.
*What changes:* installing a program makes it appear in the start menu, the
Settings app and the file manager at once, without any of them being told.
*Cheaper than it first appears:* the data and the plumbing are built; this is a
second view in a stable format, not a new subsystem.
*Against:* A-Q8 answered the same question for desktop icons with "it leaves
the kernel", and a list of GUI programs is a weaker claim on kernel space than
icon coordinates were.

**B. A userspace library is the authority; the kernel's registry goes.**
One crate under `gui/`, read by the shell, the Settings app, the file manager
and the File Associations program.
*What changes:* the same as A from the user's side. The difference is where it
lives and who may change it.
*Dearer than it first appears, and this is the correction that matters:*
`fs::appregistry` is **not** an island. `fs::startmenu` calls it at eleven
sites, `fs::procfs` reads both, `/proc/startmenu` exists as well, and both run
at boot. So B is "delete a module with a `/proc` surface and a live in-kernel
consumer, and decide what becomes of `fs::startmenu` and `/proc/startmenu`" --
a materially larger change than removing `fs::deskicons` was, and all of it in
lane A's tree.

**C. Leave them separate.**
*What changes:* nothing today. The Settings app's "default browser" screen stays
a placeholder, and the four lists go on disagreeing silently.

### If this is never answered

Nothing breaks and nothing gets worse on its own — but `default_apps.rs` and
the Settings app's Apps section stay where they are, which means one of C-Q6's
own consequences cannot be carried out. The lists will also drift again: the
`fileassoc` one drifted to eight fictional programs without anyone noticing,
because nothing compares them.

**Recommendation: A or B, and the gap between them is narrower than this
entry first claimed.** The original recommendation was B on the precedent of
A-Q8, written while believing the kernel's registry had no `/proc` view and no
consumers. Both were false (below). With the plumbing already built and
`fs::startmenu` depending on it, A is the smaller change and B is the larger
one, which is the reverse of what was written.

The principle still favours B -- what a user has installed is a property of
their userspace, and every consumer of the list is a GUI program. The cost now
favours A. That is a genuine trade rather than an obvious answer, which is why
it is the operator's.

Whichever is chosen, the defect to fix is that the four lists are four. Any
option that leaves two of them is option C wearing a better name.

### Correction, 2026-09-14: this entry was wrong about the kernel's registry

The first version said `fs::appregistry` was "reachable only from the kernel's
own debug shell -- there is no `/proc` view and nothing in userspace names it".
Both halves are false, and lane A caught them within the hour:

* `/proc/appregistry` is registered in `procfs.rs`, generated by
  `gen_appregistry()`, and dispatched. It prints every app's name and binary
  path.
* `fs::startmenu` calls `appregistry::get`, `search` and `menu_tree` at eleven
  sites, and is itself read by `/proc/startmenu`.

**Where the wrong picture came from.** A comment in `kernel/src/main.rs` reads
"appregistry and startmenu were reachable only from `kshell`" -- past tense,
describing a condition somebody then fixed. It was read as a statement of the
present. That is the same failure as reading five `os-lane-[abc]` grep hits as
five defects when four were prose, and as "only `sysinfo` collides" written
from one sample: **documentation about a past state is indistinguishable from a
description of the current one, to a search.**

The premise "four lists, none readable by the others" is therefore too strong.
Three of the four cannot be read by anything; the kernel's can be read by
anyone willing to scrape a status report. The question -- which list is the one
everybody reads -- stands, because scraping a human-readable report is not an
interface.

## C-Q19 — [C] An event you coloured like your accent is invisible on today's date. Whose colour wins? — Status: OPEN

**In short:** every calendar event can carry a colour you pick, and the month
grid marks the event's day with a small dot in it. Today's date is drawn as a
filled circle in your accent colour. Pick an event colour close to your accent
and the dot lands on that circle and vanishes — the event is still there, the
mark saying so is not. It affects one cell in forty-two, only when you chose a
colour, and only when that colour is near your accent. The question is whether
the system may quietly darken *your* colour to keep it visible.

**Where it bites:** `gui/desktop/src/calendar.rs`, `render_day_cell`.

**Why this is being asked now rather than in August**, when it was first
noticed and filed as
`known-issues.md` → `TD-C-A-USER-CHOSEN-EVENT-COLOUR-CAN-VANISH-INTO-THE-TODAY-DISC`:
the option that was least attractive then is cheap now. `appearance::legible_on`
landed on 2026-09-12 and does exactly one thing — move a colour the smallest
distance that clears the 4.5:1 floor, and nothing at all when it already does.
It **preserves hue**: green becomes `#245A18`, blue stays `#0036A3`. So option
C below is no longer "invent a darkening rule", it is one call.

**And there is an argument on record against it**, written at the call site
when the current behaviour was chosen: *"An event the user coloured keeps that
colour everywhere, even on today's disc: it is their data and the calendar
does not get to overrule it."* That principle is applied elsewhere in this
lane and was applied twice on 2026-09-13 — `apps/whiteboard`'s stroke colours
and `apps/screenshot`'s annotations are both left unfloored because they are
the user's drawing, not the theme's. This entry is the one place where the
same principle produces something the user cannot see.

**The options**

**A. Leave it. The colour you picked is drawn, always.**
*What changes:* nothing. An event coloured like your accent has no visible
mark on today's date.
For: your data is never altered, and the principle stays simple enough to
state in one line. Against: the one case it costs is the case where the mark
exists to be seen.

**B. Draw a thin ring around every coloured dot.**
*What changes:* every event dot in the grid gains a one-pixel outline in the
cell's own background colour, today's or not.
For: no colour is ever altered, and it fixes the general problem rather than
today's instance. Against: it changes the look of all forty-two cells to solve
one, and a ring on a six-pixel dot is most of the dot.

**C. Darken or lighten the dot only when it would otherwise be invisible.**
*What changes:* a dot whose colour is near your accent shifts far enough to be
seen, on today's cell only. Every other dot is untouched, including the same
colour on any other day.
For: one call to the mechanism the rest of the theme already uses, and it is a
no-op for nearly every colour. Against: it is still the system changing a
colour you chose, which is the thing A exists to refuse.

**My recommendation: C**, but weakly, and I nearly implemented it without
asking — the call-site comment is what stopped me. The reason to prefer it is
that a mark you cannot see is not a smaller failure than a colour shifted by a
shade. The reason to hesitate is that **A is the rule this lane follows
everywhere else**, and an exception needs to be worth the inconsistency.

**If it is never answered:** nothing degrades. The event is still in the day's
detail card, whose colour bar sits on `mantle` and is unaffected, so the
information is reachable — just not from the grid.
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


# Resolved

**The body above holds OPEN questions only.** When the operator answers one,
write it up in `design-decisions.md` as a `Decided by: Operator` entry,
**delete the entry from the body**, and add one line here. That is the whole
point of the file: it is scanned for what still needs a decision, so an
answered question left in the body is pure cost — and, being older, it sorts
*first*, right where it is most in the way. (Why this is not append-only:
`design-decisions.md` §437.)

## Resolved — lane A

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
