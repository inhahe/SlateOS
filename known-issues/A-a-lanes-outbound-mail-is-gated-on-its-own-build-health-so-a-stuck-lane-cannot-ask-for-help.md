### [A] A lane's outbound mail is gated on its own build health, so a stuck lane cannot ask for help -- 2026-09-21
**Status:** OPEN (structural, in `roadmap.md`'s three-lane protocol; found by lane C, confirmed here at 36 undelivered commits)

**In short:** when one of the three parallel sessions gets stuck, the way
it asks the others for help stops working -- *because* it is stuck. The
messages pile up unsent exactly when they are most needed.

**How it happens, in three rules that are each individually right:**

| rule | where |
|---|---|
| cross-lane asks go in `requests/`, which is a file on a branch | roadmap.md, three-lane protocol |
| a lane's files are invisible to other lanes until merged to `main` | roadmap.md hazard 1, which cost `a-b-init-conflates-...` a day |
| merge to `main` only after a green boot | CLAUDE.md, "never merge a red tree" |

Compose them and the delivery of a lane's mail is **coupled to the health
of its build**. Lane A's green gate is a ~90 minute boot that has been red
for six rounds on `ctest-coreutils-runs` -- a fixture lane A does not own.
Measured today: **36 commits on `origin/lane-a` and not on `origin/main`**,
including `requests/a-b-execl-fails-where-execv-succeeds-in-the-same-
boot.md`, filed specifically to get another lane's eyes on the thing
keeping the tree red.

**The shape is worth naming because it is self-reinforcing.** The state
that blocks delivery is the state that generates the most mail: a lane
stuck on something it cannot fix alone files requests. So the queue grows
fastest precisely while the channel is shut, and every day red is another
day of accumulation.

It is also invisible from inside. `git push origin lane-a` succeeds, the
request file exists, `open-requests.py --outgoing` lists it. Nothing says
*nobody can see this*. I filed that request and considered it sent; it
reached lane B only because I separately asked lane C to relay, and lane C
noticed the merge gap while doing so.

**Three responses, in increasing order of commitment:**

| option | cost |
|---|---|
| ask a green lane to relay | works, but only if you think to ask -- and a lane that files-and-moves-on never does |
| cherry-pick the `requests/` file alone onto `main` | a document cannot make `main` red, and it merges none of the other commits. Sidesteps the rule's *purpose* while touching its *letter* |
| decouple: let `requests/**` reach `main` without the boot gate | the real fix, and a protocol change, so it is the operator's |

**The second option is safe for a reason worth stating**: `requests/**`
and the shared `.md` documents are not built by anything. The boot gate
exists to stop unverified *code* reaching a trunk three lanes build from.
A request file has no compilation, no test and no runtime.

**Measured magnitude, from `scripts/merge-readiness.py`** -- which exists for
a related question and answers this one as a side effect:

> CAUTION: your last passing boot ran at `273905c13`, which is **945
> commit(s) behind HEAD**.

945 commits since lane A last had a green boot. That is the size of the
window in which the merge-when-green rule has delivered nothing, and it is
a better argument than any reasoning about the rule: whatever the policy is
meant to protect, it has spent 945 commits not protecting it while also not
delivering.

The same tool settles the other half in lane A's favour: `origin/main` is
13 commits ahead touching 11 files, and **no incoming file is in lane A's
scope**. So a completed run still speaks for this subsystem -- the code it
asserted behaviour about is byte-identical after the merge. Two shared
`scripts/` files changed, which it says are *"cheap to settle: re-run the
gates, not the boot test."*

**That narrows the open question usefully.** It is not "may lane A merge
untested code" -- nothing incoming touches lane A, and the gates pass. It
is: *may a lane merge when the only red rungs are another lane's fixtures,
already present on `main`, failing identically before and after?* Posed
that way it is answerable, and the answer might still be no.

**Recorded rather than acted on unilaterally**, because it is a rule in
`roadmap.md` governing all three lanes and changing it is not lane A's to
decide. Promoted to `open-questions.md` if it survives one more day red.
