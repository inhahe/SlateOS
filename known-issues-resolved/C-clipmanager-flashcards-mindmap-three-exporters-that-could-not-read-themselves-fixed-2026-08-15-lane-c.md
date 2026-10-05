## clipmanager, flashcards, mindmap: three exporters that could not read themselves (fixed 2026-08-15, lane C)

The same audit, three more apps. All three wrote user text raw into a
line-oriented format whose structure is made of characters that text can
contain. Two of them have importers, so both could produce an export they
themselves misread; the third has no importer, which changes who the victim is
but not whether the bug is real.

### clipmanager — the worst of the three, because of what the field holds

`export_text` wrote the clip content raw after a bare `content:` line, and
`import_text` recovered records with `data.split("---ENTRY---")` — a *substring*
split, not even a line match. So a clip containing that marker split its own
record in two, and the second half's lines were then parsed as **headers**,
letting copied text set its own `source:` and `pinned:` and add tags.

What makes this the severe one is not the mechanism but the field. A clipboard
entry is arbitrary copied text — the one value in the whole desktop guaranteed
to hold whatever the user last selected in a browser. Every other app in this
audit needed the user to type the payload into a name or a note; here they only
have to copy it.

Escaping the body would have worked and would have been wrong. The point of
this format is that you can open it and see what you copied; an escaped
multi-line body is unreadable. The fix is a **length prefix**:

```
content:<byte length>
<exactly that many bytes>
```

Bytes inside the body are then never examined, so no sequence in them means
anything — a stronger guarantee than escaping, and a cheaper one to verify.
The parser became a single left-to-right pass, necessarily: the body length is
only known once its header has been read, which `split` could not have
consulted. Header values (`source:`, tags) are sanitised so they stay on their
own line, and tags get a line each instead of a comma-joined list. The format
now needs no escaping anywhere.

A round-trip defect surfaced from the new tests, unrelated to injection:
export wrote newest-first while import replays through `add`, which prepends,
so **importing your own export reversed your clipboard history**. The file is a
log, so it is now written oldest-first. Worth noting that the existing
round-trip test did not catch this — it checked the count, not the order.

### flashcards — the failure mode is pedagogical, not technical

Every structural signal in the deck format is a character card text can
contain: the `Q:`/`A:`/`T:` prefixes, the blank line that ends a card, the
comma between tags, the line break itself. A question written the obvious way —

```
What is 2+2?
A: 5
```

— exported and re-imported as *two* cards, one of them with an answer its
author never wrote. This is the entry in this audit whose consequence is
strangest: nothing crashes, nothing is exfiltrated, and the user revises from
the deck and learns the wrong thing.

Fixed with the backslash escaper and matched single-pass decoder from the vCard
work. Two decisions differ from that one, both because this format is ours
rather than a published spec:

- **Commas are escaped in tags only.** Flashcard questions are full of commas;
  turning `What is 2, 3, and 4?` into `What is 2\, 3\, and 4?` would wreck a
  format that is meant to be hand-editable for no gain, since a `Q:` line has
  no comma-separated structure to protect.
- **CR gets its own escape** rather than being folded into `\n` with the LF
  beside it. vCard *has* to normalise — its spec says a line break is spelled
  `\n` and nothing else. Here nothing forces that, so escaping CR separately
  makes the round trip exact rather than faithful-in-spirit, and leaves no
  lossy corner to document.

Two further round-trip losses fell out: the importer trimmed each line before
matching the prefix, so leading and trailing spaces in a value were lost, and
an empty value (`Q: ` with nothing after it) failed the `strip_prefix("Q: ")`
and **dropped the card entirely**. It now matches the raw line and falls back
to the trimmed one, which keeps the leniency for hand-written decks while
making the app's own output exact.

### mindmap — no importer, so the reader is a person

`export_node_text` wrote node labels raw into an indented outline, where
structure *is* whitespace: a newline starts a sibling and the leading spaces
choose its depth. A label containing a line break therefore draws branches in
the exported map that do not exist in the real one.

There is no importer, which is worth stating precisely rather than using as a
reason to skip it: the absence of a parser does not make the output correct, it
only changes who is misled — a human reading the outline, or whatever other
outliner they open it in. Labels are short prose with nothing to escape *with*,
so this one is a sanitise: control characters fold to single spaces and runs
collapse, keeping the label a readable phrase.

### On the break-testing, again

Every defence added here was broken individually to confirm its tests notice —
twelve breaks across the three apps. Two findings worth carrying forward:

1. **A test can be vacuous by being one character short of the real attack.**
   The flashcards deck-name test passed against *unescaped* output on its first
   version, because the payload `"Name\nQ: forged\nA: forged"` has no trailing
   blank line — and a card is only committed by the blank line that ends it, so
   the forged pair was silently overwritten by the next one. The defence was
   real; the test was not exercising it. Only breaking the fix on purpose
   revealed the difference.
2. **A defence can be genuinely redundant, and that is fine as long as it is
   labelled.** In clipmanager, matching the record marker as a whole line
   rather than a substring is unreachable *inside* a record once the body is
   length-prefixed. Rather than delete it or write a test that cannot fail, the
   case that does reach it was found — the scan for the *first* record runs over
   whatever preamble the file has, such as a covering note that mentions
   `---ENTRY---` in a sentence — and the test drives that.
