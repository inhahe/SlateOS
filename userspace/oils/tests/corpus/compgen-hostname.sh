# `compgen -A hostname` reads `$HOSTFILE`, a file in `/etc/hosts` format.
#
# `HOSTFILE` is kept set throughout: with it unset the list comes from the real
# `/etc/hosts`, whose contents are this machine's, not this case's — and the two
# shells do not even resolve that path the same way, osh being a native Windows
# build. Everything the case is actually about (what a line contributes, and how
# the kept list changes) is visible with `HOSTFILE` pointing at a file it wrote
# itself.

printf '1.2.3.4 alpha alpha.example\n' > a.hosts
printf '# a whole-line comment\n' >> a.hosts
printf '5.6.7.8\tbeta  gamma # and a trailing one\n' >> a.hosts
printf '\n' >> a.hosts
printf '9.9.9.9\n' >> a.hosts
printf '10.0.0.1 alpha\n' >> a.hosts
printf '1.1.1.1 one\n' > b.hosts
printf '2.2.2.2 two\n' > c.hosts

echo "== the first field is the address; the rest are names"
# A comment goes wherever it starts, so `gamma` counts and the words after `#`
# do not; an address with no names, and a blank line, contribute nothing; and
# `alpha` is offered twice because two lines name it.
HOSTFILE=a.hosts
compgen -A hostname
echo "rc=$?"

echo "== the word narrows it like any other action"
compgen -A hostname al
echo "rc=$?"
compgen -A hostname zz
echo "rc=$?"

echo "== naming another file adds to the list rather than replacing it"
HOSTFILE=b.hosts
compgen -A hostname

echo "== …but unsetting it throws the list away first"
unset HOSTFILE
HOSTFILE=c.hosts
compgen -A hostname

echo "== a file that cannot be read contributes nothing, and is not an error"
HOSTFILE=nosuch.hosts
compgen -A hostname
echo "rc=$?"

echo "== -P/-S and -X apply, and the action takes its place in the order"
HOSTFILE=b.hosts
compgen -A hostname -P '<' -S '>'
compgen -A hostname -X 'o*'
echo "rc=$?"
compgen -A hostname -W 'wone' -k -X '[cfw]*'
