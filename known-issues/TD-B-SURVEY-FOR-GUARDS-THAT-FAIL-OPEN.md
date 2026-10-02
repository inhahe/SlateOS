## TD-B-SURVEY-FOR-GUARDS-THAT-FAIL-OPEN (lane B, 2026-09-10)

**In short:** after fixing `mkfs`/`fsck`, I ran the check I had just told the
other two lanes to run: grep this lane for a guard that answers "safe to
proceed" when it could not tell. One more turned up, in the package manager,
and it destroyed user data rather than a filesystem.

**Method, so it can be repeated.** 134 sites in lane B match
`Err(_) => return false`, `Err(_) => false` or `unwrap_or(false)`. Most are
harmless -- a shell option that defaults to off, a "is this a symlink" that
falls back to no. What narrows it is asking **which way `false` points**: a
guard is dangerous when `false` means *go ahead*. Deriving the set that way --
every `-> bool` function containing a fail-open arm, then reading the names --
gave 165 functions, of which all but a handful were `userspace/oils` shell-option
accessors.

**What it found: `userspace/pkg`'s `is_user_modified`.** It guarded
`deploy_config`, which overwrites a config file on upgrade unless the user has
edited it. It returned `false` -- "not modified", therefore overwrite -- in two
cases where it had no idea:

* the file could not be **read**, though the caller had already established it
  exists, so the failure is a permission problem, an I/O error or a race; and
* the installed version had **no recorded checksum**, which is the state of any
  package installed by an older `pkg` or any database that lost the field.

The second is the one that would have been met in practice: on such a system
*every* user-edited config was silently replaced on the next upgrade, with the
notice that exists for exactly that case never printed.

Renamed to `must_preserve`, because the question the caller asks is "may I
overwrite this?" and the old name answered a different one. Both arms now
preserve, which costs a `.pkg-new` file and a notice when the file really was
untouched -- `rpm`'s `.rpmnew` bargain. Four tests, where there were none.

**What it did not find**, which is worth recording so nobody repeats the sweep:
no guard named `is_mounted`, `is_busy`, `is_in_use`, `is_locked`, `is_readonly`,
`is_running`, `is_active`, `in_use`, `is_protected`, `is_immutable` or
`is_open` fails open anywhere in lane B. The two that did are fixed.

### A third one, 2026-09-12, that this method could not have found

`doas`'s `user_in_group` -- the guard deciding whether `permit :wheel` or
`deny :wheel` applies to you. It failed open for `deny`, and **every one of its
error arms was already correct**. The fail-open was in the *success* path.

The function asked whether `/etc/group`'s **member list** names you. That field
deliberately omits everyone whose *login* group it already is (`pwdb::Group::
members` says so, and glibc agrees -- measured, `id -nG` lists the primary
group). So a caller whose primary group was `wheel` was answered "not a
member". For `permit :wheel` that withholds a grant, which is safe. For `deny
:wheel` the deny stops applying **to precisely the accounts most likely to be
in the group**, and the caller falls through to whatever `permit` comes next.

**Why the survey above could not reach it**, which is the part worth keeping:

| The method looked for | This guard |
|---|---|
| a `-> bool` function | returns `Option<bool>` |
| a fail-open **error arm** (`Err(_) => false`) | error arms all correct; one returns `None`, and `None` is handled well -- an unevaluable `deny` is *fatal* |
| `false` produced by not knowing | `false` produced by **knowing a narrower fact than the question** |

That last row is the new shape. The guard was not confused about whether it
could answer; it answered confidently, about something slightly different from
what it was asked. No grep over error handling finds that, because there is
nothing wrong with the error handling.

The irony is on the record in the file: `read_group_entries`' doc comment, ten
lines above, was written *specifically* to warn about this inversion after lane
A found it in `mkfs`/`fsck` -- *"for a check guarding a privileged action, 'I
do not know' and 'it is safe' must not be the same value."* The comment guarded
the error path. The bug was in the success path, eight lines below it.

**A filter that would have caught it**, offered for the next sweep rather than
as a finished tool: for each guard on a privileged action, ask not "what does
it do when it fails" but **"what fact does it actually establish, and is that
the fact the caller needs?"** Here the caller needs *membership*; the function
established *listed-in-the-member-field*. Those differ for the commonest case
there is. That question cannot be mechanised, but it can be asked of the dozen
or so guards that gate a privilege, which is a reviewable number.
