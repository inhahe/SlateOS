## 623. A transcription reproduces an upstream bug only when the bug is cosmetic; a bug that gives a wrong answer is fixed

**Date:** 2026-08-27
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** our `renice` is a line-by-line copy of the real `renice` from
util-linux, so that it behaves identically. The real one has a genuine defect:
`renice 0 -u root` works, but `renice 0 -p abc -u root` says "unknown user
root" -- root has not gone anywhere, it is just that a leftover scrap from the
earlier bad word `abc` is still lying around when the program checks whether
"root" was found. Copying it faithfully would mean copying that too. We copied
the rest and fixed this one, and the rule this sets is: **reproduce a bug that
only makes output look wrong; fix a bug that makes output *be* wrong.**

### The bug

`renice.c` declares one `char *endptr` for the whole `main`, and every
`strtol` in the operand loop writes through it. The user branch does not:

```c
} else if (which == PRIO_USER) {
        struct passwd *pwd = getpwnam(*argv);
        if (pwd != NULL)
                who = pwd->pw_uid;          /* endptr not touched */
}
if (who < 0 || *endptr)                     /* ...but read here */
        warnx(_("unknown user %s"), *argv);
```

So a *successful* lookup is validated against a byte an *earlier, unrelated*
operand left behind. Measured on util-linux 2.39.3:

| Command | Upstream | Why |
|---|---|---|
| `renice 0 -u root` | works | `endptr` still points at the priority word's `\0` |
| `renice 0 -p abc -u root` | `unknown user root` | `abc` left `endptr` on `a` |
| `renice 0 -g abc -u root` | `unknown user root` | same |
| `renice 0 -u nosuchuser -u root` | `unknown user root` | the failed lookup fell through to `strtol`, which left `endptr` on `n` |
| `renice 0 -u nosuchuser -p 393` | works | `-p 393` called `strtol`, which reset `endptr` |

The last row is what makes it a *stale-state* bug rather than a plain logic
error: whether a correct command works depends on what the command before it
was.

### Why this one is fixed and `cal`'s two are not

Entry 622 records that our `cal` reproduces two upstream alignment bugs
deliberately, and that looks like the opposite policy. The distinguishing
question is **what a user loses by the bug**:

| | `cal`'s bugs | this one |
|---|---|---|
| What is wrong | a column of a calendar is three bytes off | a user that exists is reported as not existing |
| What the user does | reads the calendar anyway | believes root is absent, or does not renice it |
| Detectable from output | yes, immediately | no -- the message is a plausible lie |
| Anything scripted on it | no; nobody diffs `cal -v 2 2024` | yes; `renice` in a script checks the status |

A cosmetic bug is part of the program's *appearance*, which is the thing a
transcription exists to preserve -- a `cal` whose columns line up "better"
than upstream's is a `cal` whose output does not match, and matching is the
point. A wrong answer is not appearance; reproducing it would mean shipping a
utility that lies about the state of the system in order to be bug-compatible
with one that does. `B-WHICH-DIVERGES-FROM-GNU-IN-FOUR-MEASURED-PLACES` had
already made this call once, declining two GNU `which` bugs that reported a
present executable as missing; this is the same call and now the same stated
rule.

### The rule, stated

When transcribing a utility, a departure from upstream needs a reason. These
are the two that are accepted:

1. **The bug produces a wrong answer** -- reports a present thing as absent,
   an absent thing as present, a wrong number, or a wrong exit status. Fix it,
   record it as a numbered divergence in the module doc, and pin the fix with
   a test named for the behaviour rather than for the bug.
2. **The bug is unreachable here** -- it depends on a facility this OS does
   not have. Say so; do not emulate the facility to reproduce the bug.

Everything else -- spacing, column alignment, ordering of equal things,
message punctuation, the choice of which of two equally-valid diagnostics to
print -- is reproduced, because it is the observable surface that the
transcription is for.

### Alternatives rejected

- **Reproduce it, for exactness.** Rejected: nothing depends on the wrong
  behaviour, and the wrong behaviour is a lie about the password database. A
  script that does `renice -5 -p $pid -u $svc` would silently skip the user.
- **Reproduce it behind a switch.** Rejected: a flag whose only function is to
  turn on a bug is a maintenance cost with no user.
- **Fix it silently.** Rejected: an undocumented divergence is a difference
  someone rediscovers as a bug in *our* program. It is divergence 1 in
  `renice.rs`'s module doc, with the measurement table above it.

### Where it lives

`userspace/coreutils/src/bin/renice.rs`. The structural fix is that there is
no shared `endptr` at all: `strtol_whole` returns `Option<i64>`, which is
`None` exactly when C would leave `*endptr` non-zero, so the "was the whole
word consumed" answer cannot outlive the word it is about. The lookup path in
`read_target` returns `Target::Id` directly and never consults a number. The
test `a_successful_lookup_is_not_poisoned_by_an_earlier_bad_operand` is the
pin.

### How to reverse

Reintroduce a mutable `last_endptr_was_clean: bool` threaded through the
operand loop, set by `strtol_whole` and left alone by the lookup, and consult
it in the user branch. That is a faithful model of the C. Also delete the
test, and move divergence 1 in the module doc into a "reproduced upstream
bugs" list next to `cal`'s.
