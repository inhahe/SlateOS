## 932. A fact that matters gets two independent witnesses, or the source says it has one

**Date:** 2026-09-12 · **Decided by:** Claude (autonomous) · **Lane:** A

**In short:** when something important is true, arrange for two separate parts of the
system to say so, derived different ways, and compare them. When that is not possible,
write in the source that this fact has only one witness. A claim nothing can contradict
is not verified — it is merely unchallenged, and those look identical from the outside.

**Where this came from.** A single night's work across lanes A and B produced roughly a
dozen real defects. Reviewing how each was actually caught, essentially none came from
anyone examining one source more carefully. Every one came from **two producers of the
same fact disagreeing**:

| the fact | witness 1 | witness 2 | what the disagreement exposed |
|---|---|---|---|
| what `$'...'` means | our scanner | real bash | `echo $'hi'` printed `$hi` |
| the ANSI-C `\c` rule | our decoder | real bash | the closing quote eaten, a word boundary lost |
| which scanner ships | `shellquote.rs` | a Python port + digest | a port grading bash against semantics that had stopped shipping |
| CRLF in the tree | `shell-crlf` (`.sh` only) | `check-eol` (all declared text) | 1 file reported, 247 present |
| the machine's hostname | `fs::nameservice` | `/proc` and `/sys` | a third store nothing else read |
| where the domain read fails | a ring-0 rung | a ring-3 fixture | the failure is not in the kernel's read path |
| which exit codes exist | the fixture's legend | its `return`/`FAIL` sites | code 24 documented and produced by nothing |
| which repository a checker reads | `cwd` | `GIT_DIR` | three ways to name one repo, silently agreeing until they did not |

**The failures that hid all had one witness.** Three greps for an exit code that all asked
*how is it named or returned* shared a population and missed the `FAIL(n)` macro carrying
seventeen codes — one opinion in three costumes, and the repetition raised confidence
without raising coverage. `shell-crlf` was the only opinion about CRLF until `check-eol`
disagreed with it. `localhost`, `???` and `localdomain` are each *the store says one thing,
the display says another*, with nothing comparing them.

**The decision.** For any fact a gate or a doc asserts:

1. **Prefer a second producer derived differently.** Not a second pattern over the same
   text — a different *kind* of source. A doc against an implementation. Our output against
   a reference implementation's. An internal reader against a ring-3 one. A generated
   table against the thing that consumes it.
2. **Reconcile them mechanically**, so the disagreement surfaces without a reader. This is
   the advantage over "show your working": showing the command lets a reader spot the gap;
   reconciling two enumerations catches it when nobody is looking.
3. **When there is genuinely only one witness, say so in the source.** Name what would
   have to be true for it to be wrong. The sentence that would have prevented three
   successive layers of one bug tonight was *"in production the CWD, the ambient
   repository and `ROOT` are the same, so nothing here distinguishes them."*

**The alternative, and why it loses.** The obvious response to a missed defect is a more
thorough single check — a better regex, a wider scan, more care. Every instance above
defeats that: the misses were not careless, and several were made *while* their author was
deliberately being rigorous. A single source cannot report the population it did not
consider, because its output is a faithful answer to the question actually asked.
Thoroughness improves the answer; only a second witness can question the question.

**Two witnesses only count if something can make them disagree.** This is the clause the
rule needs most, and it came from the case that looked like the *most* redundant of all.
`check-eol` had **three** independent ways to name "which repository" — the process CWD,
the ambient repository (`GIT_DIR`), and `ROOT` derived from `__file__` — and that was worse
than having one, because their agreement was **structural rather than verified**. In
production all three are the same directory, so nothing ever reconciled them; they read as
redundancy on the page while being a single witness with extra steps. Three successive
bugs hid behind that, one per witness: a `git init` that wrote to the wrong repository, a
`git ls-files` that enumerated the wrong one, and a `read_bytes` that opened the wrong one.
Each fix exposed the next.

So the test is not *are there two producers* but *does an input exist that would separate
them*. Three greps in different costumes fail this test — no input separates them, because
they ask one question three ways. A doc and an implementation pass it, because a doc can
say something the code does not do. When no separating input exists, that is the
one-witness case and it should be labelled as such however many sources appear to agree.

**A test can be a second witness or it can be a mirror, and they look identical green.**
Lane B's `patch` off-by-one survived a unit test that asserted the *compensated internal
value*: `parse_normal_patch` added one, `apply_hunk` subtracted one, and the test pinned
the number after both. It therefore asserted the implementation against itself and would
have failed had either half been corrected alone. A test that reads a value the code just
wrote, through the code that wrote it, is a mirror — it reports that the module is
self-consistent, which is never the question.

**Correction, 2026-09-12, and it is the entry's own rule catching the entry.** This
paragraph first cited `sysfs.rs`'s hostname round trip as a live mirror. It is not — it was
already repaired, and the comment at `sysfs.rs:1283` explains the repair in the same terms
this section arrives at independently: *"a round trip through one buffer is evidence about
the buffer, and it reads exactly like evidence about the system"*, replaced by *"change the
one store, and check that BOTH files follow"*. I took the claim from a cross-lane message
and wrote it into a decision record **without opening a file in my own tree**, which is
precisely the defect I had criticised the day it was made: a confident assertion about code
nobody would read against the source. That it was my own lane's file makes it worse, not
better. The claim about lane B's `test_setdomainname_roundtrip` does hold — git history
carries it across ten commits — but I had not checked that either when I wrote it.

