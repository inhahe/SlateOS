### TD-OILS-A-HERE-DOCUMENT-DELIMITER-OF-A-LONE-TRAILING-BACKSLASH. `cat <<\` at the end of the input — 2026-08-06 — ✅ FIXED 2026-08-08

**Where:** `userspace/oils/src/parser.rs` — the new `close_last_line` /
`InputKind`; `userspace/oils/src/interp.rs` — `Shell::run_source_out` and
`Shell::run_source_flow_result`; `userspace/oils/src/main.rs` —
`Plan::Command`. **Not** `Lexer::read_heredoc_delim`, where this entry
originally looked for it.

**What.** A `\` with nothing after it quotes nothing. osh dropped it, leaving an
empty (quoted) delimiter; bash keeps it, and what it does next depends on which
reader is running:

```text
eval 'cat <<\'          bash: warning: here-document … (wanted `\')
                        osh : warning: here-document … (wanted `')

a file whose last line  bash: syntax error: unexpected end of file
is `cat <<\`, no        osh : warning: here-document … (wanted `')
trailing newline
```

**Why — and why the entry was looking in the wrong place.** The delimiter scan
was never the subject. Measuring the same bytes through a script file, `-c`,
`.`/`source`, `eval` and a pipe showed the divergence is not about here-documents
at all: `bash -c 'echo two\'` prints `two\` where a *file* of those bytes prints
`two`, and osh printed `two` for both. The rule is in the reader, and bash
states it outright (parse.y:2567):

```c
  /* Add the newline to the end of this string, iff the string does
     not already end in an EOF character.  */
  if (shell_input_line_terminator != EOF)
    {
      /* Don't add a newline to a string that ends with a backslash if we're
         going to be removing quoted newlines, since that will eat the
         backslash.  Add another backslash instead (will be removed by
         word expansion). */
      if (bash_input.type == st_string && expanding_alias () == 0 &&
          last_was_backslash && c == EOF && remove_quoted_newline)
        shell_input_line[shell_input_line_len] = '\\';
      else
        shell_input_line[shell_input_line_len] = '\n';
    }
```

So a file that stops mid-line is read as though it ended in a newline, and its
final `\` is an ordinary line continuation onto an input that is not there. A
*string* is closed with a second backslash instead, and the pair is one quoted,
literal backslash. (`last_was_backslash` is assigned
`last_was_backslash == 0 && c == '\\'` for every character, so what it holds at
end of input is the parity of the trailing run — `echo two\\\` keeps one
backslash under both readers, `echo two\\\\\` keeps two under the string one.)

**Fix.** The split is exactly `st_stream` vs `st_string`, which osh already had
in structure: `run_source_flow_result` *is* `parse_and_execute`, so the string
rule sits there and covers `eval`, `.`/`source`, a trap action, a `mapfile -C`
callback and an `fc` replay at once; the stream rule sits in `run_source_out`,
the reader loop for a script file and for stdin. `-c` was the one caller on the
wrong side of that line — bash runs it through `parse_and_execute` — so it got
its own entry point, `Shell::run_command_string`. Applying the stream rule first
leaves the string rule nothing to do for a top-level read, so the two compose
without a flag.

**Pinned by** the corpus cases
`the-two-readers-close-an-unterminated-last-line-differently.sh` and
`a-script-file-that-stops-mid-line-is-read-as-if-it-ended-in-a-newline.sh` (that
second file deliberately has no final newline — do not add one), and the unit
test `the_two_readers_close_an_unterminated_last_line_differently`.

**Left behind:** TD-OILS-A-CONTINUATION-AT-END-OF-INPUT-DOES-NOT-MOVE-THE-READER,
which is what the "second, independent divergence" in this entry's earlier text
was pointing at.
