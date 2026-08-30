#!/usr/bin/env python3
"""Prove the lock screen's authentication tests are regression tests.

The thirteenth of these harnesses, and the one with the least excuse not to
exist: `known-issues.md` ->
`TD-C-THE-LOCK-SCREEN-THROWS-AWAY-THE-ANSWER-TO-THE-ONLY-QUESTION-IT-ASKS`
records that `apps/lockscreen` shipped with a full suite of green tests over a
screen that *could not unlock*. `submit_password` returned a `bool` and both
callers wrote `let _ =`. Every test called `submit_password` directly, so the
one thing that was broken -- the wiring between the unit and the world -- was
the one thing nothing looked at.

That is the exact failure mode a reintroduction sweep exists to catch, and it
is why the rework that fixed it does not get to be trusted on the strength of
"the tests pass". They passed before.

So each defect below is a way the new plumbing could be wrong:

- the verdict computed and dropped again, in either of the two submit paths;
- the unlock flag read instead of taken, so one password unlocks for ever;
- the flag not revoked by a wrong guess, or carried across a user switch;
- "nothing here can check a password" collapsed back into "wrong password",
  which is the distinction lane B insisted the return type exist for;
- the username not sent, or the wrong name sent, to an authority that will
  shortly be per-machine rather than per-screen;
- `Locked` shown to the user in different words from `Rejected`, which tells
  an attacker which of the two they hit.

None stops it compiling. None is visible in a screenshot. All of them are the
kind of thing that reads fine.

Restore discipline as in the companions: a byte snapshot up front, written back
unconditionally in a `finally`, verified by SHA-256 -- not a reverse
search-and-replace, which silently leaves the tree modified if a patch
half-applied or the process died between the write and the undo.

Two modes:

- `--check` matches every pattern against the snapshot and builds nothing.
  Seconds, no toolchain, and it answers the question that rots on its own: has
  a rename or a rustfmt pass stopped a defect applying?
- No flag: the real sweep. Apply, run the tests, restore, report.

Filter either with defect letters: `reintro-lockscreen.py A B C`.
"""

import hashlib
import pathlib
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
TARGET = "x86_64-pc-windows-gnu"

MAIN = "apps/lockscreen/src/main.rs"

