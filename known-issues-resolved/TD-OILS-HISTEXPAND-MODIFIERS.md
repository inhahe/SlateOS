### TD-OILS-HISTEXPAND-MODIFIERS. the `:h`/`:t`/`:r`/`:e` modifiers were pathname-aware, and an unknown modifier was silently ignored — ✅ RESOLVED 2026-07-29

**Where:** `userspace/oils/src/histexpand.rs` — `modify_path()`, the tail of
`apply_modifiers()`, and the `None` arm of `apply_word_designator()`.

**What.** Three unrelated divergences in the modifier suffix, all found by
probing bash 5.2 while investigating `^old^new^` (below):

* **`modify_path` implemented a *sensible* basename/extension split; readline's
  is one naive `strrchr` over the whole selected text.** osh emptied the string
  when the separator was missing (`echo abc` under `:h` gave the empty string
  where bash leaves `echo abc`), special-cased a leading `/` (`/abc:h` gave `/`
  where bash gives nothing at all), and confined the dot search to the basename
  and skipped a leading dot (`a.b/c:r` was unchanged where bash gives `a`, and
  `.abc:r` was unchanged where bash gives the empty string). Rewritten as four
  `rfind`s with the text returned unchanged when the separator is absent.
* **An unrecognized modifier was silently ignored.** `echo !!:z` appended a
  literal `:z`; bash reports `z: unrecognized history modifier` and abandons the
  line. A `:` commits to a modifier, so anything else after it — including the
  end of the line, which bash reports with an *empty* character name (`!!:`,
  `!!:g`, `!!:h:` all print `: unrecognized history modifier`) — now fails the
  whole expansion. A character not preceded by a `:` is unaffected and stays
  literal, which is why `!!:hz` is still the `:h` of the event with a `z` on the
  end, and `!!:pz` prints `… z` rather than running it.
* **A dangling `:` was eaten by the word-designator scan.** `apply_word_designator`
  consumed the `:` before returning "no designator here", so the modifier scan
  never saw it and could not report it. It now hands the position back
  unconsumed, exactly as its non-designator sibling arm already did.
* **`:s` with no delimiter at all** (`!!:s` at end of line) is a no-op in bash,
  not an error — the `s` is consumed and the text is left alone. osh left the
  `:s` behind as literal text.

**Tests.** `tests/corpus/histexpand-modifiers.sh` (new) pins all of it
differentially; unit tests `pathname_modifiers_are_naive_strrchr` and
`unrecognized_modifier_aborts_the_expansion` cover the same ground in-process.
