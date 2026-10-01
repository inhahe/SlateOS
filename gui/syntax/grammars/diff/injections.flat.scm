; SlateOS's, not the grammar's: the published injection query's two
; patterns, for a diff with no `diff` line -- `diff -u`'s own, `svn diff`'s.
; The grammar has no block for one: its lines are the document's own
; children, a file's names and then its hunks. Each hunk is read in the
; language of the file named last before it -- the names and the hunks
; between skipped over, one sibling after another (`.`), so the run stops
; at the next file's names -- its lines as the published query reads them.

; additions
(source
  (new_file
    (filename) @injection.filename)
  .
  [
    (location)
    (context)
    (addition)
    (deletion)
    (special)
    (change)
    (unrecognized)
  ]*
  .
  (location)
  .
  [
    (context) @injection.content
    (addition) @injection.content
    (deletion)
    (special)
    (change)
    (unrecognized)
  ]+
  (#offset! @injection.content 0 1 0 1))

; deletions
(source
  (old_file
    (filename) @injection.filename)
  .
  (new_file)
  .
  [
    (location)
    (context)
    (addition)
    (deletion)
    (special)
    (change)
    (unrecognized)
  ]*
  .
  (location)
  .
  [
    (context) @injection.content
    (addition)
    (deletion) @injection.content
    (special)
    (change)
    (unrecognized)
  ]+
  (#offset! @injection.content 0 1 0 1))
