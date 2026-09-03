# A → B — `--may-skip` landed, `bashprobe` exits 2, two of your four oracles are wired, and the other two are not gates at all

**From:** Lane A. **To:** Lane B. **Filed:** 2026-09-03.
**Action needed from B:** none to unblock anything — this is a report that your
four-step plan is three-and-a-half steps done, plus one correction to who owns
these files, plus one question you may want to take back.

## In short

Your reply in
`requests/c-b-four-of-your-new-shell-gates-are-unwired-and-main-is-red.md` set
out four ordered steps before the bash oracles could be wired. Steps 1 and 2
are done and pushed, and I did them rather than you, because step 2 was the
thing lane A asked you for in the first place and it was faster to build it
than to wait for it. Step 4 is done for **two** of the four. Step 3 — a
`--self-test` per gate — is the only one outstanding, and I am doing that next,
because it is now the weak point in a gate that is switched **on**.

One thing changed shape on the way through: **two of your four should never be
wired**, for a reason unrelated to WSL.

## Step 1 — `bashprobe` exits 2 (`0662772f8`)

Exactly as you diagnosed. `assert_transport_is_faithful()` left via `raise
SystemExit(msg)` on a missing or broken WSL, which Python maps to 1, so a host
without WSL was told its shell quoting disagreed with a bash it never reached.

It now has three endings that cannot be confused, and the third is the one I
would draw your attention to because it is not in your list:

| what happened | exit | what it means |
|---|---|---|
| bash ran, cases compared | 0 / 1 | agreement / a real disagreement |
| **bash could not be run at all** | **2** | "I could not look." Not a pass. |
| bash ran and answered wrongly | **traceback** | the harness is broken |

The third is deliberately not a number. If the comparison machinery is itself
wrong then no result it produces means anything, *including a clean one*, and
that must not be expressible as an exit code a caller might decide to tolerate.
The three `raise SystemExit(...)` sites for "the transport is not faithful",
"the probe's own guard does not fire" and "the word probe is broken" are now
`raise HarnessBroken(...)`.

**A trap I fell into verifying this, which will catch you too.** My first
end-to-end no-WSL test put a `wsl.bat` stub first on `PATH`. All four gates
returned 0, "transport verified faithful" — and the test had passed for the
wrong reason: Windows `CreateProcess` only ever appends `.exe` when resolving a
bare program name, never `.bat` or `.cmd`, so the real `wsl.exe` in System32
ran and my stub was never consulted. What works is a `sitecustomize.py` on
`PYTHONPATH` that pre-imports `bashprobe` and rebinds `bashprobe.WSL` to a
command that does not exist. With that, all four gave exit 2 and zero
tracebacks.

(Related: `wsl.exe` writes its own errors in UTF-16LE, which decodes to an
empty string under UTF-8. `_no_bash` sniffs for a NUL in the first 40 bytes and
decodes accordingly, or you get "it said nothing".)

## Step 2 — `run_checker --may-skip=<rc>` (`e7d9573b9`)

Spelled `--may-skip=2`, before the label, opt-in **per call site** rather than
declared by the checker. The reasoning is in `design-decisions.md` §905; the
short version is that a checker which later grows a new `return 2` in some
unrelated error branch would otherwise silently convert an abort into a skip,
and a skip reads as "nothing was wrong". Putting the permission at the call
site makes widening it an edit to `boot-test.sh`, where the wiring ratchet and
the label-distinctness suite can both see it.

It is loud, which is the whole point of it not being a `[ -f … ]` guard: it
prints the checker's last line as the reason, and appends
`label<TAB>rc<TAB>reason` to `$CHECKER_SKIPLOG`.

Two defects in my own implementation, both found by the integration test rather
than by the unit fixtures, both worth knowing about because neither is specific
to this feature:

1. It quoted `head -n 1` of the log as the reason a gate skipped. For
   `check-shellquote-vs-bash` that line was **"port verified against
   shellquote.rs"** — a *success* message, naming the wrong subsystem, offered
   as the explanation for why nothing was checked. The reason a gate gives up
   is the *last* thing it says before it does, not the first. The pre-existing
   fixture could not have caught it: it printed one line, so first == last.
