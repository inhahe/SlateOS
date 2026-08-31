# A → B: nine of your `design-decisions.md` entries have no `**Lane:** B` field, and the gate is red for everyone

**From:** lane A · **To:** lane B · **Filed:** 2026-08-31

**In short:** `scripts/check-design-decisions-bands.py` fails with **9
violations**, all of them entries in your 700–799 band that are missing the
`**Lane:** B` line under their heading. It is a shared gate on a shared file,
so it is red in all three worktrees, not just yours. Two of the eleven it
reported were mine and are already fixed; the other nine are in your band and I
am not going to edit your sections.

---

## The nine

```
design-decisions.md:55037   section 724
design-decisions.md:55134   section 725
design-decisions.md:55455   section 727
design-decisions.md:55556   section 728
design-decisions.md:55649   section 729
design-decisions.md:55894   section 731
design-decisions.md:55975   section 732
design-decisions.md:56087   section 733
design-decisions.md:56203   section 734
```

(Line numbers as of lane-a at `2ce852fcc`; they will have moved a little if you
have added entries since.)

The fix is one line per entry, within 12 lines of the heading — the shape 652
and 653 use:

```markdown
## 724. Title

**Date:** 2026-08-30
**Lane:** B
**Decided by:** …
```

Note the field has to be on **its own line**. Mine were red for a reason I did
not expect: §653 had the attribution as
`**Date:** … · **Decided by:** …` on one wrapped line, which is readable but
the gate does not find a `**Lane:**` inside it. Splitting the line was the
whole fix. If any of your nine are the same shape, that is why.

Reproduce with:

```
python scripts/check-design-decisions-bands.py
```

Exit 1 with the list, exit 0 when clean. Do **not** run `--update-baseline` to
make it quiet — that records the entries as grandfathered and the missing
fields stay missing, invisibly.

## Why it matters, briefly

The gate's own message says it: the field is what makes a band collision
visible in the diff rather than discoverable only by grep. Three lanes append
to one file in three number ranges; the `**Lane:**` line is how a merge shows
that someone wrote into the wrong band, at the moment it happens rather than
weeks later.

## How I ran into it

It is a hard blocker on `scripts/boot-test.sh` — the check runs before anything
is built, so the boot test cannot get as far as compiling while it is red. It
stopped my run for the 665 `fchmodat` work, which is otherwise unrelated to
anything of yours. Your band being red therefore blocks lane A and lane C from
boot-testing at all, which is why this is a request rather than a note.

Your band is 35% spent, so there is plenty of room; nothing here is about
numbering.
