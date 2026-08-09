# A construct left open in text no parser read cannot be a *parse* error — no
# parser read it. The scanner that meets it is the expansion-time one in
# subst.c, so the failure happens when the word is expanded, kills only the
# enclosing command, and lets the script carry on. Two reporters share the job:
#
#   * `$( … )` is not scanned for at all: `extract_command_subst` hands
#     `xparse_dolparen` (parse.y:4256) everything from the `$(` to the end of
#     the text and lets a real parse decide where the body stops. With no `)`
#     to stop at, that parse always fails — at end of input with
#     `shell_eof_token` outstanding (`unexpected EOF while looking for matching
#     `)'`, parse.y:6289), or sooner on a token or on a construct of the body's
#     own. The message carries the `command substitution` input name, and its
#     line is the shell's line plus the 1-based line of the failure *within the
#     body*, because `parse_string` does `push_stream (0)` and never renumbers.
#
#   * `${ … }`, `$(( … ))`, `$[ … ]` and `` ` … ` `` are found by
#     `extract_dollar_brace_string` (subst.c:1785, 1980),
#     `extract_delimited_string` (subst.c:1498) and `param_expand`'s backquote
#     arm (subst.c:11290). All four say `bad substitution: no closing `%s' in
#     %s` at the shell's plain line, then `exp_jump_to_top_level (DISCARD)`. The
#     `%s` is the **whole text being expanded**, not the construct — except at
#     the backquote site, which prints only what follows the backtick.
#
# Which one fires is decided by whichever scan gives up first, reading left to
# right and innermost-out: a `$(` *inside* an unterminated `${ … }` is met by
# the brace scan and reports as a command substitution. But only a scan that
# carries `SX_COMMAND` reaches for one — `$[` (subst.c:1303) does not, so it
# steps over a `$(` as so much text and keeps its own complaint.
#
# An unterminated *quote* is the control: it is not a construct the scanner has
# to close, so it is simply text.
#
# Verified against bash 5.2.37.

r() { printf '%-30s -> ' "$1"; ( eval "$2" ) 2>&1 | sed 's/^[^:]*: //' | cat -A; }

echo "=== each construct, alone in a here-document body"
r 'a $( )'                  $'cat <<E\n$(echo\nE\necho after=$?'
r 'a ${ }'                  $'cat <<E\n${x:-a\nE\necho after=$?'
r 'a backquote'             $'cat <<E\n`echo\nE\necho after=$?'
r 'a $(( ))'                $'cat <<E\n$((1+\nE\necho after=$?'
r 'a $[ ]'                  $'cat <<E\n$[1+\nE\necho after=$?'
r 'a quote is just text'    $'cat <<E\n"abc\nE\necho after=$?'
r 'a quoted delimiter'      $'cat <<\'E\'\n${x:-a\nE\necho after=$?'

echo "=== the echoed text is the whole body, not the construct"
r 'text before it'          $'cat <<E\nq${x:-a\nE\necho after=$?'
r 'a whole sub before it'   $'cat <<E\n$(echo hi) ${x:-a\nE\necho after=$?'
r 'a whole sub after it'    $'cat <<E\n${x:-a $(echo hi)\nE\necho after=$?'
r 'three body lines'        $'cat <<E\nx\n${y:-a\nz\nE\necho after=$?'
r 'the backquote exception' $'cat <<E\npre `echo\nE\necho after=$?'

echo "=== whichever scan gives up first wins"
r 'a ${ } holding a $('     $'cat <<E\n${x:-a\n$(echo\nE\necho after=$?'
r 'a $( holding a ${ }'     $'cat <<E\n$(echo\n${x:-a\nE\necho after=$?'
r 'a ${ } holding a `'      $'cat <<E\n${x:-`echo\nE\necho after=$?'
r 'two ${ }'                $'cat <<E\n${y:-b ${x:-a\nE\necho after=$?'
# `$((` carries SX_COMMAND and `$[` does not, so only the first reads the `$(`.
r 'a $(( )) holding a $('   $'cat <<E\n$((1+$(echo\nE\necho after=$?'
r 'a $[ ] holding a $('     $'cat <<E\n$[1+$(echo\nE\necho after=$?'
r 'a $( holding a backquote' $'cat <<E\n$(`echo\nE\necho after=$?'

echo "=== the \$( ) line counts the newlines the read consumed"
r 'body line 1'             $'cat <<E\n$(echo\nE\necho after=$?'
r 'body line 2 of 3'        $'echo one\ncat <<E\nx\n$(echo\ny\nE\necho after=$?'

echo "=== the body's own failure comes first, and names a token"
r 'a token in the body'     $'cat <<E\n$(fi\nE\necho after=$?'
r 'the echoed line'         $'cat <<E\n$(echo a\nb ;;\nc\nE\necho after=$?'
r 'an unclosed quote in it' $'cat <<E\n$(echo \'x\nE\necho after=$?'
# A `$( … )` *inside* the body is `parse_comsub`'s own error, and that one is
# `jump_to_top_level (FORCE_EOF)` (parse.y:4185) — the shell stops outright.
r 'a nested $( ) is fatal'  $'cat <<E\n$(echo $(fi) \nE\necho after=$?'

echo "=== everything before the failure has already happened"
r 'side effects still run'  $'cat <<E\n$(echo hi >&2) $(echo\nE\necho after=$?'
# …but not inside the construct that failed: its scan never completed, so
# nothing in it was ever expanded.
r 'nothing inside it does'  $'cat <<E\n${x:-$(echo hi >&2)a\nE\necho after=$?'

echo "=== errexit promotes the diagnostic; posix mode does not"
# All five sites report through `report_error`, whose whole promotion rule is
# `if (exit_immediately_on_error) exit_shell (…)` (error.c:201). None of them
# consults `posixly_correct`, unlike the assignment-error paths that do.
r 'plain'                   $'cat <<E\n${x:-a\nE\necho after=$?'
r 'set -e'                  $'set -e\ncat <<E\n${x:-a\nE\necho after=$?'
r 'set -o posix'            $'set -o posix\ncat <<E\n${x:-a\nE\necho after=$?'

echo "=== and a word the parser *did* read is still a parse error"
r 'a here-string'           $'cat <<< "${x:-a"\necho after=$?'
r 'an ordinary word'        $'echo "${x:-a"\necho after=$?'

echo "=== the text prints back as written, there being no re-print"
f() { cat <<E
$(echo
E
}
declare -f f

echo "=== a value expanded as a prompt reports it and carries on"
# `expand_prompt_string` raises `no_longjmp_on_fatal_error`, which every one of
# these sites tests: the `else` branch sets `*sindex` past the text and returns
# NULL instead of jumping, and what the caller makes of the NULL differs per
# construct — a `${ }` becomes the caller's own `bad substitution`, a backquote
# keeps its `no closing` message, and a `$(( ))` says nothing at all.
v='a${x:-b'; printf '@P brace : [%s] rc=%s\n' "${v@P}" "$?"
w='a`echo';  printf '@P bquote: [%s] rc=%s\n' "${w@P}" "$?"
u='a$((1+';  printf '@P arith : [%s] rc=%s\n' "${u@P}" "$?"
# `$( )` is the one that carries on to *run* the body — and to a body that
# never closed, `xparse_dolparen`'s `substring (ostring, 0, nc - 1)` (which is
# there to drop the `)`) costs a real character instead. Hence `ech`.
t='a$(echo'; printf '@P cmdsub: [%s] rc=%s\n' "${t@P}" "$?"
( PS4='${x:-a'; set -x; : m; set +x ) 2>&1
echo "prompt rc=$?"

echo done
