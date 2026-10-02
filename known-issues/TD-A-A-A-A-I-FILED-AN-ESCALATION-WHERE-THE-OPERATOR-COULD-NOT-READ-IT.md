## TD-A-A-A-A-I-FILED-AN-ESCALATION-WHERE-THE-OPERATOR-COULD-NOT-READ-IT (lane A, 2026-09-11) — **caught by a gate, not by me**

**In short:** I needed the operator to decide something, wrote it up carefully, and
appended it to the end of the file where such questions go. The end of that file is the
*archive* of already-answered ones. So the request for help was filed among the things
that no longer need reading.

`check-open-questions` refused the build and said exactly why: *"an OPEN question is
filed below `# Resolved`, where the body says only answered ones go … The operator reads
the queue from the top, so a question filed there is not a question that was asked."*

### Why this one is worth an entry

Every other instance of this shape I found today was in someone else's work or in a
document: a stale commitment naming a retired script, a benchmark series excused by text
the source no longer reached, a self-test asserting a round trip through one buffer, an
instrument whose only assertion is `> 0`. This one is the same defect in a *message*, and
I produced it in the same hour as writing three of those up. Appending to the end of a
file is the default motion; in a file that is ordered by status, the default motion files
by position and the position means "done".

The gate existing at all is the reason this cost ten minutes instead of however long the
operator would have taken to not see it. Worth noting that the gate's message is a model
of the form: it names the cause, the two other causes it could have been, and the
consequence in terms of who reads what.

### Disposition

The entry was **removed rather than promoted**. It asked whether to start lane C or
authorise me to baseline their module; lane C turned up and wired the module properly, so
the question was answered by events and not by the operator. Moving a dead question into
the live queue would spend the operator's attention on a decision that no longer exists,
which is the failure `deferred-questions.md` was created to prevent.
