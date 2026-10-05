### TD-OILS-HISTEXPAND-QUICKSUB. `^old^new^` was a path of its own, so everything after the closing delimiter was thrown away — ✅ RESOLVED 2026-07-29

**Where:** `userspace/oils/src/histexpand.rs` — `expand()`, and the deleted
`quick_substitution()`.

**What.** osh treated `^old^new^` as a whole-line form with its own parser and
its own diagnostics. readline does something much simpler: it *textually*
prefixes `!!:s` to the line and runs the ordinary expander over the result. Four
observable consequences, all measured against bash 5.2 and none of which the
bespoke path reproduced:

```
$ echo one two
$ ^one^X^ tail       bash → echo X two tail    osh → echo X two
$ ^one^X^^           bash → echo X two^        osh → echo X two
$ ^one^X^ !!         bash → echo X two echo one two   (the tail is expanded too)
$ history -c ; ^a^b^ bash → !!: event not found  osh → :s^a^b^: event not found
```

**Fix.** `expand()` now builds `format!("!!:s{line}")` when the line starts with
`^` and falls into the normal loop; `quick_substitution()` is gone. The `:s`
error wording (`:s^zz^two^: substitution failed`, `:s^: no previous
substitution`) comes out of `apply_modifiers` unchanged, because it already
quoted the modifier back from the `:` — which is exactly the rewritten spec.

One subtlety needed a new `Expansion` variant. bash adopts `history_expand`'s
output string unconditionally but only *echoes* it when the expander proper
changed something, so a `^old^new^` whose `!!` turns out to be quoted runs, and
is recorded, as the literal `!!:s^old^new^` with nothing echoed to stderr:

```
$ echo 'x
$ ^one^two^'         bash prints  x  then  !!:s^one^two^   with an empty stderr
```

`Expansion::ChangedQuietly` carries that case. It is only reachable once the
reader's quote state is carried across physical lines
(TD-OILS-HISTEXPAND-LINE-QUOTE-STATE), which is the change that produced the
measurement above.

**Tests.** `tests/corpus/histexpand-quicksub.sh` (new) plus unit tests
`quick_substitution_keeps_what_follows_the_delimiter` and
`quick_substitution_failures_name_the_rewritten_spec`.
