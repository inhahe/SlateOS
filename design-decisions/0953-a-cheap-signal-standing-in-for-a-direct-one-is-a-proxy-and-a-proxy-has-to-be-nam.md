## 953. A cheap signal standing in for a direct one is a proxy, and a proxy has to be named as one

**Date:** 2026-09-18 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** A &middot; three instances mine in one day, a fourth contributed by lane C

**In short:** four times in one day I answered a question by looking at
something *near* the answer instead of the answer, and did not notice I had
substituted. None of them were careless in the moment -- each was a true
observation about the wrong thing. The rule this produces is not "be more
careful": it is that when a cheap signal stands in for a direct one, say so
where the conclusion is written, because the substitution is invisible
afterwards.

**The four, which have nothing in common but their shape.**

| I looked at | I concluded about | what was actually true |
|---|---|---|
| a variable's **declaration** (`SLATE_ZIG=$SLATE_SPIKE/zig/zig`) | where zig lives | `slate_ensure_zig` reassigns it; it resolved to the shared cache, and `build/spike/zig` does not exist |
| a module's **name** (`binfmt`) plus its signatures | what it is for | line 1 says *statistics*; I called it a registry |
| a gate's note about the **default** invocation ("`cargo clippy` runs for a Windows target") | what any invocation can reach | `x86_64-unknown-linux-gnu` is installed; the check takes one flag |
| a **search returning nothing** (lane C's: `grep cannot`) | what the program admits | it says `Cannot`; there were five |
| a **wall of 2114 compile errors** (lane C's) | a font crate being broken | `--target` was omitted, so cargo built for the bare-metal default and every `std` import failed |

Declaration vs call site. Name vs line 1. Default vs possible. Silence vs
absence. **Volume vs evidence** -- lane C's sharpest: *a wall of errors is as
unconditional as a label.* All 2114 of their messages were true, and
together they said only that the question had been misconfigured; nobody
doubts 2114 errors, which is exactly what makes the count persuasive and
uninformative at once. The same family as an output line that states its
conclusion regardless of the data -- I wrote `count; echo "(0 = torn
down)"` three times in one day and it printed its claim over a count of 4.
The silence row is also lane C's, from their
`TD-C-SEVEN-WAYS-A-SEARCH-SAYS-NOTHING-AND-MEANS-NOTHING`, which has seven
more of the family that I did not have.

**Why this is a decision and not just a list of mistakes.** Because the
proxy is usually the *right* thing to reach for. Reading a declaration is
how you find a variable; grepping is how you find code; a module's name is
how you find the module. Banning the cheap signal would mean never
navigating at all. What went wrong each time was not the lookup, it was
writing the *conclusion* in terms the lookup could not support -- and by
then the proxy is gone from the sentence and only the claim remains.

So the practice is: **when the conclusion is load-bearing, take the direct
observation, and when you keep the proxy, name it in the artifact.** "No
caller found by grep" and "not called" are different sentences and only one
of them is defensible. Concretely, for each row above the direct observation
was cheap and available: run the function and see where the file landed; read
line 1; run `rustup target list --installed`; grep case-insensitively, or
better, compile.

**The corollary that cost the least and taught the most.** Earlier the same
day I nearly recorded that the kernel's file-immutability protection did not
work, having established by call-graph that its only caller was a test. The
thing that settled it in one command was *does the test pass* --
`[ext4] immutable: write, truncate and unlink are all refused ... OK` on
every boot. A call-site search is evidence about one route to a behaviour; a
passing behavioural test is evidence about the behaviour. Same substitution,
pointed the other way, and it would have told a reader not to rely on
something they can rely on.

**The test that makes a negative claim defensible, contributed by lane C
after reading the above:** *"no caller" and "nothing writes it" are
different sentences, and only the second survives a second implementation
path.*

That is exactly why the immutability case broke. "`is_writable` has one
caller and it is a test" is a claim about **one predicate**, and there were
three other paths -- `write_file`, `truncate`, `unlink` -- each refusing on
its own. A claim about *the data* rather than about an entry point would
have held: nothing writes it by any route is not defeated by finding
another route.

Lane C re-ran their own two newest findings against this and both survive,
for that reason rather than by luck:

| finding | the defensible form |
|---|---|
| slides "cannot type" | not "`set_text` has no caller" -- there is no `set_text`. Zero assignments to `.text` anywhere in the crate, tests included |
| notes "cannot make one" | not "`create_note` has no caller" -- both `notes.push` sites are *inside* the two unreachable creators. The mutation was checked, not the entry point |

So the call graph is fine for finding candidates and indefensible in the
conclusion -- the same split as the four rows above, stated as a usable
test rather than as a warning.

**Lane C also hit this corollary in the worse direction the same day**, which
is worth recording because it is the direction that costs a reader something.
They filed `apps/weather` as drawing a dashboard over an empty model --
constructor checked, fields checked, every writer of `current` checked, all
true -- corrected it once, still wrong, and withdrew it: `render_commands`
returns through `render_cannot_fetch` and prints four lines saying it cannot
fetch, ending *"Silence here is not an all-clear."* The app was handling
emptiness better than the proposed fix would have. They had answered "can
this show anything" from the model when only the draw can answer it.

Both of us landed on the same one-command version in different domains:
**stop asking where the call site is; ask what the program does when it
runs.** For the kernel that is a boot line; for an app it is the render tree.

**The same substitution applied to a FIX rather than to a claim, contributed
by lane C 2026-09-18:** *a remedy applied where the failure was seen does not
reach the places it was not.*

Their case is unusually complete, which is what makes it worth recording.
An intermittent failure in `gui/desktop` turned out to be a race on the
process environment: `populate_defaults` reads `HOME`, `settingsfile::testing`
*removes* `HOME` for the duration of a scratch-config turn, and four tests
take those turns. Process-global environment, tests as threads, so the
reader saw `HOME` mid-deletion and built two icons instead of four.

Everything needed to prevent it was already in the tree:

| already present | evidence |
|---|---|
| the diagnosis | an entry named `TD-C-A-TEST-LOCK-SERIALISES-WRITERS-AGAINST-EACH-OTHER-BUT-NOT-AGAINST-READERS` |
| the tool | `config_turn()`, built for exactly this |
| the precedent | 23 of 27 `oswindow` tests converted, and four sites in the *same crate's* `session/tests.rs` |

`icons.rs` simply was not among them. The knowledge, the mechanism and the
worked example were all in the building, and the bug survived anyway --
because the population a fix is applied to is *the sites where it was seen
to fail*, and the population at risk is *the sites that can fail*. Those are
different sets, and nothing in a green tree distinguishes them. It is the
corpus problem of dd-942 pointed at a remedy instead of at a verdict.

**And their second observation is the volume costume again, wearing green.**
2,843 tests passed after the fix -- and 2,843 passed *before* it, four times
the same night. They are claiming the fix structurally (the reader now takes
the lock the writers take) and explicitly **not** on the strength of the
pass, because a flake that reproduces once in five runs is not disproved by
one green run. A large passing count is exactly as unconditional as 2,114
failing ones: both are impressive numbers that answer a question nobody
asked.

**Lane C's one-line statement of the whole entry, which is better than its
title:** *the outcome carries no information about the mechanism, and we
keep reading outcomes as if it did.* 2,114 errors, 2,843 passes, a green
run over an unfixed race, and -- the one that stung -- a fix of mine that
satisfied a rule I had never read. **A test that passes for the wrong reason
is indistinguishable from one that passes for the right one, right up until
the reason matters.** Passing by accident is worse than failing, because
failing would have sent me to the rule.

**And the timing refinement has two forms, because the good fix is not
always available.** Where a second clock exists, compare against it: two
clocks stall together, so no host pause can separate them. Where it does
not -- lane C's `rssreader` parser test measures its own work, with nothing
running alongside it to compare to -- rule 1 is unavailable and the ceiling
simply has to clear the largest stall the host has been observed to take.
They raised theirs from 1s to 30s on that reasoning, which still catches the
non-termination the test exists for.

Their instance is worth recording for how it was justified: the 1-second
ceiling carried a comment calling it *"orders of magnitude of slack over the
closed form"* -- the ratio argument, stated confidently, in the one place
the ratio argument does not hold.

**Two more from lane C, both operational rather than epistemic.**

*Rewrite the call sites before adding the definition.* They inserted a
guarded helper and then textually rewrote the calls it was meant to
replace -- so the rewrite caught the helper's own body and made it recurse
into itself. `warning: function cannot return without recursing` found it in
a minute. **To a textual replace, a definition added first is
indistinguishable from a call site**, which is the same tool-operates-on-text /
question-is-about-structure shape as everything above. Checked my own staged
applier against it rather than assuming: `build/add-contention-notice.py`
inserts a shell helper and then patches two call sites, and its anchors
(`GATES_START_EPOCH=...`, the QEMU echo) appear nowhere in the helper text,
so insert-then-patch is safe *here* -- verified by simulating both steps
and counting matches, not by reasoning about it.

**The eleventh costume has no tool in it, and it sharpens with experience.**
Reading a four-line failure cluster, I took the last line -- *"persistent
userspace netstack startup failed"* -- for the cause, when the first line
(`listen failed`) was the cause and the last was the report of the cascade.
The log was correct, complete and in causal order. **The error was entirely
in the reading**, and the thing that produced it -- a prior model of how
these failures usually go -- is the same asset that makes someone able to
read a 45,000-line log at all. Lane C's observation, which is why this is
its own row rather than folded in: every other costume is a *tool*
answering a different question than the one asked; this one has no tool,
and **a novice with no expectations would have read line one first.**

The mitigation is mechanical rather than attitudinal, which is the only
kind that survives being tired: read a failure log in timestamp order and
state the first anomaly before reading the rest, because a summary line is
the *last* thing that happened and reads like a conclusion. Built into
`build/scan-guest-output.py` rather than resolved to remember.

And building it produced the same error immediately: the first version
printed the globally-earliest hit as "read this before anything below",
which implied a netstack cascade was downstream of a `getcwd` error 24,000
lines earlier. It now clusters by proximity and implies nothing across
clusters -- and its docstring states that it cannot see kernel-prefixed
lines at all, so "first of the cluster" means first of what it can see.

**A Python escape warning is evidence about the escapes Python does not
recognise, and about nothing else.** Lane C's, and it is a new mechanism
rather than a fifth costume, which is why it is here after I stopped adding
instances. Their heredoc collapsed a backslash and produced two damaged
escapes at once:

| escape | Python's view | outcome |
|---|---|---|
| the invalid one (`w`) | unrecognised | **SyntaxWarning, naming it precisely** |
| the valid one (`a`) | recognised | **silently became BEL, naming nothing** |

So the diagnostic named the harmless one *because* it is the one Python
cannot handle, and said nothing about the one that put a control byte in a
tracked file. A clean run -- or a warning about some other escape -- is not
evidence that the escapes you cared about survived.

**I had one, in the comment explaining a filter that did not work.**
`build/scan-guest-output.py` held a literal 0x08 where `fail` followed by
backslash-b was meant, the regex word boundary I was documenting. The
collapse corrupted the sentence describing the collapse -- the third time in
one day that a paragraph about a byte acquired the byte (my NUL, lane C's
CRLF, lane C's BEL).

**And the way I found it is the part worth keeping.** My detection loop
searched for the two-character sequence backslash-b, written in a heredoc as
an escaped backslash -- which collapsed, so it searched for the *byte* and
found the byte. Had the heredoc behaved, the search would have found nothing
and reported the file clean. **The bug in the detector is the only reason
the detector worked**, which is as clear a statement as I will get of why a
green result from an instrument is not evidence about the thing until the
instrument is evidence about itself.

Practical rules, both cheap: build a replacement from `chr()` rather than
from an escape when the subject *is* an escape; and assert the absence of
**all** control bytes after an edit rather than the one you were thinking
about, because the collapse that produced one can produce another you did
not look for. All six tracked shared documents were verified clean by
byte-class scan, not by eye -- a control byte is invisible in every viewer,
which is the whole reason the check has to be a count.

**Check whether the remedy already exists, before proposing one.** Three
times on 2026-09-18 the thing I was about to build or file was already
there:

| I was about to | it already existed as |
|---|---|
| file a kshell UTF-8 limitation as untracked | `TD-KSHELL-LINE-EDITOR-IS-UTF8`, logged 2026-08-13 |
| add a bounded retry to `submit_and_reap` as a new idea | the same loop, stated with its rationale eight times elsewhere in that file |
| suggest lane C assert `ALL.len() == variant_count` | `scripts/check-variant-lists.py`, **which lane C wrote**, already checking 81 lists across `apps`/`gui`/`net*`/`pkg` |

The third is the instructive one, and it is a new wrinkle rather than more
of the same: **the person who described the problem as unenforced was the
person who had enforced it.** Lane C asked how to stop `Command::ALL`
drifting from its enum; their own script had already checked it that morning
and reported it in step, including the variant they had just added. So
"does a gate exist" is not answered by asking the author.

It also killed my proposed fallback on evidence I would not have found
otherwise. `assert!(Foo::ALL.len() == core::mem::variant_count::<Foo>())`
is `error[E0658]: use of unstable library feature 'variant_count'` on this
toolchain -- measured and dated in that script's docstring, which exists
*because* the in-language assertion could not be written. A suggestion
refuted by the very artefact that replaced it.

And the script carries the third-copy problem in its own scope: it checks
only lists **named** `ALL`, `ALL_*` or `EVERY_*`, so a list that should be
total and is called `PRIMARY_COMMANDS` is invisible to it and nothing says
so. The gate's population is defined by what the author called the thing.

**The filter kept the wrong end.** Lane C gated a merge on
`cargo test --workspace ... | grep -E "FAILED|^error|test result: ok" | tail -40`.
It printed forty `test result: ok` lines and no failures, and it **could not
have printed a failure**: a 420-crate workspace emits hundreds of `ok` lines
after any early one, so `tail -40` is guaranteed to show the clean end. The
count was reassuring and unrelated to the question, and the pipe discarded
cargo's exit status on top -- so the evidence and the verdict were both gone.
The fix is not a wider filter: it is `cargo test > log 2>&1; RC=$?` on its
own line, then search the file.

My version of it was `cargo clippy | grep ... | head -20`, where `head -20`
could have cut a real error and the grep's `error:` pattern matched
`crate::error::KernelResult`. Audited the committed tooling afterwards rather
than assuming: `build/chain.sh` and `build/sweep.sh` put no filter between a
command and its verdict -- full output appended, `RC` captured on its own
line, the gate a `grep -q "^..._RC=0"` against that line. The convention
held, which I know because I looked.

**Never infer a verdict from a log's emptiness; infer it only from a line the
run wrote on purpose.** Lane C's sharpening of the boot-log fix below, after
they read a 0-byte task output as "still running" and happened to be right.
Truncating at start is what makes emptiness *safe* -- absence rather than a
stale wrong answer -- but emptiness is still not evidence. The positive
signal is the `BOOT_RC=` line the chain writes deliberately.

**And the sh/Python boundary, which is not symmetric.** `sh` reads a script
incrementally and keeps a byte offset, so inserting lines above the current
position shifts everything below it and the shell resumes mid-token -- which
is why editing `chain.sh` while it ran was luck rather than safety. Python
compiles a whole module before executing any of it, so editing a running
`.py` cannot affect the run in progress. So "put the change in a script"
buys safety in Python that it does not buy in `sh`, and for shell the rule
has to be the stronger one: **never edit a script that is executing; put the
change in a new file.**

**A proxy should report when it has stopped being one.** Lane C's cure, and
the most useful thing in dd-953 that is not a diagnosis. Their
`frozen-flag-survey.py` is a proxy engine by construction -- *no assignment
to this field by name* standing in for *the user cannot change this* -- and
it was wrong three ways in one week, each silently:

| failure | why nothing said so |
|---|---|
| scope regex wanted `impl App for X`; every app writes `impl oswindow::app::App for X` | it matched none and **fell back to scanning whole crates** without announcing the fallback |
| `*self = Self::new(..)` writes every field | no by-name search can see it |
| `.field = Type { .. }` from outside | same |

Two-thirds of one headline number was furniture. **The fix was not a better
regex.** It was making the tool print the count of what it could not scope,
so the proxy announces the moment it stops being one.

**Applied to my own instrument within the hour, and it corrected a claim I
had made.** `build/scan-guest-output.py` had always *stated* that it cannot
see kernel-prefixed lines. Made to count them:

```
population: 47626 lines, of which 44022 are kernel-prefixed and INVISIBLE
to this scanner by design; 109 unprefixed lines were eligible.
```

**It examines 109 lines out of 47,626 -- 0.23%.** In prose that limitation
read as a caveat; as a number it is the whole story. And it revises what its
verdicts were worth: when I wrote that a boot log was "otherwise clean", the
population that covered was 109 lines, not a boot. The intermittent `listen`
failure found the same day is a `[netsock]`-prefixed line -- **invisible to
this scanner** -- and was found by grepping the log directly, which at the
time I did not register as evidence about the tool.

**And counting one limit is worse than counting none, because it looks
finished.** Lane C's, one message later, and it caught me mid-mistake. They
had been printing an unscoped-crate count -- 6 crates -- and read that as
having discharged the obligation. It was the smallest of four blind spots;
the largest set aside 387 types. *"A tool that counts one of its limits looks
like a tool that counts its limits."*

I had just done exactly that. My scanner counted the kernel-prefix rule
(46,370 lines) and printed the post-filter total as what it examines. Its
`DROP` has seven clauses and a separate error-shape test after them. Counting
all of them:

| stage | lines |
|---|---|
| in the log | 47,626 |
| survived every `DROP` rule | 109 |
| of those, not error-shaped, never considered | 103 |
| **actually judged** | **6** |

**0.013%, not the 0.23% I reported an hour earlier** -- and my own figure was
the overstatement, by eighteen times. "109 eligible" is not "109 examined",
and I published the first as the second.

The scanner now prints the whole ladder, iterating the *same* rule list it
filters with rather than a second hand-written copy, which would be the
third-copy problem above. It also states that the rules **overlap** -- a
blank line is also indented -- so the column is not read as a partition when
only `survived` and `judged` are.

Two orders of correction in two messages, on the same instrument, from the
same rule: state the limit, then count it, then count *all* of them. Each
step revealed the previous one had been satisfied too cheaply.

So the general form is stronger than "document your limits": a stated limit
cannot be weighed, and a reader has no way to tell 30 excluded lines from
30,000. **A limit that is not counted is a limit nobody can price.**

**The checker is a third copy** -- lane C's, and the strongest of the
operational ones. Five of their apps print a list of their keyboard
shortcuts on screen. That printed list and the key handler are **two copies
of one fact**, and they drift: `apps/rssreader` shipped an overlay of
twenty-one shortcuts of which about four worked. The cure is a test that
reads both -- but four of the five apps that *had* such a test wrote it by
naming the keys itself, **a third list**, drifting from the other two and
catching neither. Each of the four had independently written the same forty
lines of `"Left" => Key::Left`.

So the rule is: if the list can be derived, derive it; if it cannot, make
the test read **the artifact**, never a list beside it. A checker that
restates the fact it is checking has joined the population it was meant to
police.

They also corrected a weaker fallback I had proposed for the same problem
(asserting `ALL.len() == variant count`): it catches a *missing* variant and
not a duplicated or wrong one, so `[A, B, B, D]` passes at length four. Worth
recording because it is a guard that looks total and is not -- the same
genus as a ratio bound that assumes proportional noise.

**And the mention-versus-use trap has a favourite disguise: the comment you
wrote explaining the fix.** My freeze-fix guard counted occurrences of
`1_000_000_000` and fired because the replacement's own comment quotes the
old constant. Lane C did it twice: documenting in a source file that
`Command::new`/`spawn(`/`exec(` *"appear zero times"* made that file match a
search for all three, and they reintroduced a lone CRLF into
`known-issues.md` **inside the paragraph documenting the CRLF bug**. The
structural defence is to assert the count *before* replacing and abort
rather than write, which is what their
`def sub(t, old, new, n=1): assert t.count(old) == n; ...` does -- it stopped
three wrong edits in one evening, one where an anchor matched three times and
a plain `str.replace` would have silently edited the wrong one.

*(Audited my own 58 appliers against that: 54 pair every `.replace` with a
count assertion. The four that do not are `.replace("_", "")` on a string,
`replace(microsecond=0)` on a datetime, `.replace(b"\x00", b"")` stripping
NULs, and one already-applied script -- none of them anchored edits. The
convention held, which I only know because I checked it rather than
assuming I had followed it.)*

*A partial fix must say it is partial.* They swept five crates for the
environment race and deliberately left `apps/explorer` half done: the path
that actually reproduced now holds the lock, while 35 further sites in five
modules bypass that helper and remain exposed. Rather than either doing a
38-site mechanical change in the minutes before a merge, or quietly
shipping the narrow fix, the entry says **"partly fixed"** and lists the
five files. That is the direct antidote to the fix-coverage row above: a
sweep that stops is fine, and a sweep that stops *silently* leaves a
silence which reads as completion.

A footnote of theirs worth keeping for operational reasons: the bug was
diagnosable at all only because the log was still on disk. The earlier
instance of the same failure is recorded as *unidentified* because that log
was deleted before it was read.

**And the one control that detects the whole family.** Probing how cmake
finds its module tree, my first experiment returned three clean passes and
was entirely invalid: `mktemp -d` had not captured, so every path collapsed
onto the real `/usr/bin/cmake` with its real module tree. Nothing in the
output looked wrong. **The only thing that revealed it was the
expect-failure case passing.** Lane C put it better than I did: the
expect-failure control is the only part of a suite that can tell you the
suite is wired up at all. That is dd-942's rule turned into a construction
rule rather than a warning -- a corpus you cannot see is checked by
including one member whose verdict you already know, and checking it is the
one you expected.
