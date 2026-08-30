# Every row of the `hash` table is a path that was found under some particular
# `$PATH`, so the moment `$PATH` changes the table is stale. bash does not try
# to work out which rows a new `$PATH` would still reach — it throws all of
# them away, without even comparing the value, so `PATH=$PATH` empties the
# table as thoroughly as `PATH=/nowhere` does.
#
# What counts is binding the name, not mentioning its value. Everything that
# binds flushes: a plain assignment, `+=`, `unset`, an assignment used as a
# command *prefix* (which is undone again when the command finishes, and the
# table stays empty anyway), and one made inside a function. An assignment in a
# subshell of course cannot reach the parent's table.
#
# *Declaring* the name counts as binding it even when no value is named, so
# `declare PATH` flushes as surely as `declare PATH=v` does — and so do
# `typeset` and `local`, and any attribute letters riding along, including one
# the name goes on to refuse. `declare -p PATH` is a listing rather than a
# declaration and does not. `export PATH` and `readonly PATH` name neither a
# value nor a declaration and leave the table alone; assigning some *other*
# variable leaves it alone too.
#
# The table is filled here with `hash -p PATH NAME`, which files a location
# without looking for it, so every row names a path that does not exist and
# no real lookup — whose spelling differs between the two shells on this host
# — is ever printed.
#
# Every probe runs in a subshell so a table entry cannot reach the next one.
# Nothing here writes to stderr, but it is collected and replayed at the end
# anyway, so that a diagnostic appearing where there should be none is compared
# in a fixed place rather than lost.
exec 4>&2 2>err

echo "=== an assignment empties it"
( hash -p /bin/nosuch xx; PATH=/nowhere; hash; echo "  plain  rc=$?" )
( hash -p /bin/nosuch xx; PATH=$PATH; hash; echo "  same   rc=$?" )
( hash -p /bin/nosuch xx; PATH=; hash; echo "  empty  rc=$?" )
( hash -p /bin/nosuch xx; PATH+=:/nowhere; hash; echo "  append rc=$?" )
( hash -p /bin/nosuch xx; unset PATH; hash; echo "  unset  rc=$?" )

echo "=== including one that is only a command prefix, or inside a function"
( hash -p /bin/nosuch xx; PATH=/nowhere true; hash; echo "  prefix rc=$?" )
( hash -p /bin/nosuch xx; f() { PATH=/nowhere; }; f; hash; echo "  infunc rc=$?" )
( hash -p /bin/nosuch xx; f() { local PATH=/nowhere; }; f; hash; echo "  local  rc=$?" )

echo "=== and declaring the name binds it even with no value"
( hash -p /bin/nosuch xx; declare PATH=/nowhere; hash; echo "  =value rc=$?" )
( hash -p /bin/nosuch xx; declare PATH; hash; echo "  bare   rc=$?" )
( hash -p /bin/nosuch xx; typeset PATH; hash; echo "  typese rc=$?" )
( hash -p /bin/nosuch xx; declare -x PATH; hash; echo "  -x     rc=$?" )
( hash -p /bin/nosuch xx; declare -r PATH; hash; echo "  -r     rc=$?" )
( hash -p /bin/nosuch xx; declare -g PATH; hash; echo "  -g     rc=$?" )
( hash -p /bin/nosuch xx; declare -a PATH; hash; echo "  -a     rc=$?" )
( hash -p /bin/nosuch xx; f() { local PATH; }; f; hash; echo "  local  rc=$?" )
( hash -p /bin/nosuch xx; f() { declare PATH; }; f; hash; echo "  d-infn rc=$?" )

echo "=== but a mention, or a listing, is not a declaration"
( hash -p /bin/nosuch xx; export PATH; hash; echo "  export rc=$?" )
( hash -p /bin/nosuch xx; readonly PATH; hash; echo "  ronly  rc=$?" )
( hash -p /bin/nosuch xx; declare -p PATH >/dev/null; hash; echo "  -p     rc=$?" )
( hash -p /bin/nosuch xx; declare NOTPATH; hash; echo "  other  rc=$?" )
( hash -p /bin/nosuch xx; HOME=/nowhere; hash; echo "  otherv rc=$?" )
( hash -p /bin/nosuch xx; echo "$PATH" >/dev/null; hash; echo "  read   rc=$?" )

echo "=== and a subshell's assignment cannot reach the table it left"
( hash -p /bin/nosuch xx; ( PATH=/nowhere ); hash; echo "  subsh  rc=$?" )
( hash -p /bin/nosuch xx; x=$(PATH=/nowhere; echo); hash; echo "  cmdsub rc=$?" )

echo "=== the table refills after the flush, under the new PATH"
( hash -p /bin/nosuch xx; PATH=/nowhere; hash -p /bin/other yy; hash; echo "  refill rc=$?" )
( hash -p /bin/nosuch xx; PATH=/nowhere; hash -t xx; echo "  gone   rc=$?" )

exec 2>&4 4>&-
echo "=== what went to stderr"
cat err
