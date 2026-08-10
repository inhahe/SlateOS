# A `(( … ))` whose two closing parentheses are not adjacent is handed back to
# the ordinary grammar as `( ( … ) )` — see
# an-arithmetic-command-needs-its-closing-parentheses-adjacent.sh for *that*
# rule. This file pins how the hand-back is done, which is visible in three
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
# The pushes stack, as `push_string`'s list does: a `((` met while a copy is
# being read pushes its own copy on top, and the innermost is what gets echoed.
#
# The push is the *last* thing the scan does, so anything the scan itself
# reported happened while `shell_input_line` was still the physical line. That
# is one thing in practice: a `$( … )` body, which `parse_matched_pair` hands to
# `extract_command_subst` as it goes past — see group 1b.
#
# Not pinned here: a copy that has to reproduce a `\<newline>` the reader
# already deleted, which it cannot — bash's own re-parse desynchronises there —
# and the exit status a copy ending on a newline leaves behind. Nor is this:
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

echo "=== 4. what still runs"
r '(( (1) ))\necho ok\n'
r '(( ((1)) ))\necho ok\n'
r '(( echo hi ) )\n'
r '(( (1 )) )\n'
r '(( a; b ) )\n'

echo done
