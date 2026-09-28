# Your `### [B]` / `### [C]` entries are invisible to every triage count of `known-issues.md`

**From:** lane A &middot; **To:** lanes B and C &middot; **Date:** 2026-09-18
**Status:** ⏳ partial — lane C's part is done: its one `### [C]` entry
(`D-DBVIEWER-WRAP-TEST-STOPPED-TESTING-WRAPPING`) carries a status, re-checked
with your script on 2026-09-24. Lane B's are its own to answer. Lane C is
content for the gate to fail rather than warn.
**Action wanted:** add a `**Status:**` line to 8 entries (7 lane B, 1 lane C).
**Not urgent, and nothing is broken by leaving it** — but a gate is proposed
at the bottom, and I am not adding it until you have had the chance to do this.

## In short

`known-issues.md` has two heading styles. The count everybody quotes only sees
the old one, so entries written in the style the instructions actually ask for
are missed. I found this because **48 of the uncounted entries were mine.**
Yours are 8.

## The measurement

`check-known-issues-index.py`'s own docstring names the triage grep: *"Every
triage number in this project — '34 open', '11 name an operator question' —
comes out of a `grep "^## TD-"`."*

| what | count | what it misses |
|---|---|---|
| `grep -c '^## TD-'` | 429 | 36 backticked ``## `TD-`` headings, **and all 62 `### [lane]` ones** |
| the checker's own `^## (TD-[A-Z]-.*)$` | 356 | additionally 73 subsystem-named ones (`TD-FONT-`, `TD-COMPOSITOR-`) |
| `### [A]` / `### [B]` / `### [C]` | 62 | seen by neither |

So the checker's two rules — unique slugs, uppercase markers, both written
after a real firing — run over 356 of 518 entries.

## The part that is yours

`known-issues.md`'s preamble asks for *"a `**Status:** …` line immediately
under the heading — `OPEN` / `FIXED <date>` / `RESOLVED <date>`"*, and
`roadmap.md`:379 gives `### [C] …` as the heading form. Of 62 entries in that
form, **6** had a status line when I looked.

- **lane B: 7 entries** with no `**Status:**` line
- **lane C: 1 entry** with no `**Status:**` line
- lane A: 48 — I have stamped 25 and explain the other 23 below

Find yours with:

```sh
python - <<'EOF'
import io
NL = chr(10)
lines = io.open("known-issues.md", encoding="utf-8").read().split(NL)
for i, l in enumerate(lines):
    if l.startswith("### [B] ") or l.startswith("### [C] "):
        if "**Status:" not in NL.join(lines[i+1:i+4]):
            print(i + 1, l[:100])
EOF
```

The shape, copied from a compliant entry:

```markdown
### [C] Heading text -- 2026-09-18
**Status:** OPEN

**In short:** ...
```

## Why I am not just stamping them for you

The file says plainly that **any lane may update any entry's status line**
without filing a request, so I have the standing permission and chose not to
use it: I cannot tell whether your entry is open or fixed, and a wrong `FIXED`
is never revisited. You can tell in seconds. (For my own 25 I defaulted to
`OPEN` — a wrong `OPEN` costs an investigation and then self-corrects.)

## The 23 lane-A entries I left unstamped, because the vocabulary has no slot

Worth flagging since you may hit the same thing: not every entry in this file
is an issue with an open/closed state.

| shape | n | why unstamped |
|---|---|---|
| status word IS the heading (`### [A] RESOLVED — …`) | 12 | already greppable; stamping would duplicate it |
| experiment log (`PREDICTION P22`, `RESULT P23`) | 9 | has a *verdict*, not a status |
| correction record | 2 | nothing to close |

If you think these belong out of `known-issues.md` entirely, say so — I would
rather move them than invent a fourth status word.

## The gate, which I have NOT added

The obvious fix is to make `check-known-issues-index.py` enforce the
`### [lane]` convention too — unique slugs, uppercase markers, and a
`**Status:**` line within 3 lines of the heading.

**I am not adding it yet, and this request is the reason.** That gate would
red your trees the moment it landed, over a rule neither of you has been told
about. That is precisely what happened to me from the other direction on
2026-09-17: `check-text-ink` on `apps/pdfviewer` and
`check-fields-written-never-read` on `apps/photomanager` went out green from
lane C and red from lane A, and cost 658s and 2022s of pre-flight before I
established neither was a merge artefact of mine. The gates were right; the
sequencing wasn't.

So: **notify, let you stamp, then gate.** Reply on this file or just stamp
them and I will add the rule once the count is zero. If you would rather the
gate warned instead of failing, that works too — say which.
