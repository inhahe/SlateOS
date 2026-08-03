# `bind -f` and the startup inputrc: readline's init-file reader, whose grammar
# is a superset of the one `bind`'s operands use.
#
# The two share a line syntax — `set NAME VALUE`, `"SEQ": target`, blank, `#`
# comment — and a rule that nothing in a line is fatal: a line readline cannot
# use is reported and the next one is read, and the builtin's status does not
# move. Only failing to *open* the file is `bind -f`'s own failure, and that one
# is a diagnostic with the `bind:` prefix and status 1. A directory is not a
# failure: bash opens it, reads nothing, and says nothing.
#
# What a file adds is the `$` directives, which mean nothing as a `bind` operand
# because there are no further lines for them to include or exclude:
#
#   $if COND / $else / $endif   COND is `mode=`, `term=`, `version OP N`, or —
#                               anything else — the application name, compared
#                               case-insensitively. So `$if Bash` is true and
#                               `$if application=bash` is *false*: readline has
#                               no `application=` form, so that whole string is
#                               read as a name, and it is not `bash`. `term=`
#                               matches the full `$TERM` and the part before its
#                               first `-`. A false `$if` hides its whole body
#                               including any `$if` nested in it — nothing turns
#                               parsing back on but the matching `$endif` — and
#                               an `$if` left open simply ends with the file, in
#                               silence. `$else` or `$endif` with no `$if` is
#                               reported, and the lines around it still apply.
#   $include FILE               read relative to the *shell's* directory, not to
#                               the including file's. One that cannot be read is
#                               silently nothing.
#
# A complaint from inside a file is readline's voice with the file and line in
# front of it: `readline: FILE: line N: MSG`. `FILE` is the name as written.
#
# The keymap is one live variable that everything moves together. `set keymap`
# — as an operand or as a line of a file — steers every binding after it and
# outlives the call. `-m` is readline's save-and-restore around the whole
# builtin: it steers each phase, shows through `bind -v`, and is then put back,
# so even a `set keymap` reached during a `-m` call does not survive it.
#
# The startup read is not an interactive-only thing. A non-interactive shell
# with no line editor still answers `bind` out of tables `$INPUTRC` has been
# folded into, and it happens lazily at the first `bind` — so exporting a
# different `INPUTRC` after that first call changes nothing.
#
# Every other `bind` case sets `INPUTRC=/dev/null` precisely to keep that read
# out of the way; this one is where it is the subject.
exec 4>&2 2>err

echo "=== the file named by INPUTRC is folded in before the first bind answers"
printf '"\\C-t": yank\n' > startup
export INPUTRC=startup
bind -q yank
# Lazily, and once: the tables are already seeded, so a later INPUTRC is inert.
export INPUTRC=/dev/null
bind -q yank

echo "=== -f reads a file into the keymap the call is working on"
printf '"\\C-e": yank\n' > f1
bind -f f1; echo "  rc=$?"
bind -q yank
bind -m vi-insert -f f1; echo "  -m rc=$?"
bind -m vi-insert -q yank
# ... and `-m` put the keymap back.
bind -v | grep -a 'set keymap'

echo "=== a file that cannot be opened is the builtin's failure, a directory is not"
bind -f /nosuch/file; echo "  missing   rc=$?"
mkdir adir
bind -f adir; echo "  directory rc=$?"

echo "=== a line readline cannot use is reported against its own line number"
cat > bad <<'INPUTRC'
set nosuchvariable on
$else
$endif
$nonsense arg
"\C-g: yank
"\C-]": yank
INPUTRC
bind -f bad; echo "  rc=$?"
bind -q yank

echo "=== a false conditional hides its whole body, nesting and all"
cat > cond <<'INPUTRC'
$if nosuchapp
$if Bash
"\C-b": yank
$endif
"\M-b": yank
$else
"\C-^": yank
$endif
$if Bash
"\C-_": yank
$endif
$if application=bash
"\M-\C-b": yank
$endif
INPUTRC
bind -f cond; echo "  rc=$?"
bind -q yank

echo "=== an if left open ends with the file, silently"
printf '$if Bash\n"\\M-\\C-e": yank\n' > open
bind -f open; echo "  rc=$?"
bind -q yank

echo "=== version and mode"
cat > ver <<'INPUTRC'
$if version >= 4.0
"\M-\C-g": yank
$endif
$if version < 4.0
"\M-\C-h": yank
$endif
$if mode=emacs
"\M-\C-k": yank
$endif
$if mode=vi
"\M-\C-l": yank
$endif
INPUTRC
bind -f ver; echo "  rc=$?"
bind -q yank

echo "=== include is resolved against the shell's directory"
mkdir -p sub
printf '"\\M-\\C-n": yank\n' > sub/inner
printf '$include sub/inner\n$include /nosuch/file\n' > outer
bind -f outer; echo "  rc=$?"
bind -q yank

echo "=== set keymap inside a file steers the lines after it, and outlives it"
cat > km <<'INPUTRC'
set keymap vi-insert
"\C-t": yank
set keymap vi-command
"\C-t": yank
INPUTRC
bind -f km; echo "  rc=$?"
bind -v | grep -a 'set keymap'
bind -m vi-insert -q yank
bind -m vi-command -q yank
bind 'set keymap emacs'

echo "=== a set keymap operand steers the operands after it, in one call"
bind 'set keymap vi-insert' '"\C-n": yank'
bind -m vi-insert -q yank
bind -v | grep -a 'set keymap'
bind 'set keymap emacs'

echo "=== a set keymap reached under -m does not survive the call"
printf 'set keymap vi-command\n"\\C-o": yank\n' > kmm
bind -m vi-insert -f kmm; echo "  rc=$?"
bind -v | grep -a 'set keymap'
bind -m vi-command -q yank
bind -m vi-insert -q yank

echo "=== a macro, an empty macro and a -x command all come through a file"
cat > mac <<'INPUTRC'
"\C-k": "hi\tthere"
"\C-l": ""
"\M-\C-y": nosuchfunction
INPUTRC
bind -f mac; echo "  rc=$?"
bind -s
bind -q yank

echo "=== unbinding through a file"
printf '"\\C-e":\n' > un
bind -f un; echo "  rc=$?"
bind -q yank

echo "=== stderr, in one place"
exec 2>&4
cat err
