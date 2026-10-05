### TD-OILS-A-SUBSTITUTIONS-EXTENT-IS-FOUND-BY-A-SECOND-SCAN-RATHER-THAN-BY-PARSING — 2026-08-10 — OPEN (design debt)

**Where:** `userspace/oils/src/lexer.rs`, `read_balanced_body` and its re-lex of
the body it returns. bash reads a `$( … )` body **once**, with the real parser
(`parse_comsub`, parse.y:4133, a whole nested `yyparse`), and the body's extent
is wherever that parse stops. osh reads it **twice**: a character scan finds the
`)`, then the text between is lexed again.

The scan is therefore a model of the parser, and every construct in which a `)`
closes nothing has to be taught to it separately. The list is now complete as
far as anything measured reaches — quotes, `$'…'`, `${ … }`, backticks,
comments, here-documents (including their ordering across nesting and their
end-of-input warnings), `case` patterns, `(( … ))` and `$(( … ))` spans, nested
`$(`/`<(`/`>(`, and array subscripts — and each was added because a measurement
demanded it, not speculatively. But the shape is still one pass too many, and
its cost is real: the re-lex has to be told what the first pass already ate
(`Spanned::taken`, `heredocs_forgotten`, `SubstBail`), and a construct nobody
has measured yet will be wrong in the same way each of the above was.

**The fix** is to read the body with the ordinary reader and let its error out,
taking the extent from where that reader stopped. What makes it a large change
rather than a small one is that the here-document machinery currently depends on
the two-pass shape: `gather_ahead` moves the cursor onto later lines and records
a read-ahead, and it is called at the *nested* `)` as well as the outer one so
that bash's warning order and line numbers come out right. Doing that from
inside a single reader means the reader itself has to carry the substitution
nesting, which is roughly what bash's `save_parser_state`/`restore_parser_state`
pair is for.

Not urgent: nothing measured is currently wrong because of it. Logged so the
next construct that turns out to need teaching is recognised as the third
reminder rather than as a one-off.
