## TD-B-QUOTE-NAMES-COUNTED-ONE-OF-TWO-SPELLINGS (lane B, 2026-09-10) — fixed

**In short:** `scripts/quote-names.py` flags `eprintln!("prog: {path}: {e}")`
and did not flag `eprintln!("prog: {}: {e}", path.display())` — the same
defect, the older spelling. Seven sites were in the tree while the baseline
read "0 sites in 0 files". Detector extended, all seven fixed, baseline still
zero and now means it.

**How it surfaced.** Not from the gate. A one-line fix in `setcap` replaced a
positional diagnostic with an inline capture, and the gate refused the push —
correctly. The line it *replaced* had been there all along and had always
passed. A gate rejecting a change while accepting its predecessor is the
cheapest possible signal that its rule is narrower than its subject, and it is
only visible when you touch the line.

**A correction to my own commit message.** `f14def32b` said 224 sites across 43
files. That number came from grepping for any unquoted `.display()` inside any
diagnostic macro. The gate's proposition is narrower — `prog: <name>: <rest>` —
and it does not cover the broader class in the **inline** spelling either. So:

| Population | Size | Covered? |
|---|---|---|
| `prog: {name}: …`, inline | — | yes, always |
| `prog: {}: …`, positional | **7** | no, until now — the actual gap |
| any `.display()` in any diagnostic | ~224 | no, in either spelling; never in scope |

The gap was real and is seven. The 224 is a different, larger population this
gate has never claimed. Stating it as the gap overstated the finding, in a
commit message that is on `main`, which is why the correction is here rather
than only in a later message.

**Fixed:** `cpio` (2), `scp` (2), `getty`, `make`, `pkg` — `quotef_os` /
`quoteaf_os`, and the `quoting` dependency added to the three crates lacking
it. Eight new self-test fixtures, including an escaped quote inside the format
string, because reading the argument list from *inside* the literal is the
mirror image of the `strip_noise` bug that hit `check-read-defaults.py` twice
this week.

**Still uncovered, deliberately:** the ~224. Widening the detector to every
`.display()` in every diagnostic is a different decision with a much larger
burn-down, and it should be taken as one rather than smuggled in behind a
seven-site fix.
