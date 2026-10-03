### TD-OILS-A-REFUSED-MAINTAINED-ASSIGNMENT-KEEPS-THE-SUBSTITUTION-STATUS. An assignment-only command refused by `att_noassign` reports the command substitution's status where bash reports 1 — 2026-08-04 — ✅ RESOLVED 2026-08-04

**Where:** `userspace/oils/src/interp.rs` — `Shell::apply_assignment_inner`'s
`noassign` branch (~11913), which returned without disturbing the status the
command substitution left behind.

**What.** An assignment-only command normally takes its status from the last
command substitution in it (`z=$(exit 3); echo $?` → 3, in both shells). When
one of its assignments is refused because the shell maintains the name, bash
replaces that with 1 — but *only* when there was a substitution to replace: a
plain `FUNCNAME=5` still reports 0, and so does `FUNCNAME=$((1+1))`, which is
an arithmetic expansion rather than a substitution.

```
              FUNCNAME=5   FUNCNAME=$(exit 0)   FUNCNAME=$(exit 3)   FUNCNAME=$((1+1))
    bash          0                1                    1                   0
    osh           0                0                    3                   0
```

It is the whole command that is judged, not the one assignment: `z=$(exit 3)
FUNCNAME=1` and `FUNCNAME=$(exit 3) z=1` are both 1, and `FUNCNAME+=$(exit 3)`
is too.

The rule is not "a refusal replaces the substitution's status", which was the
first reading and is wrong: `FUNCNAME=1 z=$(exit 3)` is **3**. A refusal is
simply one more thing that sets the command's status — to 1 — as the words are
worked through in order, so what answers is whichever came *last*:

| | status |
|---|---|
| `FUNCNAME=1 z=$(exit 3)` | 3 — the substitution came after |
| `z=$(exit 0) FUNCNAME=1` | 1 — the refusal came after |
| `q=$(exit 3) FUNCNAME=1 w=$(exit 0)` | 0 |
| `q=$(exit 0) FUNCNAME=1 w=$(exit 3)` | 3 |
| `FUNCNAME=$(exit 3) GROUPS=1` | 1 |

**Fixed** by recording `comsub_count` at the refusal —
`Shell::assign_refused_maintained: Option<u64>`, set in
`apply_assignment_inner`'s scalar `noassign` branch — and asking, in
`exec_simple`'s assignment-only arm, whether it still equals the count at the
end. Equal means nothing substituted after the last refusal, so the status is
1; unequal means a substitution had the last word; and the existing
"no substitution at all resets to 0" branch is tried first, so `FUNCNAME=5`
stays 0. `exec_simple` saves and restores the field around its assignment loop,
so a refusal inside one of those substitutions stays that command's business.
Corpus: `a-maintained-name-refusing-a-write-sets-the-assignment-commands-status.sh`.

**Impact.** `$?` after an assignment to one of the six `att_noassign` names,
with a command substitution on the right. Found while probing
TD-OILS-A-REFUSED-ASSIGNMENT-STILL-EXPANDS-AND-TRACES.
