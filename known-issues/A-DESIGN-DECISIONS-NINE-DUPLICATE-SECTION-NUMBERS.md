## `A-DESIGN-DECISIONS-NINE-DUPLICATE-SECTION-NUMBERS` (lane A, 2026-08-29) — ⚠️ RECORDED, NOT FIXED (deliberately); the *recurrence* is fixed

**In short:** `design-decisions.md` numbers its sections, and other files cite
those numbers the way you'd cite a footnote. Nine numbers — §268 through §276 —
each label **two different sections**, and have done for weeks. Three of the
nine (§270, §271, §273) are cited from source comments and from this file in
ways that resolve to *different entries depending on which citation you follow*.
The duplicates are **not being renumbered**, for the reason given below; what is
fixed is that a tenth cannot now be created.

**Found by:** writing `scripts/check-design-decisions-bands.py` (landed
`0c6937afb`). Not by any symptom — that is the point of the entry.

### Why nobody had noticed

`design-decisions.md` uses two heading styles interchangeably:

```
## §270 — A page flip may not change the resolution, and `SETCRTC` is the call that may
## 270. A self-test may skip, but only on a fact it looked up -- and it must say so in the report
```

Every hand-check ever run against this file matched only the second form. That
includes the `^## 62[4-9]\.` grep that caught the §626 collision on 2026-08-27,
and the regex lane A published in
`requests/a-bc-design-decisions-numbering-c-is-right-b-is-withdrawn-and-i-will-gate-the-bands.md`
as the one the gate would use. **That regex sees 201 of the file's 527
headings.** The other 326 were invisible to inspection, so a duplicate spanning
the two styles could not be seen by the only method anyone was using. Every one
of these nine is exactly that: one section-sign heading and one plain heading.

That is also why §626 *was* caught and these were not — §626 was a collision
between two plain headings, the only kind the grep could see.

### The nine

Both members of each pair are real, distinct, still-live decisions.

| № | The `§N —` entry | The `N.` entry |
|---|---|---|
| 268 | virtio-gpu render resource is not a GEM object (line 31572) | the bootable USB image is built by our own Python (line 30819) |
| 269 | three capability types for clock/ports/rlimits (31639) | hrtimer uses a binary min-heap over a slot slab (33722) |
| **270** | a page flip may not change resolution; `SETCRTC` may (31726) | a self-test may skip, but only on a fact it looked up (40071) |
| **271** | a non-contiguous framebuffer exposes no base pointer (34553) | the skip ledger is fixed-capacity and allocation-free (40184) |
| 272 | `/dev` grows subdirectories and a chardev entry type (34619) | an unreadable Path-Z fixture fails the rung (40617) |
| **273** | a self-test the build cannot reach is a build failure (36455) | lockdep records no incoming edge for a `try_lock` (40699) |
| 274 | a self-deadlocking lock panics immediately (36532) | a health check that never passed gets a new question (40787) |
| 275 | a dying thread steps off its page tables first (36613) | a command that can fail to *look* gets three exit statuses (42716) |
| 276 | a CPU hands off the task it just left (36710) | `tr`'s classes are ASCII-only and code-point-ordered (43960) |

### The three that are actually ambiguous today

The other six happen to be cited in only one of their two senses. These three
are cited in **both**, so following a citation can land you on the wrong entry:

- **§270** — `kernel/src/drm/mod.rs:409,665,782`, `kernel/src/drm/atomic.rs:375`,
  `kernel/src/drm/ati/backend.rs:382` and two `requests/` files all mean the
  *page-flip* entry. `kernel/src/syscall/dispatch.rs:3535` ("which is why the
  checker cannot see it") and `known-issues.md:71553,71753,71783` (the
  `fs::selftest::Skips` ledger) all mean the *self-test skip* entry. Twenty-one
  citations, split across two unrelated subsystems.
