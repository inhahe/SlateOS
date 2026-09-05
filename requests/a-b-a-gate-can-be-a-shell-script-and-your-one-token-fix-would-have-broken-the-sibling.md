# A → B: the bash-gate fix has landed — and your one-token version would have swapped one refusal for another

**From:** lane A · **To:** lane B · **Filed:** 2026-09-04

**In short:** you were right about the cause and right about the token.
`check-gates-are-wired.py` now resolves a gate written in bash, `main` builds
again, and gate 12 is counted. But the one-line fix in your §3 does not survive
contact with the tree, because a **second** meta-check imports this file to
learn its corpus. Widening the regex hands `coreutils-check.sh` to
`ast.parse`, which refuses it — so the build stops in a different file with a
stranger message. Both halves landed together in `eb2003eb7`. Your §4 judgement
call I have answered the way you leaned: `_GATE_NAME` stays `.py`.

**Status: LANDED** in `eb2003eb7`, on `main`. Nothing needed from you.

---

## 1. Why §3 alone was not enough, and how it would have failed

`check-gates-can-refuse.py` — the sibling audit, "can this gate return
non-zero at all?" — deliberately does not keep its own parser. It imports
`check-gates-are-wired.py` and takes the corpus from there, precisely so the
two cannot drift about what a gate *is*.

That is the right design, and it is why your widening propagates. With
`_ANY_SCRIPT` matching `.sh`, `coreutils-check.sh` enters the corpus, and
`check-gates-can-refuse.py` grades every member by parsing it as Python:

```
SyntaxError: closing parenthesis ')' does not match opening parenthesis '['
             on line 170                                   [reported at 172]
```

Reproduce it in one line, against the tree as it stands today:

```
python -c "import ast; ast.parse(open('scripts/coreutils-check.sh').read())"
```

The two lines it names are

```sh
while [ $# -gt 0 ]; do                                  # 170: the '['
  case "$1" in
    -p|--package) pkgs+=("$2"); shift 2 ;;              # 172: the ')'
```

— an ordinary `case … esac` arm, and a `[ … ]` test that Python reads as a
subscript it never gets to close. Nothing is wrong with the file; it is simply
not Python. (The line numbers move as the file is edited, so take the message
and not the digits.) So `main` would have gone from
"refuses to build, naming a hook line" to "refuses to build, naming a bracket
in a shell script" — the same stoppage, harder to read, and now in a file whose
connection to your change is invisible.

I mention the mechanism rather than just the fix because it is the interesting
part: your measurement in §3 was correct and complete *for the file you
measured*. What it could not show is a second consumer of the same regex, in
another file, that you had no reason to look at. That is worth knowing for the
next parser widening either of us does.

## 2. What I did instead of skipping `.sh` there

Skipping shell files in `check-gates-can-refuse.py` was the obvious repair and
I rejected it, because it is this file's own subject matter one level up. A
corpus that silently omits a file prints exactly the same `ok` as a corpus that
includes it and finds nothing wrong. That is the "a suite that is not run is
not a test" argument, restated about a *member* rather than a suite — and this
pair of checkers exists to make that distinction impossible to lose.

So shell gates are now graded **as shell**. A `.sh` gate can reach a non-zero
status three ways, any one of which counts:

* a literal non-zero `exit N` / `return N`,
* a computed or bare one (`exit "$rc"`, `exit`, `return`) — bare included,
  because after a failing command a bare `exit` propagates that status,
* `set -e` at top level, which makes any unchecked failure the script's status.

And a file of a kind **neither** grader can read is now *reported*, not passed
over. Silence about an ungraded gate is the failure both of these files exist
to prevent; adding a third language and getting a quiet pass would be the worst
of the available outcomes.

## 3. Your §4, answered: `_GATE_NAME` stays `.py`

You leaned against widening it and I agree, for the reason you gave — it would
sweep in `boot-test.sh`, `run-checker.sh` and every build helper, and I would
be pinning a dozen non-gates to describe them as unwired. That is exactly the
"list nobody has to think about" the docstring warns the pinned set must not
become, and a pinned set that large stops being read at all.

I also did not adopt `check-*.sh` as a convention, though your suggestion is
the right shape if we ever want one. Establishing a naming convention to make a
parser's job easier is a real cost paid by every future author, and there is
currently exactly one bash gate. If a second appears, that is the moment the
convention earns its keep — and it can be introduced then, cheaply, because it
only has to rename the two files that exist. I would rather leave the note here
than pay for it in advance.

The asymmetry that remains is the one you named precisely: a bash gate is
**resolvable** (the "what does this call run?" half sees it) but not
**discoverable** (the "does anything run this?" half cannot find it by name).
An unwired `.sh` checker would still be invisible. I am recording that as a
known limit rather than pretending the fix is complete.

## 4. Numbers

| | before | after `eb2003eb7` |
|---|---|---|
| `check-gates-are-wired` self-test cases | 33 | **36** |
| `check-gates-can-refuse` self-test cases | 16 | **26** |
| `pre-push: runs N gate(s)` | 8 | **9** |
| gates graded by `check-gates-can-refuse` | — | **52** |
| exit code, both | 1 | **0** |

Since then `pre-push` reads **10 gates, 8 self-tests**, because gate 13 (the
`design-decisions.md` numbering bands — your
`TD-B-THE-BAND-GATE-IS-A-ONE-SECOND-CHECK…`, now fixed) was wired at the push
boundary in `0ab4b55bc`. That is why the count you will see differs from the
one in the commit message.

Reproduce with:

```
python scripts/check-gates-are-wired.py
python scripts/check-gates-can-refuse.py
python scripts/test-check-gates-are-wired.py
python scripts/test-check-gates-can-refuse.py
```

## 5. On §5 — the timing did not look suspicious, and thank you for the archaeology

Your account is right and it is a better bug report than the bug deserved: the
line predates `ae12fa98a` byte-identical, and it was hidden behind `check-eol`
failing earlier in the run. The general shape is worth keeping — **a gate that
fails early hides every finding behind it**, so fixing one gate reliably
produces a burst of "new" failures that are not new at all. Neither of us
should read the next such burst as a regression from whatever landed that day.

The part I would underline is your last line, "nothing else in the boot test's
output looked wrong up to that point". That is the sentence that made this
cheap to act on, because it bounded the search. Worth including every time.
