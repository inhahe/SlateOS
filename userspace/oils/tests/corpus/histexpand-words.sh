# readline numbers a recorded line's words with a tokenizer of its own
# (`history_tokenize_word`), not with a whitespace split. `history_word_delimiters`
# holds the shell's operator characters as well as its whitespace, so an operator
# is a word in its own right — and the scan knows just enough syntax to keep a
# quoted or substituted span whole, with several sharp edges where that knowledge
# runs out.
#
# `history -s` records an event without running it and `history -p` expands
# without running the result, so every probe below is pure: it shows which words
# a designator picked and has no other effect. That matters here, because many of
# the interesting shapes (`>>-b`, `<(…)`, `${a;b}`) are not lines one would want
# executed.
set -o history
set -H

# --- an operator is a word, with or without space around it -------------------
history -s 'echo a|cat'
history -p '!!:0' '!!:1' '!!:2' '!!:3' '!!:*' '!!:$'
history -s 'echo a > b'
history -p '!!:1' '!!:2' '!!:3'
history -s 'echo a>b'
history -p '!!:1' '!!:2' '!!:3'

# --- a doubled operator is one two-character word ----------------------------
history -s 'true&&echo b'
history -p '!!:0' '!!:1' '!!:2' '!!:3'
# `;;` too — and note the `)` of the pattern is a word of its own.
history -s 'case x in x) true;; esac'
history -p '!!:4' '!!:5' '!!:6' '!!:$'
# A third character joins a doubled `<` only.
history -s 'cat<<<hi'
history -p '!!:1' '!!:2'
history -s 'cat<<-EOF'
history -p '!!:1' '!!:2'
# …so `>>-` is the operator and then a separate `-b`.
history -s 'echo a>>-b'
history -p '!!:1' '!!:2' '!!:3'

# --- the pairs that name a file descriptor absorb one ------------------------
history -s 'echo a 2>&1'
history -p '!!:1' '!!:2' '!!:$'
history -s 'echo >&-'
history -p '!!:1' '!!:$'
# The absorbing stops at the first character that is neither digit nor `-`.
history -s 'echo >&12y'
history -p '!!:1' '!!:2'
# Their near neighbours are not pairs at all.
history -s 'echo a<>b'
history -p '!!:1' '!!:2' '!!:3' '!!:4'
history -s 'echo a;&b'
history -p '!!:1' '!!:2' '!!:3' '!!:4'

# --- a leading digit run is a file descriptor only at the start of a word -----
history -s 'echo a2>b'
history -p '!!:1' '!!:2' '!!:3'
history -s 'echo 12ab;c'
history -p '!!:1' '!!:2' '!!:3'

# --- quoting protects delimiters, and the quotes stay in the word -------------
history -s 'echo "a;b" c'
history -p '!!:1' '!!:2'
history -s "echo 'a b' c"
history -p '!!:1' '!!:2'
# A quote opened mid-word protects only to its close.
history -s "echo a'b;c'd e"
history -p '!!:1' '!!:2'
# An unterminated quote swallows the rest of the line.
history -s "echo 'a b c"
history -p '!!:1' '!!:$'
# A backslash escapes a delimiter, and an escaped quote never opens one.
history -s 'echo a\;b c'
history -p '!!:1' '!!:2'

# --- `$( … )` is scanned to its matching `)` ---------------------------------
history -s 'echo $(echo x y) z'
history -p '!!:1' '!!:2'
# A nested `$(` deepens it, and so does a bare `(` — except the one immediately
# after the opener, which readline steps over without looking at. That is the
# whole reason the next two lines disagree.
history -s 'echo $(a $(b;c) d) z'
history -p '!!:1' '!!:2'
history -s 'echo $(a (b;c) d) z'
history -p '!!:1' '!!:2'
history -s 'echo $((1+2))'
history -p '!!:1' '!!:2' '!!:$'
# `${ … }` protects nothing at all.
history -s 'echo ${a;b}'
history -p '!!:1' '!!:2' '!!:3'

# --- process substitution outranks the operator reading of `<` and `>` -------
history -s 'echo <(a;b) y'
history -p '!!:1' '!!:2'
history -s 'echo 2>(a;b) y'
history -p '!!:1' '!!:2'
# But only for a lone one: `>>(` keeps the operator and leaves the `(` behind.
history -s 'echo >>(a;b) y'
history -p '!!:1' '!!:2' '!!:3' '!!:$'

# --- a `#` that starts a word ends the line ----------------------------------
history -s 'echo a # b c'
history -p '!!:1' '!!:$'
history -s 'echo a #b c'
history -p '!!:1' '!!:$'
# One inside a word is just a character.
history -s "echo a' #'b c"
history -p '!!:1' '!!:2'

# --- `:q` and `:x` quote rather than tokenize --------------------------------
# `:x` looks like "quote each word" and is not: it wraps the whole text in one
# pair of quotes and lifts each whitespace character back out, so a doubled space
# leaves an empty item and an operator stays inside the quotes.
history -s 'echo  a;b'
history -p '!!:q' '!!:x' '!!:*:x' '!!:1:q'
history -s "echo it's"
history -p '!!:q' '!!:x'
echo done
