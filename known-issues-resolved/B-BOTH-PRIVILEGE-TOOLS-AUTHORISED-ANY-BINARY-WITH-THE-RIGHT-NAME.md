## B-BOTH-PRIVILEGE-TOOLS-AUTHORISED-ANY-BINARY-WITH-THE-RIGHT-NAME (lane B, 2026-09-12) -- FIXED

**In short:** `doas` and `sudo` both decide whether a rule's command matches
what you asked to run. Both compared **only the last part of the path** when
the rule named a command without a directory. So a rule saying you may run
`pkg` let you run *any* file called `pkg` -- including one you had just written
yourself, in a directory you control, as root.

```text
doas.conf:  permit alice cmd pkg
$ doas /tmp/evil/pkg          # ran as root
$ doas ./pkg                  # so did this
$ PATH=/tmp/evil doas pkg     # and this, because doas read the CALLER's PATH
```

sudoers was the same shape: `alice ALL = pkg`, and `sudo /tmp/evil/pkg`. In
`sudo` the command is the caller's argv verbatim -- nothing resolves it -- so
the basename arm was the whole of the check.

**Proven before either was changed.** An assertion added to each program's
existing basename test showed the escalation returning `true`. Both are kept,
inverted, as `..._does_not_authorise_a_path_the_caller_chose`.

**The fix, the same in both.** An unqualified spec is *resolved* against a
trusted path and the whole path is compared. `cmd pkg` therefore still means
`/usr/bin/pkg`, which is what anybody writing it meant, and means nothing else.
A spec naming a program that is not on that path matches nothing -- the safe
direction: a rule that cannot be resolved authorises nothing rather than
authorising by name.

The trusted path differs per program, and in each case it is one the program
already had:

| | trusted path | note |
|---|---|---|
| `doas` | `target_path(uid)` | the `PATH` it was already going to hand the target, so the rule check and the `exec` cannot disagree about which binary a name means |
| `sudo` | `Defaults secure_path` | **parsed, stored and never read until today** -- a setting that accepted a value and did nothing with it. This is its first consumer. |

**How it was found.** By applying the filter written a few hours earlier, after
`doas`'s `user_in_group`: the fail-open survey greps *error arms*, and neither
of these has one. The question that reaches them is **"what fact does it
actually establish, and is that the fact the caller needs?"** The caller needs
*is this the command the rule names*; the function established *does the last
path component match*, which is a different statement the moment the caller
supplies a path of their own.

That is now three defects found by that question in one day -- `doas`'s group
membership, `sudo`'s credential clock, and this -- against zero found by
grepping error handling, which had already been done and had already declared
both files clean.

**Two host artifacts that each read exactly like the fix being wrong**, and
cost two rounds apiece to tell from a real failure. `ScratchDir::path` composes
with a backslash on Windows while the code composes with `/`; `fs::metadata`
accepts both, so the file was found and only the string comparison failed. And
an absolute Windows path starts with a drive letter and a colon, while the
search splits a `PATH` on `:` -- so the test searched two fragments of its own
fixture directory. Both programs now split the path list at the edge
(`path_dirs`) and pass the split list inward, which is a seam for the host's
benefit and costs the target nothing.
