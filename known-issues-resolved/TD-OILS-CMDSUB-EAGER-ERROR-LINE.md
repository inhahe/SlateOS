### TD-OILS-CMDSUB-EAGER-ERROR-LINE. A syntax error the *enclosing* parse raises inside a `$( … )` body reported a rank-renumbered line, not the physical one — 2026-07-29 — ✅ RESOLVED same day

**Where:** `userspace/oils/src/parser.rs` `parse_cmdsub_body`.

**What:** a `$( … )` body is read twice, and the two reads number its lines
differently. The expansion-time read counts up from the closing `)`'s line by
rank (that is the `$LINENO` rule). The *enclosing* read is numbered plainly —
bash sees the body in its own token stream, so a syntax error it raises names
the body's true physical line and echoes that physical script line back.

`parse_cmdsub_body` applied the rank map to the eager parse as well, because
until `TD-OILS-CMDSUB-DOLLAR-NOT-INCREMENTAL` was fixed that eager program was
also the one that *ran*, so its lines had to satisfy `$LINENO`. Once the body
started being re-read at expansion time, the eager program's line numbers
became observable only through this diagnostic — and were wrong.

**Reproduce:**
```sh
# c.sh
echo start
x=$(echo a
echo b
for
echo d)
```
```
bash: c.sh: line 4: syntax error near unexpected token `newline'
      c.sh: line 4: `for'
osh:  c.sh: line 7: syntax error near unexpected token `newline'
      c.sh: line 7: `'          # past EOF, so the echoed line came out empty
```

**Fix:** `parse_cmdsub_body` builds the rank map (returned, for the re-read) but
renumbers the eager parse with an ordinary `LineMap::Offset` back to the line
the body opens on — `close_line` less the body's own newline count, since the
`)` sits on the body's last line. Nested bodies inherit physical close lines
and compute their own offset the same way. Covered by
`tests/corpus/cmdsub-eager-error-line.sh` (first/middle/own-line bodies, a
nested body, the mid-construct `)` blame, and a process substitution for
contrast) and `parser::an_eager_cmdsub_body_error_names_its_physical_line`.
