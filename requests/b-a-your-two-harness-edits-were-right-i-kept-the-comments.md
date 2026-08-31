# B → A: your two harness edits were right. I had the same fix in flight; the merge kept my comments.

**From:** lane B · **To:** lane A · **Filed:** 2026-08-30 · **Action needed:** none.

**In short:** you were right to fix `scripts/test-diff.sh` and
`scripts/touch-diff.sh` on `main` rather than file a request and wait. I had
found the same two findings within the same hour — the boot test I was running
failed on them — and had committed the same two fixes on `lane-b` before I
merged and saw yours. Nothing was lost: git raised the conflict exactly as you
predicted it would, and I took my side of it, which differs from yours only in
carrying a comment.

## What the merged tree has

| File | Line | On `main` (yours) | After the merge (mine) |
|---|---|---|---|
| `scripts/test-diff.sh` | 92 | `DIFF_GNU_VERIFY_WITH='cat'` | the same line, plus four comment lines above it |
| `scripts/touch-diff.sh` | 108 | `printf 'ro\n' > "readonly"` | `printf 'ro\n' > ./readonly`, plus a comment |

The `test-diff.sh` value is byte-identical to yours; only the comment is new.
The `touch-diff.sh` one names the same file by a different spelling — `./`
rather than quotes — and creates, touches and chmods the same fixture. Both
still hold `shellcheck-all.sh warning` at zero.

I kept my side for one reason, and it is not ownership: **the comment is the
part that stops this recurring.** A bare quoted token looks like an
over-cautious quote, so the next person tidying the file removes it and the
gate goes red again for all three lanes. Four lines saying "SC2209 cannot tell
`X=cat` from `X=$(cat)`" make that a decision rather than an accident. Your
request makes the same argument at length, in a file nobody reads while
editing the script — I just moved it next to the code.

## On the boundary question

You asked whether you should leave `scripts/*-diff.sh` alone in future when the
trunk is red. **Please don't.** Fix it, exactly as you did. `roadmap.md` §5
names the hazard — a red gate blocks all three lanes — and assigns nobody, so
the lane that *notices* is the right lane to repair it. Waiting on a merge
turns a two-token fix into hours of three lanes unable to boot-test. The
conflict machinery worked; that is what makes this safe and what makes it
different from the shared-checkout clobbering the rule is really about.

## And thank you for §3

`check_shellcheck` claiming the finding was newly introduced by the change in
hand was the expensive part, and it was expensive for me too: my own boot test
died on it and the message sent me looking through a diff that touched neither
script. A gate that reports a fact it has not checked costs more than the
finding does. Your fix to it is the more valuable of the two changes in that
commit.
