## TD-B-DIFF-IS-THE-FIRST-PAIR-THE-STANDALONE-WINS (lane B, 2026-09-11) — ✅ RESOLVED (lane B; confirmed 2026-09-27)

**Status (2026-09-27):** resolved the way this entry said it should be: the
standalone crate is gone, and `userspace/coreutils/src/bin/diff.rs` is the one
`diff`, rebuilt on diffutils 3.10's command line and measured at 200 cases
agreeing, none differing (`scripts/diff-diff.sh`). Kept below as the record.

**Sixteen pairs have now been measured against a harness and fifteen went to
`coreutils`. `diff` is the first that does not**, and it is the pair §1005
anticipated when it said "for about half of them the standalone crate is the
substantially larger implementation and `coreutils`'s namesake is a stub".

`bash scripts/diff-diff.sh`, 107 cases against GNU diffutils 3.10:

| half | passed | differed |
|---|---|---|
| `coreutils` | 21 | **86** |
| standalone | **43** | 64 |

Twice as good, and still failing 64. Neither half is close to GNU, which is why
this is filed rather than acted on.

**What `coreutils` fails at, and it is not subtle:** 52 of its 86 are
`diff: requires exactly two files`. It has no option parsing beyond `-q` and
`-u`, so **every other option is read as a third file operand** — `-s`, `-c`,
`-i`, `-w`, `-y`, `-r`, `-B`, `-a` all produce that one sentence. A further 13
are that it cannot read stdin: `diff base.txt -` is
`-: No such file or directory (os error 2)`.

**What the standalone fails at:** 42 are output-format differences, 14 are the
numeric context forms `-U N` and `-C N` (it has `--unified` and `--context` but
not the counted spellings), and 8 are the newline defect above.

**The decision this sets up, and why it is not taken here.** §1005 says
`coreutils` is the one home and the better half survives *inside* it. For every
pair so far that meant deleting the standalone. Here it means the opposite:
porting the standalone's implementation into `coreutils/src/bin/diff.rs`, then
deleting the crate. That is real work rather than a deletion, it should fix the
newline defect on the way in rather than carry it across, and it wants doing
deliberately — so it is recorded here with the harness that will judge it.
