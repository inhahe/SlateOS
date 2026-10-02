## 755. A gate's mutation table is checked on every build; the mutation sweep that fills it is not — because the sweep costs nine minutes and the check costs one second

**Date:** 2026-09-03
**Decided by:** Claude (autonomous)
**Lane:** B

**In short:** Some of the small yes/no programs under `scripts/` contain guards
that do nothing at all — a limit that no code ever reads, a test whose fixtures
are computed from the very number they are supposed to be testing. A guard like
that is spelled exactly like one that works, and the whole test suite stays
green either way. The only way to tell them apart is to *break the guard on
purpose* and check that something complains; each gate now carries a list of
breakages to try. The decision is about who runs that list. Breaking things and
re-running the suite takes about nine and a half minutes, so it stays a thing a
person starts deliberately. What runs on every build is the one-second check
that the list still refers to lines that exist — because the list is written as
quotations of the gate's own source, and a rename turns a quotation into a line
that matches nothing, gets silently skipped, and leaves a row that reads as
coverage.

### The defect this is all in aid of, three times over

| Gate | The decoration | How it was found |
|---|---|---|
| `check-libc-shape.py` | `MIN_MEMBERS`/`MIN_SYMBOLS` were pinned against a measured archive and `main()` never read them; the only assertion about them was `MIN_MEMBERS > 0`, which a gutted constant passes | gutting them by hand |
| `check-doc-links.py` | a five-part coverage floor whose fixtures were derived from the constants under test, so zeroing three of them shrank the fixtures to match | gutting them by hand |
| `check-doc-links.py` | a subset-vs-whole-tree branch that no case ever drove, and a counter nothing ever asked for a real number | the first sweep: nine of seventeen mutants survived a 74/74 green suite |

The third row is the argument in miniature. Seventy-four passing cases, and
nine separate assertions that could be deleted without any of them noticing.

### Why the sweep cannot be the thing that runs every time

Not safety — that was solved separately (§ below). Cost. The sweep is one
subprocess per row and each subprocess runs the target gate's entire suite.
Measured 2026-09-03 on a host busy with a boot test: **9m38s** for three
tables, against **24s** for the static check on the same host. Worse, it is the
wrong shape of cost: it grows linearly with the number of rows, so the price of
the gate that catches a survivor rises exactly in proportion to how thoroughly
anyone tests. A boot test that already spends forty minutes on gates cannot
take another ten, and a gate people are tempted to skip is worse than no gate,
because it turns "green" into "green, probably, mostly".

### What is lost, stated plainly

The wired check cannot tell you a mutant *survived*. It can only tell you the
question stopped being asked. That is the weaker of the two facts — but it is
the one whose failure is silent, and a stale needle is not merely a missed
detection: it is a row that goes on reading as coverage after it has stopped
being any. A survivor, by contrast, is discovered the moment anyone runs the
sweep, which the tooling now makes cheap to do.

### The alternatives, and why not

**Wire the sweep and accept ten minutes.** Rejected on the measurement above,
and on the shape of the growth rather than today's figure.

**Sweep on a schedule (nightly, or on merge to `main`).** There is no scheduler
in this project; the boot test *is* the schedule. A cron job nobody has written
is not a plan.

**Sweep only the gates whose files changed in the commit.** Attractive, and
genuinely cheaper — but wrong for the failure it targets. A needle goes stale
because the gate was reworded, so a changed-file sweep would cover exactly that
case; but a *survivor* appears when a test stops covering code that did not
change, and that is the case it would miss. Half the value at most of the
complexity, and the half it keeps is the half the static check already has.

**Keep the throwaway scripts.** This is what happened the first two times: one
ad-hoc mutation script per gate, deleted after use. `known-issues.md` lesson 63
— a rule kept only by copying is a rule that will be dropped — and it was
dropped, twice, which is why the same defect class recurred.

### Two supporting calls, made the same day

**The table lives inside the gate it breaks, not beside it.** A needle is a
quotation, and a quotation kept in another file goes stale the moment the line
it quotes is reworded, with no diff connecting the two. Co-located, the rename
that breaks the needle is in the same diff as the needle. The cost is that
every needle then matches twice — once in the code, once in its own row — which
is cut out exactly by parsing the assignment's extent with `ast` rather than by
asking authors to remember an invisible anchoring convention.

**Mutants are written beside the gate, never over it.** The first version
mutated in place and restored in a `finally`, which is correct until something
skips the `finally`: a SIGKILL leaves a broken checker on disk and the next
sweep judges mutants against it, all of which look normal. Two sweeps at once
is the same failure by another route, and it actually happened during
development. Writing `.mutant-<pid>-<gate>` next to the gate removes the
failure mode instead of detecting it. It must be a *sibling*, not a temp file
elsewhere, because a gate locates the tree relative to its own path.
