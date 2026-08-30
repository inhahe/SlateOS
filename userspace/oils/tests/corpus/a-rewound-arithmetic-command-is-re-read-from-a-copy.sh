# A `(( … ))` whose two closing parentheses are not adjacent is handed back to
# the ordinary grammar as `( ( … ) )` — see
# an-arithmetic-command-needs-its-closing-parentheses-adjacent.sh for *that*
# rule. This file pins how the hand-back is done, which is visible in four
# separate ways.
#
# bash does not rewind its cursor. `parse_arith_cmd` (parse.y:4519-4562) rebuilds
# the text it has just scanned —
#
#     tokstr[0] = '(';  strncpy (tokstr + 1, ttok, ttoklen - 1);
#     tokstr[ttoklen] = ')';  tokstr[ttoklen+1] = c;
#
# — that is, the physical text with the *first* `(` dropped and the one character
# that failed the adjacency test appended, and hands the copy to the reader with
# `push_string (wval, 0, NULL)` (4498). The parser then yields a plain `(` of its
# own, so the stream is `(` followed by the copy re-read from the second `(`.
#
# 1. **The copy is `shell_input_line` while it lasts**, so `print_offending_line`
#    (6214-6226) echoes the *copy*, not the line it was cut from — embedded
#    newlines and all — with only trailing newlines stripped.
#
# 2. **Reading a pushed string is not reading input**, so the `line_number++` at
#    the top of `shell_getc` (2361) never fires for it: it is neither rewound
#    before the push nor advanced by the newlines the copy contains. Every line
#    inside the copy is therefore blamed on the line the abandoned scan gave up
#    on. The counter resyncs by itself on the next real fetch.
#
# 3. **A copy that ends on a newline is handed that newline twice.** `pop_string`
#    puts the reader back at the saved index, which for a newline-terminated line
#    is the buffer's own NUL (2667-2669); the reader takes that NUL for a word,
#    so end of input is discovered once more than it otherwise would be. A real
#    line answers the extra request for free — only at end of input does it cost
#    a line, which is why the floor below is `scan_end + 2`.
#
# 4. **That NUL is a word, and the word swallows what follows it.** The token
#    buffer opens with the NUL, so whatever is appended after it is past the end
#    of the C string `make_word`'s `savestring` copies: the word's value is `""`
#    however much text was read into it. Everything up to the next delimiter is
#    therefore read *and lost* — `((:)<nl>echo A)` runs `A`, not `echo A` — and
#    an unquoted empty word is removed by splitting, so a copy followed by
#    nothing but its `)` leaves a command with no words at all, status 0.
#    Reading is not running: a `$( … )` in the swallowed run is parsed where the
#    scan meets it (so a bad one is still fatal) and never performed.
#
# The pushes stack, as `push_string`'s list does: a `((` met while a copy is
# being read pushes its own copy on top, and the innermost is what gets echoed.
#
# The push is the *last* thing the scan does, so anything the scan itself
# reported happened while `shell_input_line` was still the physical line. That
# is one thing in practice: a `$( … )` body, which `parse_matched_pair` hands to
# `extract_command_subst` as it goes past — see group 1b.
#
# Not pinned here: a copy that has to reproduce a `\<newline>` the reader
# already deleted, which it cannot — bash's own re-parse desynchronises there.
# Nor is this:
# `bash -c`, `eval` and `.` disagree with `bash file` and
# `bash < file` about a copy that ends on a newline at end of input — the string
# reader stops with the physical line still current and reports `syntax error
# near` against the scan's last line instead. osh follows the stream answer; see
# TD-OILS-A-REWOUND-ARITHMETIC-COMMAND-IS-NOT-REWOUND-THE-WAY-BASH-REWINDS-IT.
# Every case below therefore runs a *file*, which is also the only way to reach a
# genuine end of input.

r() { printf '%b' "$1" > f.sh; "$BASH" f.sh 2>&1; echo "  rc=$?"; rm -f f.sh; }

echo "=== 1. the echoed line is the copy"
r '(( 1 + (2 ))\n'
r '(( fi ) )\n'
r '(( 1; done ) )\n'
r '(( esac ) )\n'
r '(( ) )\n'
r '(( 1\n+ (2 ))\n'
# A token read after the copy is exhausted is echoed from the physical line.
r '(( (1 )) fi\n'
r '(( (1 ))x\n'
# The pushes stack: the innermost copy is the one echoed.
r '(( (( fi ) ) ) )\n'
r '(( (( 1 ) ) ) )\n'
r '( (( 1 ) ) )\n'

echo "=== 1b. but an error the scan itself raised is echoed from the line"
# A `$( … )` body is parsed while the scan is still looking for the closing
# `))` (`parse_matched_pair` hands it to `extract_command_subst`), so a body
# that does not parse is reported before `parse_arith_cmd` has tested anything
# and `shell_input_line` is still the physical line.
r '(( $(fi ))\n'
r '((echo $(fi) ) )\n'
r '(( 1 + $(fi) ) )\n'
r '(( $(for) 1 ) )\n'
r '(( (( $(fi) ) ) ) )\n'
# Even where the copy spans lines and would have been echoed whole.
r '(( 1\n+ $(fi) ) )\n'
# And a body that parses is just text: this one runs.
r '(( $(echo hi) fi ) )\n'

