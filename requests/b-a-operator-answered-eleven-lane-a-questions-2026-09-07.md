# B → A: the operator answered eleven of your open questions. Here they are verbatim.

**From:** lane B · **To:** lane A · **Filed:** 2026-09-07

**In short:** the operator sent a batch of answers to me (lane B) covering all
three lanes' questions in one message. Eleven of them are yours. I am not
writing in your region of `open-questions.md` or `design-decisions.md`, so
they are reproduced here **verbatim** for you to record — write each up in
`design-decisions.md` in your own band, delete the entry from
`open-questions.md`, and index it under `## Resolved — lane A`.

Where the operator asked a question *back*, I have flagged it. Those are not
answered — they need your reply, and one of them (Q46) is an explicit
instruction to design something better than what the entry offered.

---

## Q46 — release-mode boot test

> **C, and do something about '"periodic" needs a trigger nobody has defined;
> in practice it tends to mean "never"'. Make a solution that will not result
> in "never" in practice.**

The second sentence is an instruction, not a preference: option C's weak point
was that it relied on a "periodic" re-measurement with no defined trigger, and
the operator wants that replaced with something that actually fires. A
trigger that cannot be forgotten is one attached to an event the tree already
produces — a commit count, a gate run, a boot-test invocation, a release tag —
rather than to a human's intention to remember. State it in the decision.

## Q47 — shared build-output directory

> **I don't know, but regarding the problem of slowing down due to only one
> lane being able to build at a time, I don't know if that's really an issue
> because I think two builds going at a time would each take twice as long
> anyway due to CPU and filesystem I/O saturation? But also, the project was
> moved to E: since that question was made, which has about 300gb free. But
> I'd rather not use 120gb of space unnecessarily anyway. Oh, and as for
> pruning, I already had a session evaluate what can be regularly pruned
> that's actually dead without slowing down any future rebuilds, and it seemed
> to say relatively little could be spared.**

Three facts that change the entry's cost table: (1) the serialisation cost may
be near zero because concurrent builds saturate the same CPU and I/O anyway —
worth measuring rather than assuming; (2) the tree is now on **E:** with ~300
GB free, so the 0-bytes-free disaster that motivated the question cannot
repeat in the same way; (3) pruning has already been evaluated and buys little.
The operator has not chosen an option; the standing preference is "do not use
120 GB unnecessarily".

## Q56 — Linux programs exempt from our file-permission checks

> **I like A better. You say Linux programs expect ambient authority by
> construction, but if you simply suspend the linux program long enough to ask
> the user for permission, or the user gives it permission ahead of time, then
> that's not an issue, right? It can be done without making any functional
> difference for the Linux program - just warn the user that the Linux program
> may not work correctly without the permission. Also, we could have A but
> allow the user the option to grant all Linux programs the capabilities by
> default. Should native programs also be granted them by default? Also, do we
> have a mechanism to define what native programs (and also Linux programs)
> under a given account are granted by default? Because I think we should.**

**Answer: A**, with the ambient-authority objection answered — suspend the
process at the check and prompt, or let the grant be made ahead of time, which
is functionally invisible to the Linux program. Plus a requested feature: a
per-account default-grant policy for both Linux *and* native programs. The
last three sentences are questions back to you: should native programs get the
same defaults, and does such a mechanism exist? (I do not believe one does;
`posix/` has no per-account capability defaults today.)

## Q57 — may a program prompt for keyboard/mic/camera permission

> **A and fix the error message.**

## A-Q1 — `find . -size 100`

> **C, though I'm concerned that for someone not familiar with the tool, "b"
> would naturally be assumed to mean bytes. But I guess there's nothing we can
> do about that, I don't see a way to do it that's both intuitive and
> compliant. If we disallow 100 and make 100b be bytes, that bites anyone
> who's already familiar with the tool. If we allow 100 and make it be bytes,
> that also bites anyone who's already familiar with the tool. Either could be
> mitigated with warnings printed when those parameters are used, but I think
> that would still annoy Linux users.**

**C**, with the ambiguity acknowledged as unavoidable rather than solved.

## A-Q2 — C-test programs built against an unidentifiable library

> **A**

## A-Q3 — should the kernel run its test suite on a user's boot

> **Is there any advantage in the long run to doing A now, D eventually rather
> than just doing D now? And is the OS currently usable on actual hardware?**

Not an answer — two questions back to you. The second one is a plain factual
question about the current state of bare-metal boot.

## A-Q4 — `oci run` refusing when an option cannot be applied

> **A**

## A-Q5 — the shell's `grep` defaults

> **A, though I have my own grep utility - a Python implementation and a C++
> implementation - under `d:\visual studio projects\grep`. It has some features
> I made that normal grep doesn't, that I like, but it also lacks some features
> that standard grep has, so it'd probably be better to integrate my grep's
> additional features with Slate OS's grep so that it has all the GNU grep
> features plus my additions.**

**A**, plus a new piece of work: read the operator's own grep at
`D:\visual studio projects\grep` (Python and C++ implementations), identify
the features it adds over GNU grep, and fold those into SlateOS's grep so it
is a superset. Note `grep` is a **lane B** file (`userspace/`), so the port
itself is mine — but the answer arrived on your question, so I am recording it
here and will pick the work up on my side.

## A-Q6 — the two destructive-looking commits and 33 fake-signed commits

> **A**

## A-Q7 — 70 ms per file opened

> **d:\visual studio projects was already excluded for the virus scan, so I
> don't know what the 70 ms was. Also, the project was since moved from the HDD
> to an SSD. As for everything being slower, it could be because my CPU is
> often pegged lately, and the filesystem could be under more heavy load too.
> And failing to delete things could be because I have two continuous local
> backup jobs going at all times *and* two continuous cloud backups.**

The entry's leading hypothesis (Defender) is **excluded** — that directory was
already whitelisted. New facts: the tree moved from HDD to SSD, the CPU is
frequently saturated by other work, and there are **four** continuous backup
jobs (two local, two cloud) touching the tree, which is a much better
explanation for both the per-file latency and the delete failures. Worth
re-measuring on E: before drawing conclusions.
