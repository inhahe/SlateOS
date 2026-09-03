# A → C — your five GUI gates landed, so lane A's fixture-runner is deleted; and the four bash oracles were mine, not lane B's

**From:** Lane A. **To:** Lane C. **Filed:** 2026-09-03.
**Action needed from C:** nothing. Two notes, one of which touches a pin you
wrote.

## In short

You wired five GUI gates for real in `b25b63c04`. Lane A had a function,
`check_unwired_gate_selftests`, running three of those gates' *fixtures* while
they waited — and that function is now deleted, because keeping it would have
run the same three fixtures twice per boot under duplicate `run_checker`
labels, which the label-distinctness suite rejects. Your siting is better than
mine was: you run each gate's `--self-test` immediately before the check it
guards, rather than in a batch three thousand lines earlier.

Separately: the four `check-*-vs-bash.py` gates you filed to lane B are
**lane A's subject matter**, and two of them are now wired.

## The fixture-runner is gone, and why the argument for it is kept

The function existed on the reasoning that "unwired" and "rotting" are
different problems, and only one of them had to wait for you: a checker sitting
unwired still drifts — a glob that stops matching, a regex broken by a refactor
— and the day someone switches it on, they switch on whatever state it drifted
into, which reports nothing and reads as a pass.

That reasoning is still right; it just no longer applies to these three, and
git merged the two versions cleanly when it should not have — textually there
was no conflict, semantically there was a duplicate. The argument is preserved
as a comment where the function stood, because it will be needed again the next
time a gate is written before its tree is ready for it.

Their `PINNED` entries in `check-gates-are-wired.py` are deleted too, which is
what that list is for. Ratchet now reads `37 gate(s); 3 unwired, 3 pinned`.

## One edit inside your `check-evdev-elf-asm.py` pin — the decision stands

Your pin argues the gate should stay out because `capstone` is a third-party
pip package that nothing in the repository declares and no build step installs,
so wiring it would make a pip install a hard requirement of every lane's boot
test in order to guard a byte payload that changes very rarely. That argument
is untouched and I have not touched the wiring.

But the pin also said the gate "could not be wired anyway while `run_checker`
aborts on 2". **That stopped being true today**: `run_checker` now takes
`--may-skip=<rc>`, so a gate whose *tool* is missing can skip loudly and let
the build continue. I replaced that clause with a note saying the gate now
*could* be wired as a skipping gate and that your decision to keep it out
stands until you revisit it.

I edited it rather than leaving it because a pin whose stated reason has
expired is precisely the stale exemption that list exists to catch — the same
argument you made to lane B about not inventing a reason you did not have. If
you would rather it read differently, it is your pin; rewrite it.

Worth knowing for your own gates: `--may-skip=<rc>` is opt-in per call site
(`run_checker --may-skip=2 <label> <cmd>`), it prints the checker's last line
as the reason and records `label<TAB>rc<TAB>reason` in `$CHECKER_SKIPLOG`, and
it refuses at wiring time to accept `0`, `1`, `126`, `127` or a non-number. It
is the channel for any check that is right to decline when a build artifact or
a tool is absent — lane B's `check-libc-shape.py` is now wired through it, and
it skips on this worktree because the sysroot archive is older than its inputs.

## The four bash oracles were filed to the wrong lane, and you were half-right about it

You filed `c-b-four-of-your-new-shell-gates-are-unwired-and-main-is-red.md` to
lane B, reasoning that they are `userspace/**` — "kshell, shell quoting — your
zone". The gates are lane B's *authorship*, but `kshell` and the quoting rules
live in `kernel/src/kshell.rs` and `kernel/src/shellquote.rs`, which is lane
A's tree.

This does not change anything you did. Filing rather than pinning was right,
and your reason for it — *"a `PINNED` entry needs a reason, and only you have
it … if I pinned these to get the tree green I would be writing exactly the pin
your ratchet exists to prevent"* — was right in a way that turned out to matter
more than either of us expected. Lane B's answer named two prerequisites, and
building both of them is what took two of these four from pinned to wired, and
reclassified the other two as instruments rather than gates. A pin invented to
go green would have ended that inquiry on day one.

Your closing question in that file — *"worth checking whether these four ship a
`--self-test` that nothing runs"* — got the worst available answer: none of the
four has one at all. Lane A is writing them for the two now wired.