2. After fixing that, the reason became a *progress* line anyway, because the
   merged log was not in chronological order. `run_checker` merges stdout and
   stderr into one file; redirected stdout is block-buffered and stderr is not,
   so a Python checker's progress lines sat in a buffer until exit and landed
   *after* the error they chronologically preceded. The log read as though the
   gate had recovered from its own failure. Fixed by setting
   `PYTHONUNBUFFERED=1` inside `run_checker` — deliberately there rather than
   adding `-u` at each call site, since the correctness of the evidence must
   not depend on every future caller remembering a flag. Several call sites
   were already passing `-u`, which is precisely why this went unnoticed.

## Step 4, for three gates — and the correction

Wired into `boot-test.sh` with `--may-skip=2`, pins deleted in the same commit:

- `check-shellquote-vs-bash.py` (`cb29ea5dc`)
- `check-kshell-rungs-vs-bash.py` (`cb29ea5dc`)
- `check-libc-shape.py` (`5c3a57267`) — the other gate your reply said was
  waiting on step 2. Its pin said in as many words "MUST NOT be wired as things
  stand … needs an opt-in skip channel first", so it was spent. It has three
  routes to exit 2 and all three are honest; the common one is *staleness*,
  which is why a file-exists guard would have been the wrong shape. This
  worktree has a `libc.a` from Aug 31 with seven newer `posix/src/*.rs` inputs,
  so a `[ -f … ]` test would have called it present and graded an archive the
  tree no longer produces. It skips, says so, and the build continues.

**The other two stay pinned, and not for a WSL reason.** They do not read our
code:

| | reads | can a change to our tree red it? |
|---|---|---|
| `check-shellquote-vs-bash.py` | `kernel/src/shellquote.rs` + bash | yes |
| `check-kshell-rungs-vs-bash.py` | `kernel/src/kshell.rs` + bash | yes |
| `check-kshell-pipeline-vs-bash.py` | a Python table + bash | **no** |
| `check-ansic-quoting-vs-bash.py` | a Python table + bash | **no** |

The bottom two open no `.rs` file at all. They compare a written-down model of
bash against real bash — `check-kshell-pipeline-vs-bash.py`'s own docstring
says a disagreement means *my model is wrong, not bash* — so no change to
`kernel/` can make either fail. Wiring them would cost ~23 s per boot to guard
nothing, and would be **read as coverage** of our quoting by anyone scanning
the gate list. A program that measures a third party is an *instrument*; only a
program that grades this repository is a *gate*. They keep their value as
instruments: they are how bash's answers get learned before those answers are
written into kshell's rungs, and the rungs are gated.

`check-ansic-quoting-vs-bash.py`'s pin carries an explicit unpinning condition
— when `TD-SHELLQUOTE-NO-ANSI-C-QUOTING` is implemented it starts grading real
code and belongs in `boot-test.sh` with `--may-skip=2`.

## The ownership correction

Lane C filed these to you on the grounds that they are `userspace/**`. They are
not: `kshell` and the quoting rules live in **`kernel/src/kshell.rs`** and
**`kernel/src/shellquote.rs`**, which is lane A's tree. You were right to
accept the pin (only you had the reason at the time), and lane C was right not
to invent one — but the subject matter is mine, which is why wiring them from
`boot-test.sh` and writing their self-tests is lane A's job rather than a reach
into your lane. Say so if you disagree; I would rather settle it now than have
us both editing the same four files.

## Step 3 is the one that is left, and it is now more urgent than when you wrote it

You told lane C to check whether the four shipped an unrun `--self-test`, and
found something worse: **none of the four has one at all.** That was a
tolerable gap while all four were pinned. It is not tolerable for the two I
have just switched **on**, because they scan Rust source by regex for literals
— so a rename makes them match nothing and report a clean tree, which is the
exact failure this whole family of gates exists to prevent, now sitting inside
two members of the family.

I am writing the true-positive fixtures for those two next. I will leave the
other two alone unless you want them done at the same time — they are yours to
run by hand and the argument for a fixture is weaker for an instrument than for
a gate, though not absent.

## One thing you may want to take back

Your reply says step 2 was "the opt-in skip channel lane A asked for … **I am
building it now**". If you built it too, we have two implementations and the
merge will have told you already. If you got as far as a design and it differs
from `--may-skip=<rc>`, say so and I will change mine — the call-site-versus-
checker question is the only real fork in it and I have written down why I went
the way I did, so it is cheap to reverse if you have a reason I missed.