# (name, file, [(old, new), ...], [packages], [tests expected to fail])
DEFECTS = [
    (
        "A: one accepted password unlocks for ever, not once",
        MAIN,
        [("        core::mem::take(&mut self.unlock_requested)",
          "        self.unlock_requested")],
        ["lockscreen"],
        ["an_unlock_is_authorised_once_and_not_left_standing"],
    ),
    (
        "B: pressing Enter consumes the event and submits nothing",
        MAIN,
        [("                    self.submit_password();\n"
          "                    EventResult::Consumed\n"
          "                }\n"
          "                Key::Backspace",
          "                    EventResult::Consumed\n"
          "                }\n"
          "                Key::Backspace")],
        ["lockscreen"],
        ["test_key_enter_submits",
         "pressing_enter_with_the_wrong_password_authorises_nothing"],
    ),
    (
        "C: clicking the submit button submits nothing",
        MAIN,
        [("                        if hit_test(mouse.x, mouse.y, &submit_rect) {\n"
          "                            self.submit_password();\n",
          "                        if hit_test(mouse.x, mouse.y, &submit_rect) {\n")],
        ["lockscreen"],
        ["clicking_submit_with_the_right_password_authorises_an_unlock"],
    ),
    (
        "D: a wrong guess leaves an earlier authorisation standing",
        MAIN,
        [("        self.unlock_requested = false;\n"
          "        self.failed_attempts = self.failed_attempts.saturating_add(1);",
          "        self.failed_attempts = self.failed_attempts.saturating_add(1);")],
        ["lockscreen"],
        ["a_failed_guess_revokes_an_authorisation_nobody_collected"],
    ),
    (
        "E: an authorisation survives a switch to another account",
        MAIN,
        [("            self.unlock_requested = false;\n"
          "        }\n"
          "    }\n"
          "\n"
          "    /// Append a character to the password buffer.",
          "        }\n"
          "    }\n"
          "\n"
          "    /// Append a character to the password buffer.")],
        ["lockscreen"],
        ["switching_user_revokes_an_authorisation_earned_by_the_other_account"],
    ),
    (
        "F: a screen with no authority reports a wrong password, not a fault",
        MAIN,
        [("            AuthOutcome::Unusable,\n"
          "            |authority| authority.authenticate",
          "            AuthOutcome::Rejected,\n"
          "            |authority| authority.authenticate")],
        ["lockscreen"],
        ["a_screen_with_nothing_to_check_a_password_against_reports_a_fault"],
    ),
    (
        "G: the display name is sent where the login name was meant",
        MAIN,
        [("        let username = self.active_user().username.clone();",
          "        let username = self.active_user().display_name.clone();")],
        ["lockscreen"],
        ["the_username_reaches_the_authority_that_has_to_look_it_up"],
    ),
    (
        "H: an empty stored entry is an acceptance in its own right",
        MAIN,
        [("        matches!(self, Self::Accepted)",
          "        matches!(self, Self::Accepted | Self::NoPassword)")],
        ["lockscreen"],
        ["an_empty_stored_entry_is_not_by_itself_an_acceptance"],
    ),
    (
        "I: a locked account is told the difference from a wrong password",
        MAIN,
        [('            Self::Rejected | Self::Locked | Self::Unusable => "Incorrect password",',
          '            Self::Rejected | Self::Unusable => "Incorrect password",\n'
          '            Self::Locked => "This account is locked",')],
        ["lockscreen"],
        ["a_locked_account_is_told_apart_from_a_typo_without_being_shown_apart"],
    ),
    (
        "J: a wrong password is treated as needing an administrator",
        MAIN,
        [("        matches!(self, Self::Locked | Self::Unusable)",
          "        matches!(self, Self::Locked | Self::Unusable | Self::Rejected)")],
        ["lockscreen"],
        ["a_locked_account_is_told_apart_from_a_typo_without_being_shown_apart"],
    ),
    (
        "K: the lockout says nothing about how long it has left",
        MAIN,
        [("                retry_after_secs: self.lockout.remaining_secs(),",
          "                retry_after_secs: 0,")],
        ["lockscreen"],
        ["test_lockout_blocks_submit"],
    ),
    (
        "L: submitting an empty box costs the user an attempt",
        MAIN,
        [("        if self.password_buffer.is_empty() {\n"
          "            return AuthOutcome::Rejected;",
          "        if self.password_buffer.is_empty() {\n"
          "            return self.settle(AuthOutcome::Rejected);")],
        ["lockscreen"],
        ["test_submit_empty_password"],
    ),
    (
        "M: the typed password stays in the buffer after it is submitted",
        MAIN,
        [("        self.password_buffer.clear();\n"
          "        if Self::unlocks_for(outcome) {",
          "        if Self::unlocks_for(outcome) {")],
        ["lockscreen"],
        ["a_submitted_password_does_not_stay_in_the_buffer"],
    ),
    (
        "N: a right password does not clear the failures before it",
        MAIN,
        [("            self.failed_attempts = 0;\n"
          "            self.unlock_requested = true;",
          "            self.unlock_requested = true;")],
        ["lockscreen"],
        ["a_correct_password_clears_the_failures_that_came_before_it"],
    ),
    (
        "O: an account with no password can never unlock (open-questions option B)",
        MAIN,
        [("        matches!(outcome, AuthOutcome::Accepted | AuthOutcome::NoPassword)",
          "        matches!(outcome, AuthOutcome::Accepted)")],
        ["lockscreen"],
        ["test_no_password_user_unlocks_immediately"],
    ),
    (
        "P: the lockout no longer blocks a submit",
        MAIN,
        [("        if self.lockout.is_active() {\n"
          "            return AuthOutcome::RateLimited {",
          "        if false && self.lockout.is_active() {\n"
          "            return AuthOutcome::RateLimited {")],
        ["lockscreen"],
        ["test_lockout_blocks_submit"],
    ),
    (
        "Q: a failed attempt no longer starts a lockout",
        MAIN,
        [("        if let Some(duration) = lockout_duration_for_attempts(self.failed_attempts) {\n"
          "            self.lockout.start(duration);\n"
          "        }\n"
          "        outcome",
          "        outcome")],
        ["lockscreen"],
        ["test_lockout_after_5_failures", "test_lockout_blocks_submit",
         "test_lockout_blocks_typing"],
    ),
    (
        "R: the shake and the error message no longer follow a wrong password",
        MAIN,
        [("        self.show_error = true;\n"
          "        self.shake.trigger();",
          "        self.shake.trigger();")],
        ["lockscreen"],
        ["test_submit_wrong_password"],
    ),
    (
        "S: the password is tidied up on its way to the authority",
        MAIN,
        [("            |authority| authority.authenticate(&username, self.password_buffer.as_bytes()),",
          "            |authority| authority.authenticate(&username, "
          "self.password_buffer.to_lowercase().as_bytes()),")],
        ["lockscreen"],
        # This one was unproved on the first sweep, and for the most ordinary
        # reason there is: every password anywhere in the file was lowercase
        # ASCII, so folding the case changed nothing any test looked at. Not
        # `the_username_reaches_the_authority_that_has_to_look_it_up`, which
        # does assert the password bytes but types "hunter2" -- it is about
        # the name, and lowercasing "hunter2" is the identity.
        ["the_password_reaches_the_authority_exactly_as_it_was_typed"],
    ),
]

