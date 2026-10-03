## §332 — The sudoers parser errors where the grammar is ground truth and warns where our own table is; `=` on a list setting replaces rather than adds

**Date:** 2026-08-18
**Decided by:** Claude (autonomous)

**In short:** `sudo` reads a policy file, `/etc/sudoers`, that says who may run
what as another user. Until now our parser accepted every line of it: a typo
like `Defaults timestamp_timout=5` was stored under that misspelled name, did
nothing, and was never mentioned — including by `visudo -c`, the tool whose
whole job is to say "this file is wrong" before an administrator installs it.
The parser now rejects malformed lines. Two choices inside that were not
obvious, and this entry records them: what to do about a setting name our table
has simply never heard of, and what `env_keep = "X"` should mean when
`env_keep` already has a value.

### Error where you have ground truth, warn where you have a list

Real sudo rejects an unknown `Defaults` name outright, because its catalogue of
settings is complete — it is the same program that implements them. Ours is
not. `KNOWN_DEFAULTS` in `userspace/sudo/src/main.rs` lists ~58 settings, real
sudo has more, and the list grows with each sudo release. A name missing from
our table is therefore not evidence that the name is wrong; it is evidence that
our table is short.

So the two kinds of complaint are separated by *what is actually known*:

| Situation | What we know | Report |
|---|---|---|
| A listed setting used with an operator its shape forbids — `Defaults requiretty=5`, `Defaults secure_path` with no value, `Defaults env_keep` used as a flag | A fact about the grammar. `requiretty` is a boolean; it cannot take a value under any version of sudo | **Error** |
| A name that is not in `KNOWN_DEFAULTS` at all | A fact about *our table*, which is knowingly incomplete | **Warning** |
| A name that is listed but that this implementation does not yet act on | A fact about our implementation | **Warning**, and only under `visudo --strict` |

*What changes:* `visudo -c` on a file using a setting we have not catalogued
prints a warning and still reports the file usable, while a file that misuses a
setting we *do* know is rejected with an exit status.

The alternative — error on every unknown name, matching real sudo — was
rejected because its failure mode is unfixable by the person who hits it. An
administrator writing a perfectly valid sudoers file, using a setting real sudo
documents and we have not listed, would be told the file is invalid, with no
way to proceed except editing our source. A validator that refuses correct
input is worse than the silence it replaced: the silence merely failed to help,
whereas this actively blocks. The opposite alternative — keep checking nothing,
on the grounds that our table is incomplete — was rejected because
incompleteness of the *name* list says nothing about the *shape* rules, which
are the part that catches real typos.

The two lists (`KNOWN_DEFAULTS` and `HONOURED_DEFAULTS`, the settings actually
acted on) are kept from drifting apart by a test, `honoured_defaults_are_all_known`.
Two hand-maintained lists that must agree is the defect shape this tree keeps
rediscovering; the test is the cheapest available answer to it.

### `=` replaces, `+=` adds — and the existing test said otherwise

sudoers gives list settings four operators: `=` sets the list, `+=` appends,
`-=` removes, `!` disables. Our parser implemented `=` as append, and the
pre-existing test `env_keep_extended` asserted exactly that: it wrote
`Defaults env_keep="CUSTOM_VAR"` and then checked that `TERM` — a default —
was *still* kept.

*What changes:* a file that writes `Defaults env_keep="CUSTOM_VAR"` now keeps
`CUSTOM_VAR` and nothing else, where before it kept `CUSTOM_VAR` plus the whole
built-in list.

This is a behaviour change to a security policy, so it is worth being explicit
about the direction of the error. `env_keep` is the list of environment
variables passed through to the privileged command; everything not on it is
stripped. An administrator who writes `env_keep = "..."` is stating the whole
list, and the usual reason to do so is to make it *shorter* than the default.
Treating that as an append means a file written to narrow what crosses the
privilege boundary does not narrow it — the variables the author intended to
strip keep flowing. Being wrong in the other direction (stripping a variable
the author wanted kept) makes a command fail visibly and gets fixed. So the old
behaviour was wrong in the direction that fails silently, and the test that
encoded it was rewritten rather than preserved, with a dated in-test comment
recording why the assertion was inverted.

Nothing in the tree ships an `/etc/sudoers` (`git grep -ln "etc/sudoers"` finds
none), so no existing configuration changes meaning as a result of either
decision.

### Why the operator is now kept instead of folded into the name

Adjacent to both, and not really a decision so much as a defect worth
recording, because its shape recurs. The parser took the setting name to be
everything before the first `=`, so `env_keep += "X"` was stored under the name
`env_keep +`. Two consumers compensated by matching three spellings each
(`env_keep`, `env_keep+=`, `env_keep+`); a third, `get_default`, had no such
workaround, so `timestamp_timeout` and `env_reset` written with `+=` were never
seen at all.

That is band-aid accumulation in miniature: one defect in one place, paid for
separately at each consumer, with the consumer nobody thought to patch simply
broken. The operator is now parsed into a `DefaultOp` and carried alongside the
name, the triple matches are deleted, and the operators are applied in written
order — which is also what makes `=`/`+=`/`-=` distinguishable at all.