echo "=== 2. the copy's lines collapse onto the scan's last line"
r '(( (nosuch\n) ) )\n'
r '(( (nosuch\n\n) ) )\n'
r '(( (echo A=$LINENO\n) ) )\n'
r '(( (1 \n)) )\n'
# No `((`, so no push, so no collapse — this is what isolates the cause.
r '( (nosuch\n) )\n'
# And the counter resyncs on the next real fetch.
r '(( (nosuch\n\n) ) )\necho L=$LINENO\n'
r '(( (1 )) )\necho L=$LINENO\n'
r 'f() { (( (nosuch\n) ) ); }\nf\n'

echo "=== 3. end of input is floored at the scan's last line plus two"
r '(( (1 ))'
r '(( (1 ))\n'
r '(( (1 ))\n\n'
r '(( (1 ))\n\n\n'
r '(( (1\n))\n'
r '(( (1\n))\n\n'
r '(( (1\n\n))\n'
# Owed only when the tested character was the newline: these end on `;`, a
# space and an `&`, and are charged nothing.
r '(( (1 ));\n'
r '(( (1 )) \n'
r '(( (1 ))&\n'
r '(( (1 ));'
# Again, no `((` and so no charge.
r '( (1 )\n'
r '(( (1 ) ) ; (( (2 ) )\n'

echo "=== 3b. the newline the copy ends on leaves a word behind"
# Nothing follows it, so the word is empty on its own and the command it makes
# has no words: status 0, whatever the copy's last command left.
r '((exit 3)\n)\n'
r '((nosuch)\n)\n'
r '((1+1)\n)\n'
r '(( (exit 3) )\n)\n'
r '(( (( exit 3 )\n) )\n)\n'
r '((exit 3)\n) ; ((exit 4)\n) ; echo end=$?\n'
r '((exit 3)\n) | cat; echo PS=${PIPESTATUS[*]}\n'
# The controls: the tested character has to be the newline. A space or a `)`
# leaves the reader mid-line, where there is no NUL to read.
r '((exit 3) )\n'
r '(( (exit 3 )) ) ; echo z=$?\n'
# The word swallows the run that follows it with no delimiter in between…
r '((exit 3)\necho A)\n'
r '((:)\nfoo bar)\n'
r '((:)\nprintf "[%s]" x y; echo)\n'
r '((:)\nA"B C" D)\n'
r "((:)\n'echo' A)\n"
r '((:)\nX=1 echo A)\n'
r '((:)\n\\\necho A)\n'
r '((:)\n2>out echo A)\ncat out\n'
# …but a delimiter of any kind ends it at once, and then nothing is lost.
r '((:)\n echo one two)\n'
r '((:)\n\techo one two)\n'
r '((:)\n\necho one two)\n'
r '((:)\n;echo A)\n'
r '((:)\n&&echo A)\n'
r '((:)\n|cat)\n'
r '((:)\n>out echo A)\ncat out\n'
r '((:)\n<nosuch)\necho rc=$?\n'
# A `#` is not a delimiter: a comment only opens at the start of a word, and
# the NUL already started this one.
r '((:)\n#foo bar)\necho rc=$?\n'
r '((:)\n#foo)\necho rc=$?\n'
# Read, not run: the substitution in the swallowed run is parsed where the scan
# meets it and never performed.
r '((:)\n$(echo X) A)\n'
r '((:)\n$(exit 9))\necho rc=$?\n'
r '((:)\n$(fi))\necho rc=$?\n'
r '((:)\n`fi`)\necho rc=$?\n'
r '((:)\n$(echo)$(fi))\n'
r '((:)\n$((1+)))\necho rc=$?\n'
r '((:)\n"unclosed)\n'
# And it is a word, not a name: no assignment is made and no keyword is met.
r '((:)\nfoo=bar)\necho rc=$? x=$foo\n'
r '((:)\nesac)\necho rc=$?\n'
r '((:)\nthen echo A)\n'
r '((:)\n*)\necho rc=$?\n'
# The swallowed run is a word like any other: its here-document operator is
# still an operator, and an alias name in it is still looked up (and lost).
r '((:)\ncat<<EOF)\nbody\nEOF\n'
r '((cat<<EOF)\n)\nbody\nEOF\n'
r '((:)\ncat<<EOF cat)\nbody\nEOF\n'
r 'shopt -s expand_aliases\nalias a=echo\n((:)\na hi)\n'
# It re-prints as nothing, being empty, and traces as nothing, never running.
r 'f() { ((1+1)\n) ; }\ndeclare -f f\n'
r 'f() { ((exit 3)\necho A) ; }\ndeclare -f f\n'
r 'set -x\n((exit 3)\n)\n'
r 'set -x\n((:)\necho one two)\n'

echo "=== 4. what still runs"
r '(( (1) ))\necho ok\n'
r '(( ((1)) ))\necho ok\n'
r '(( echo hi ) )\n'
r '(( (1 )) )\n'
r '(( a; b ) )\n'

echo done