# Defects whose reintroduction is genuinely unobservable, kept for the record
# and skipped by the sweep. Empty today.
NO_OP: set[str] = set()


def letter(name):
    """The defect's identifier -- everything before the first colon."""
    return name.split(":", 1)[0]


def run_tests(pkg):
    r = subprocess.run(
        ["cargo", "test", "-p", pkg, "--target", TARGET],
        cwd=ROOT, capture_output=True, text=True, errors="replace",
    )
    out = r.stdout + r.stderr
    # "error: test failed" is what a *failing test run* prints, so only
    # "could not compile" distinguishes a build break.
    if "could not compile" in out:
        return None, out
    failed = set()
    collecting = False
    for line in out.splitlines():
        s = line.strip()
        if s == "failures:":
            collecting = True
            continue
        if collecting:
            if "::" not in s:
                collecting = False
                continue
            failed.add(s.rsplit("::", 1)[-1])
    return failed, out


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    check_only = "--check" in sys.argv[1:]

    files = sorted({d[1] for d in DEFECTS})
    snap = {f: (ROOT / f).read_bytes() for f in files}
    digest = {f: hashlib.sha256(b).hexdigest() for f, b in snap.items()}
    print("snapshot:")
    for f in files:
        print(f"  {digest[f][:16]}  {f}")
    print()

    selected = [d for d in DEFECTS
                if letter(d[0]) not in NO_OP
                and (not args or letter(d[0]) in args)]

    if check_only:
        bad = 0
        for name, path, edits, _pkgs, _expect in selected:
            text = snap[path].decode("utf-8")
            problems = []
            for old, new in edits:
                n = text.count(old)
                if n == 0:
                    problems.append("PATTERN NOT FOUND")
                elif n > 1:
                    problems.append(f"AMBIGUOUS ({n} matches)")
                elif old == new:
                    problems.append("NO-OP")
                else:
                    text = text.replace(old, new, 1)
            verdict = "; ".join(problems) if problems else "ok"
            if problems:
                bad += 1
            print(f"{name}\n    {verdict}")
        print(f"\n{len(selected) - bad}/{len(selected)} patterns apply cleanly")
        sys.exit(1 if bad else 0)

    verdicts = []
    try:
        for name, path, edits, pkgs, expect in selected:
            text = snap[path].decode("utf-8")
            ok = True
            for old, new in edits:
                if old not in text:
                    ok = False
                    break
                text = text.replace(old, new, 1)
            if not ok:
                verdicts.append((name, "PATTERN NOT FOUND"))
                print(f"{name}\n    PATTERN NOT FOUND\n", flush=True)
                continue
            (ROOT / path).write_text(text, encoding="utf-8", newline="")

            all_failed, note, broke = set(), "", False
            for pkg in pkgs:
                failed, _out = run_tests(pkg)
                if failed is None:
                    broke, note = True, f"{pkg} did not compile"
                    break
                all_failed |= failed
            (ROOT / path).write_bytes(snap[path])

            if broke:
                verdict = f"DID NOT COMPILE ({note})"
            elif not all_failed:
                verdict = "*** NO TEST FAILED ***"
            else:
                verdict = f"caught by {len(all_failed)}: {sorted(all_failed)}"
                missing = [t for t in expect if t not in all_failed]
                if missing and len(missing) == len(expect):
                    verdict += f"  [MISSING: {missing}]"
            verdicts.append((name, verdict))
            print(f"{name}\n    {verdict}\n", flush=True)
    finally:
        bad = []
        for f in files:
            (ROOT / f).write_bytes(snap[f])
            if hashlib.sha256((ROOT / f).read_bytes()).hexdigest() != digest[f]:
                bad.append(f)
        if bad:
            print(f"!!! NOT RESTORED: {bad}")
            sys.exit(2)
        print("restored: all files match their recorded SHA-256")

    print("\n=== summary ===")
    for name, verdict in verdicts:
        print(f"{name}\n    {verdict}")
    unproved = [n for n, v in verdicts
                if "NO TEST FAILED" in v or "NOT FOUND" in v
                or "DID NOT COMPILE" in v]
    print(f"\n{len(verdicts) - len(unproved)}/{len(verdicts)} defects caught")
    if unproved:
        print("unproved:")
        for n in unproved:
            print(f"  {n}")


if __name__ == "__main__":
    main()