- **§271** — `known-issues.md:63749` ("an over-wide rectangle now yields a
  visibly…") means the framebuffer entry; `known-issues.md:71784` means the skip
  ledger.
- **§273** — `known-issues.md:64599,64607` (next to
  `scripts/check-self-tests-wired.py`) mean the unreachable-self-test entry;
  `known-issues.md:75712` (lockdep's own self-test violations tallied
  separately) means the `try_lock` entry.

(Those line numbers are of this file *after* the annotation pass described at
the end of this entry, and they will drift again as it grows — they are a
starting point for a grep, not an address.)

### Why they are not being renumbered

This is the same call already made twice in this project, and it is recorded in
`design-decisions.md`'s own header both times: **§217–§220** (four lane-C
entries mis-numbered into lane A's band) and **§626** (lane A vs lane B). The
reasoning holds here and is stronger:

1. **The citations are the only reason the numbers exist.** Renumbering nine
   sections means finding and rewriting ~64 citations, in three lanes' trees.
   Miss one and it becomes a *dangling* reference, which is worse than an
   ambiguous one: an ambiguous citation lands you on one of two real entries and
   you notice; a dangling one lands you nowhere and reads like the document is
   incomplete.
2. **It cannot be done from one lane.** §270's citations are in
   `kernel/src/drm/` (lane A) and `known-issues.md` (shared); §275's are in
   `kernel/src/kshell.rs` (lane A) and this file. §268's are in a lane-C
   request. A renumber would require either a cross-lane edit — forbidden — or
   three coordinated commits landing together, for a cosmetic gain.
3. **A renumber changes what published text means.** Both `main` and all three
   lane branches carry these numbers; any of them may be quoted in a request
   another lane has already read.

So the numbers stay, they are never reissued, and the pairs are recorded here
and in the baseline (`scripts/design-decisions-baseline.json`, which records
each of the nine as legitimately borne by exactly two headings).

### What *is* fixed

`scripts/check-design-decisions-bands.py`, run by `scripts/boot-test.sh` before
the build, matches **both** heading styles, rejects a heading that starts with a
number but parses as neither, and refuses a tenth duplicate. It grandfathers
these nine and nothing else. `scripts/test-check-design-decisions-bands.py`
asserts against the *real* document that both styles are still in use and that
neither is a rounding error — so a future reader "simplifying" the regex back to
the plain form fails the suite rather than silently reintroducing the blindness.

### The general lesson

**A file with two equivalent surface forms has a hidden third state: the things
your tooling can only see in one of them.** Nothing here was a bug in the
document's *rule*; the rule was fine and everyone followed it. The failure was
entirely in the checking, and it was invisible because the checker's blind spot
and the reader's blind spot were the same blind spot — both were the same grep.
The fix that matters is not the gate, it is that the gate's own test suite pins
the *document's* two-styleness as a fact, so the blindness cannot be
reintroduced by someone tidying up.

### Reducing the ambiguity without renumbering — done for lane A's sites

The cheap, lane-local option nobody has to coordinate: make the **citing** site
unambiguous rather than the cited one, so the reader does not need this entry to
know which §270 was meant.

**Done, as its own commit after the gate landed**, for every ambiguous citation
in lane A's tree and in lane A's entries in this file — each now reads
`§271 (*non-contiguous framebuffer*)`, `§273 (*unreachable self-tests*)`,
`§270 (*a self-test may skip, but only on a fact it looked up*)` and so on. Where
the two senses sit close enough together to be confused on one screen, the
annotation names the *other* sense too (`§271 (*the fixed-capacity skip ledger*
— not the framebuffer §271)`), because a disambiguator that only names one sense
still leaves the reader wondering whether there is a second.

Deliberately not a rule and not gated: this is a readability annotation on a
citation, and a gate that demanded one on every `§N` would fire on 500 unambiguous
citations to make nine legible.

**Two are left, and they are lane C's to make**, because they sit inside lane C
entries in this file and lane A does not edit another lane's entries:

| Site | Entry | Which §270 it means |
|---|---|---|
| `known-issues.md:33817` | `TD-COMPOSITOR-CANNOT-CHANGE-MODE` (lane C) | *page flip* |
| `known-issues.md:33907` | `BUG-DRM-PAGE-FLIP-ACCEPTS-A-MISMATCHED-FB` (found by lane C) | *page flip* |

Both are one-word edits — ``§270 (page flip)`` — and neither blocks anything;
they are listed so the count is honest rather than because they are urgent.
