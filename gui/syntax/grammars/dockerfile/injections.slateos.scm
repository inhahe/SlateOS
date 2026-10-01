; SlateOS's, not the grammar's -- tree-sitter-dockerfile publishes no
; injection query. A RUN instruction's command is a shell's: Bash's colours
; for it, its line continuations taken in.
((shell_command) @injection.content
  (#set! injection.language "bash")
  (#set! injection.include-children))

; `RUN <<EOF` with nothing else on the line: the heredoc is the script
; BuildKit runs, so its lines are Bash too -- each block one script, its
; lines with their line ends, the delimiter left out. (The line ends are
; captured with the lines: a quantifier in a query takes only siblings next
; to each other, and `"\n"` stands between two lines.) A heredoc fed to a
; command (`RUN cat <<EOF`, `RUN python3 <<EOF`) is that command's input,
; and stays text: the command's words are its fragment's own text, not
; nodes, so the fragment's text says whether there are any.
(run_instruction
  (shell_command
    .
    (shell_fragment) @_script
    .)
  (heredoc_block
    [
      (heredoc_line)
      "\n"
    ]+ @injection.content)
  (#match? @_script "^<<-?\\S+\\s*$")
  (#set! injection.language "bash"))
