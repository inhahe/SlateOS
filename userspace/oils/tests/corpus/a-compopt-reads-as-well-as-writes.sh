# `compopt` is documented as the way to change a completion spec's `-o` options
# while a completion function is running, and that is what it is written for —
# but a script can call it anywhere, and two things about it are then visible
# that the description does not suggest.
#
# The first is that with neither `-o` nor `+o` it does not change anything: it
# *reports*, one line per target, and the line it writes is a `compopt` command
# that would restore the state — which is why it spells out the options that are
# off as well as the ones that are on.
#
# The second is that its option scan is the same clustered getopt `complete`
# uses, with `+` opening an option word as readily as `-`. So `-onospace` and
# `-Do nospace` are ordinary spellings, a word that is not an option ends the
# options, and `+D` sets the very flag `-D` does, because only `o` ever asks
# which sign it arrived with.

echo "=== with no -o/+o it reports rather than changes"
complete -o nospace -o dirnames -W 1 dd
compopt dd; echo "  rc=$?"
complete -p dd
complete -r
# Every option is named either way, so the line restores the state whole.
complete -o bashdefault -o default -o dirnames -o filenames -o noquote -o nosort -o nospace -o plusdirs -W 1 ee
compopt ee
complete -W 1 ff
compopt ff
complete -r
# The specials report too, under the flag they were defined with.
complete -o nospace -W 1 -D
compopt -D; echo "  rc=$?"
complete -r -D

echo "=== targets are answered in the order they were written"
# Not the table's order, and not deduplicated — so a repeat is answered twice
# and a diagnostic lands between the lines around it.
complete -o nospace -W 1 c8
complete -W 1 c174
compopt c174 c8 nope; echo "  rc=$?"
compopt c8 c8; echo "  twice rc=$?"
complete -r
# The report is stdout; the complaint is not.
complete -W 1 gg
compopt gg nope 2>/dev/null; echo "  out rc=$?"
compopt gg nope 1>/dev/null; echo "  err rc=$?"
complete -r
# A name that needs quoting is quoted, as it is in `complete -p`.
complete -W 1 'a b'; compopt 'a b'
complete -W 1 ''; compopt ''
complete -r

echo "=== the option scan clusters, and both signs open one"
complete -W 1 zz
compopt -onospace zz; echo "  rc=$?"; complete -p zz
compopt +onospace zz; echo "  rc=$?"; complete -p zz
complete -r
# `-Do nospace` is `-D` then `-o`, whose value is the next word.
complete -W 1 -D
compopt -Do nospace; echo "  rc=$?"; complete -p -D
# `+D` is `-D`: only `o` reads the sign.
compopt +D; echo "  +D rc=$?"
complete -r -D

echo "=== a word that is not an option ends them"
complete -W 1 nn
compopt nn -D; echo "  rc=$?"
compopt nn -o nospace; echo "  rc=$?"
complete -p nn; complete -p -- -o; echo "  o rc=$?"
complete -r
# A lone `-` or `+` is a word, so it is a name.
complete -W 1 zz
compopt - zz; echo "  dash rc=$?"
compopt + zz; echo "  plus rc=$?"
complete -r
# `--` ends the options without being one, so the dash-name after it is a name.
complete -W 1 -- -q
compopt -- -q; echo "  rc=$?"
complete -r

echo "=== only the first of -D -E -I counts, and it replaces the names"
complete -W 1 -D; complete -W 1 -E
compopt -DE; echo "  DE rc=$?"
compopt -ED; echo "  ED rc=$?"
compopt -I -E; echo "  IE rc=$?"
complete -W 1 nn2
compopt -D nn2; echo "  D+name rc=$?"
complete -r; complete -r -D; complete -r -E

echo "=== the edits themselves"
complete -W 1 hh
compopt -o nospace +o nospace hh; echo "  rc=$?"; complete -p hh
compopt +o nospace -o nospace hh; echo "  rc=$?"; complete -p hh
compopt -o nospace -o nospace hh; echo "  rc=$?"; complete -p hh
compopt +o plusdirs hh; echo "  unset rc=$?"; complete -p hh
complete -r
# An option name is checked as it is read, so a bad one later stops the whole
# call and nothing at all is applied.
complete -W 1 ii
compopt -o nospace -o nosuch ii; echo "  rc=$?"; complete -p ii
compopt -oz ii; echo "  cluster rc=$?"
complete -r

echo "=== the usage errors"
complete -W 1 zz
compopt -Z zz; echo "  rc=$?"
compopt +Z zz; echo "  rc=$?"
compopt -Do; echo "  rc=$?"
compopt +o; echo "  rc=$?"
complete -r
# No target at all is the "not currently executing" error, whatever options
# were given, because osh is never inside a completion function.
compopt; echo "  bare rc=$?"
compopt -o nospace; echo "  opt rc=$?"
# A missing spec is status 1, named by its internal spelling when it is special.
compopt -D; echo "  D rc=$?"
compopt nope; echo "  name rc=$?"