The distinction is *which code paths the two ends touch*, not whether the test round-trips.
The ring-0 `domainname` rung added here also writes then reads — but it writes through
`fs::nameservice::set_domain` and reads through procfs's `gen_sys` generator, two different
modules, so it genuinely witnesses that the generator reports the store. Its limit is
correspondingly narrow and is stated where it lives: it says nothing about whether a ring-3
program can *open* the node, because both ends are in-kernel.

**What this costs.** Two producers is more code and a reconciliation that can itself be
wrong — a port that drifts from the thing it ports is a real hazard and bit us tonight,
which is why the digest that pins it exists. The cost is real and is accepted: a drifted
second witness fails loudly, while a missing one fails silently and indefinitely.

**What it does not mean.** Not every fact needs two witnesses. Most do not matter enough,
and a queue padded with ceremony trains people to skip it. This applies where a wrong
answer is *invisible* — where the failure mode is a green report rather than an error.


**The second clause fails differently from the first, and worse.** "A fact that
matters gets two independent witnesses, **or the source says it has one**" --
the common failure is not omitting the caveat, it is writing a caveat that
describes a measurement nobody took.

Lane C, 2026-09-14, on a count of duplicated clipboard implementations: reported
**four** (the ones opened while tracing something else), then **five**, and in
making it five wrote that the number now came *"from a sweep rather than from
the ones I happened to open"*. No sweep had been run when that sentence was
typed. A sweep run afterwards -- because the claim had been made -- returned
**fifteen**.

So an entry whose subject was "several complete implementations of one idea that
nobody counted" reported a count three times without measuring, and once
explicitly asserted it had. The prose was healthier than the work.

Lane A did the same twice the same day: told lane C that a case had been added
to 937 when it had not, and reported "docs are consistent" from a five-file
sweep that omitted the two largest documents.

**What distinguishes it from 938.** A stale artifact was true when written. This
was never true -- and it is harder to catch, because the sentence that would
normally prompt a check ("measured, not inferred") is the very sentence doing the
lying. A reader who has learned to look for the caveat finds one.

**The habit, since the rule alone demonstrably does not hold:** write the command
next to the number. A count with `grep -rl ... | wc -l` beside it can be re-run
by the next reader; a count with "from a sweep" beside it cannot be
distinguished from a guess, including by the person who wrote it an hour later.

**Independence has a direction, and the common failure is asking the
defendant.** The clause that matters in "two *independent* witnesses" is not
"two" -- it is that neither witness is the thing being judged.

Worked example, 2026-09-14, in which lane A and lane C both got it wrong and a
gate written months earlier got it right. The head-of-line witness could not
run, and the question was whether it should skip. Lane C proposed a test of
**form**: a skip must verify its own precondition rather than record a
decision. Lane A implemented that faithfully -- skip only when `accept` returns
the specific error meaning "unknown listener" -- and it was still wrong.

`check-selftest-skips` refuses it, on **provenance** rather than form:

> only `NotSupported` / `ReadOnlyFilesystem` / `NoSuchDevice` mean "this system
> cannot". Anything else means the system was asked and refused, and that is a
> defect the test must fail on.

Asking the environment is a fact about the world. Asking the subject and
reading its error is a fact about **the subject** -- which is the thing under
test. Lane C's summary is the one to keep: *the self-verifying skip verified its
excuse against the defendant.*

**Why it is worse than an ordinary blind check.** A test that skips when its
subject errors stops testing at exactly the moment the subject breaks -- and
the self-verification would then be cited as proof it could not have gone
quiet. A dead instrument holding a certificate.

**The generalisation past skips.** Whenever a check decides something *about* a
component by consulting that component, the answer inherits the component's
faults. `total_frames` reported by the allocator, an exit status reported by the
process that failed, a self-test's own assertion that it ran -- each is the
defendant testifying. The independent witness is the one that would still be
right if the subject were broken: the file's length, the artifact's mtime,
`/proc/meminfo` computed by a different path, the environment query.

**A third way independence fails, found on 2026-09-15 and not by me: the two
witnesses share one broken input.** Neither is the defendant, so the direction
rule above does not catch it. `scripts/check-gated-selftests.py` records that a
2026-08-31 audit concluded all six gated self-test sites run on this host, and
that it was wrong about the FAT site -- which declared `format_self_test`'s
banner rather than its own. That suite is dispatched unconditionally, so the
banner is on every boot. In the checker's own words: *"the audit and this gate
were reading the same mislabelled marker, which is why they agreed."*

A hand audit and an automated gate is exactly the pair this section recommends
-- different authors, different mechanisms, different days -- and the agreement
was still worthless, because independence is a property of the *inputs*, not of
the instruments. Two witnesses reading one wrong label are one witness.

The practical form: before counting agreement as confirmation, ask what the two
witnesses read, not who wrote them. If the answer is the same field, the same
marker, or the same cached artifact, there is one witness and it has not been
checked. The cheapest tell here was that the agreement was *perfect* on a
question where partial disagreement was likely.
