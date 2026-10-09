#!/usr/bin/env python3
"""Write userspace/terminfo/src/captab.rs: the terminfo compiler's tables, from the reference's libtinfo.

`tic`, `infocmp` and `toe` look capabilities up by name -- terminfo's
(`cols`) or termcap's (`co`) -- through tables ncurses generates at build
time from its `Caps` files: one entry per capability with its type and
index, a hash over them (termcap's hashes and compares a name's first two
bytes only), the aliases a source file may use (`capalias`, `infoalias`),
and the user-definable capabilities `tic -x` knows the parameters of. This
compiles a little C against the installed library -- Ubuntu 24.04's ncurses
6.4+20240113 in WSL, linked statically, since the user table's accessors
are not exported from the shared one -- that asks the library's own
lookups for every name, and writes the answers out as Rust tables:

    wsl -- python3 scripts/gen-terminfo-captab.py

The user table's size is not exported either; the program finds every
entry by walking the table's own hash chains, which between them visit
each entry once.

One table is not in the library at all: `parametrized`, which tells
`tic` how to translate each termcap string capability, is a static array
compiled from `include/Caps` by `MKparametrized.sh`. That file is read
from the reference's source -- Ubuntu's orig tarball, fetched into
`~/.cache/slateos-ncurses-src` and checked against its SHA-256 when not
already there -- with the script's own rules.

It overwrites the file and prints the counts.
"""
import hashlib
import os
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
DST = os.path.join(HERE, "..", "userspace", "terminfo", "src", "captab.rs")
LIBTINFO_A = "/usr/lib/x86_64-linux-gnu/libtinfo.a"
SRC_CACHE = os.path.expanduser("~/.cache/slateos-ncurses-src")
TARBALL = "ncurses_6.4+20240113.orig.tar.gz"
TARBALL_URL = "http://archive.ubuntu.com/ubuntu/pool/main/n/ncurses/" + TARBALL
TARBALL_SHA256 = "37a12a0f8ae2605012c9a164dd286b0cfa02b51b5055836d09eb3d597fc351b1"
CAPS_MEMBERS = ("ncurses-6.4-20240113/include/Caps", "ncurses-6.4-20240113/include/Caps-ncurses")

PROGRAM = r"""
#include <stdio.h>
#include <curses.h>
#include <term.h>

/* tic.h's, which is not installed. */
typedef short HashValue;
struct name_table_entry { const char *nte_name; int nte_type; HashValue nte_index; HashValue nte_link; };
struct alias { const char *from; const char *to; const char *source; };
struct user_table_entry { const char *ute_name; int ute_type; unsigned ute_argc; unsigned ute_args;
                          HashValue ute_index; HashValue ute_link; };
typedef struct {
    unsigned table_size;
    const HashValue *table_data;
    HashValue (*hash_of)(const char *);
    int (*compare_names)(const char *, const char *);
} HashData;

extern const struct name_table_entry *_nc_get_table(int termcap);
extern const HashValue *_nc_get_hash_table(int termcap);
extern const struct name_table_entry *_nc_find_entry(const char *, const HashValue *);
extern const struct name_table_entry *_nc_find_type_entry(const char *, int, NCURSES_BOOL);
extern const struct alias *_nc_get_alias_table(int termcap);
extern const struct user_table_entry *_nc_get_userdefs_table(void);
extern const HashData *_nc_get_hash_user(void);
struct tinfo_fkeys { unsigned offset; chtype code; };
extern const struct tinfo_fkeys _nc_tinfo_fkeys[];

static int count(NCURSES_CONST char *const *names) {
    int n = 0;
    while (names[n])
        n++;
    return n;
}

int main(void) {
    int total = count(boolnames) + count(numnames) + count(strnames);
    for (int termcap = 0; termcap <= 1; termcap++) {
        const struct name_table_entry *t = _nc_get_table(termcap);
        const HashValue *hash = _nc_get_hash_table(termcap);
        for (int i = 0; i < total; i++) {
            const struct name_table_entry *f = _nc_find_entry(t[i].nte_name, hash);
            printf("T\t%d\t%s\t%d\t%d\t%d\n", termcap, t[i].nte_name, t[i].nte_type,
                   t[i].nte_index, f ? (int) (f - t) : -1);
            const struct name_table_entry *y =
                _nc_find_type_entry(t[i].nte_name, t[i].nte_type, (NCURSES_BOOL) termcap);
            printf("Y\t%d\t%s\t%d\t%d\n", termcap, t[i].nte_name, t[i].nte_type,
                   y ? (int) (y - t) : -1);
        }
        const struct alias *a = _nc_get_alias_table(termcap);
        for (int i = 0; a[i].from; i++)
            printf("A\t%d\t%s\t%s\t%s\n", termcap, a[i].from, a[i].to ? a[i].to : "\x01",
                   a[i].source ? a[i].source : "");
    }
    const struct user_table_entry *u = _nc_get_userdefs_table();
    const HashData *h = _nc_get_hash_user();
    int last = -1;
    static char seen[4096];
    for (unsigned b = 0; b < h->table_size; b++) {
        int at = h->table_data[b];
        while (at >= 0 && at < 4096 && !seen[at]) {
            seen[at] = 1;
            if (at > last)
                last = at;
            if (u[at].ute_link < 0)
                break;
            at = u[at].ute_link + h->table_data[h->table_size];
        }
    }
    for (int i = 0; i <= last; i++)
        printf("U\t%s\t%d\t%u\t%u\t%d\t%d\n", u[i].ute_name, u[i].ute_type, u[i].ute_argc,
               u[i].ute_args, u[i].ute_index, seen[i]);
    /* `unctrl (c)` with no screen, as the compiler's messages call it. */
    for (int c = 0; c < 256; c++)
        printf("C\t%d\t%s\n", c, unctrl((chtype) c));
    /* `_nc_tinfo_fkeys`: each key code and its string capability, with
       `keyname`'s name for the code, which `tic -v` prints. */
    for (int i = 0; _nc_tinfo_fkeys[i].code; i++)
        printf("K\t%u\t%u\t%s\n", _nc_tinfo_fkeys[i].offset, (unsigned) _nc_tinfo_fkeys[i].code,
               keyname((int) _nc_tinfo_fkeys[i].code));
    return 0;
}
"""


def rust_str(s):
    out = []
    for ch in s:
        if ch in '\\"':
            out.append("\\" + ch)
        elif 0x20 <= ord(ch) < 0x7f:
            out.append(ch)
        else:
            out.append("\\x%02x" % ord(ch))
    return '"' + "".join(out) + '"'


KINDS = {0: "Kind::Boolean", 1: "Kind::Number", 2: "Kind::String"}


def caps_files():
    """The reference's `Caps` and `Caps-ncurses`: extracted from the orig
    tarball in the cache, which is fetched and checked first if need be."""
    os.makedirs(SRC_CACHE, exist_ok=True)
    tarball = os.path.join(SRC_CACHE, TARBALL)
    if not os.path.exists(tarball):
        print("fetching", TARBALL_URL)
        urllib.request.urlretrieve(TARBALL_URL, tarball)
    with open(tarball, "rb") as f:
        digest = hashlib.sha256(f.read()).hexdigest()
    if digest != TARBALL_SHA256:
        sys.exit(f"gen-terminfo-captab: {tarball} has SHA-256 {digest}, not {TARBALL_SHA256}")
    paths = []
    with tarfile.open(tarball) as tar:
        for member in CAPS_MEMBERS:
            dest = os.path.join(SRC_CACHE, member)
            if not os.path.exists(dest):
                tar.extract(member, SRC_CACHE, filter="data")
            paths.append(dest)
    return paths


def parametrized(paths):
    """`MKparametrized.sh`'s table: for each string capability, in order,
    -1 (no pad or % translation), 0 (pad only) or 1 (both)."""
    out = []
    for path in paths:
        with open(path, encoding="utf-8", errors="surrogateescape") as f:
            for line in f:
                line = line.rstrip("\n")
                if line.startswith(("#", "capalias", "infoalias", "used_by", "userdef")):
                    continue
                fields = line.split()
                if len(fields) < 3 or fields[2] != "str":
                    continue
                if fields[0].startswith(("acs_", "label_format")):
                    out.append(-1)
                elif re.search(r"#[0-9]", line):
                    out.append(1)
                else:
                    out.append(0)
    return out


def termsort(caps_path):
    """`MKtermsort.sh`'s tables: for each type, the capabilities' indices in
    terminfo-name, variable-name and termcap-name order -- `sort` of
    "NAME<tab>INDEX" lines, bytewise -- and whether each comes from termcap
    (the translation column starts `Y`), with the last index that does."""
    rows = []
    with open(caps_path, encoding="utf-8", errors="surrogateescape") as f:
        for line in f:
            if line.startswith("#"):
                continue
            fields = line.split()
            if len(fields) >= 3:
                rows.append(fields)
    out = {}
    for kind, short in (("bool", "BOOL"), ("num", "NUM"), ("str", "STR")):
        typed = [r for r in rows if r[2] == kind]
        for col, order in ((1, "TERMINFO"), (0, "VARIABLE"), (3, "TERMCAP")):
            keyed = sorted(
                ("%s\t%d" % (r[col], i)).encode("utf-8", "surrogateescape")
                for i, r in enumerate(typed)
            )
            out[f"{short}_{order}_SORT"] = [int(k.rsplit(b"\t", 1)[1]) for k in keyed]
        flags = []
        valid = 0
        for r in typed:
            first = r[6][:1] if len(r) > 6 else ""
            if first == "-":
                flags.append(False)
            elif first == "Y":
                valid = len(flags)
                flags.append(True)
        if len(flags) != len(typed):
            sys.exit(f"gen-terminfo-captab: {kind}: {len(typed)} capabilities, {len(flags)} flags")
        out[f"{short}_FROM_TERMCAP"] = flags
        out[f"OK_{short}_FROM_TERMCAP"] = valid
    return out


def main():
    with tempfile.TemporaryDirectory() as tmp:
        src = os.path.join(tmp, "captab.c")
        exe = os.path.join(tmp, "captab")
        with open(src, "w", encoding="utf-8", newline="") as f:
            f.write(PROGRAM)
        subprocess.run(["gcc", "-o", exe, src, LIBTINFO_A], check=True)
        text = subprocess.run([exe], check=True, capture_output=True, text=True,
                              encoding="utf-8").stdout
    tables = {0: [], 1: []}
    type_found = {0: {}, 1: {}}
    aliases = {0: [], 1: []}
    users = []
    unctrl = []
    fkeys = []
    for line in text.splitlines():
        f = line.split("\t")
        if f[0] == "C":
            unctrl.append(f[2])
            continue
        if f[0] == "K":
            fkeys.append((int(f[1]), int(f[2]), f[3]))
            continue
        if f[0] == "Y":
            key = (f[2], int(f[3]))
            found = int(f[4])
            if type_found[int(f[1])].setdefault(key, found) != found:
                sys.exit(f"gen-terminfo-captab: {key} found as two entries")
            continue
        if f[0] == "T":
            tables[int(f[1])].append((f[2], int(f[3]), int(f[4]), int(f[5])))
        elif f[0] == "A":
            aliases[int(f[1])].append((f[2], None if f[3] == "\x01" else f[3], f[4]))
        elif f[0] == "U":
            if f[6] != "1":
                sys.exit(f"gen-terminfo-captab: user entry {f[1]} is on no hash chain")
            users.append((f[1], int(f[2]), int(f[3]), int(f[4]), int(f[5])))
    parts = []
    for termcap, prefix, what in ((0, "INFO", "terminfo"), (1, "CAP", "termcap")):
        rows = tables[termcap]
        body = "\n".join(
            f"    Cap::new({rust_str(n)}, {KINDS[t]}, {i}),"
            for n, t, i, _ in rows
        )
        parts.append(
            f"/// `_nc_get_table ({'TRUE' if termcap else 'FALSE'})`: every capability by its {what}\n"
            f"/// name, in the table's order.\n"
            f"pub const {prefix}_TABLE: [Cap; {len(rows)}] = [\n{body}\n];\n"
        )
        found = sorted(((n, f) for n, _, _, f in rows), key=lambda x: x[0].encode())
        uniq = []
        for n, f in found:
            if uniq and uniq[-1][0] == n:
                if uniq[-1][1] != f:
                    sys.exit(f"gen-terminfo-captab: {n} found as two entries")
                continue
            uniq.append((n, f))
        body = "\n".join(f"    ({rust_str(n)}, {f})," for n, f in uniq)
        parts.append(
            f"/// What `_nc_find_entry (name, _nc_get_hash_table ({'TRUE' if termcap else 'FALSE'}))` finds for\n"
            f"/// each {what} name: its entry in [`{prefix}_TABLE`]. Sorted by name; where\n"
            f"/// two capabilities share a name, the one the library's hash chain gives first.\n"
            f"pub const {prefix}_FIND: [(&str, usize); {len(uniq)}] = [\n{body}\n];\n"
        )
        typed = sorted(type_found[termcap].items(), key=lambda kv: (kv[0][0].encode(), kv[0][1]))
        body = "\n".join(
            f"    ({rust_str(n)}, {KINDS[t]}, {f})," for (n, t), f in typed
        )
        parts.append(
            f"/// What `_nc_find_type_entry (name, type, {'TRUE' if termcap else 'FALSE'})` finds for each\n"
            f"/// {what} name and type: its entry in [`{prefix}_TABLE`]. Sorted by name, then\n"
            f"/// type; where two capabilities share both, the one the library's hash chain\n"
            f"/// gives first.\n"
            f"pub const {prefix}_TYPE_FIND: [(&str, Kind, usize); {len(typed)}] = [\n{body}\n];\n"
        )
        body = "\n".join(
            f"    Alias::new({rust_str(fr)}, {('Some(' + rust_str(to) + ')') if to is not None else 'None'}, {rust_str(so)}),"
            for fr, to, so in aliases[termcap]
        )
        parts.append(
            f"/// `_nc_get_alias_table ({'TRUE' if termcap else 'FALSE'})`: the {what} names a source may\n"
            f"/// use for another (`to`), or that it should drop (`None`), and whose they are.\n"
            f"pub const {prefix}_ALIASES: [Alias; {len(aliases[termcap])}] = [\n{body}\n];\n"
        )
        print(f"{what}: {len(rows)} capabilities, {len(uniq)} names, {len(aliases[termcap])} aliases")
    body = "\n".join(
        f"    UserCap::new({rust_str(n)}, {t}, {c}, {a}, {i}),"
        for n, t, c, a, i in users
    )
    parts.append(
        "/// `_nc_get_userdefs_table ()`: the user-definable capabilities ncurses knows\n"
        "/// (`include/Caps-ncurses`), in the table's order -- a name of two types\n"
        "/// holding both in its mask.\n"
        f"pub const USER_TABLE: [UserCap; {len(users)}] = [\n{body}\n];\n"
    )
    print(f"user-definable: {len(users)}")
    caps = caps_files()
    sorts = termsort(caps[0])
    for name, value in sorts.items():
        if isinstance(value, int):
            parts.append(
                f"/// `{name}`: the last index whose capability comes from termcap.\n"
                f"pub const {name}: usize = {value};\n"
            )
        elif value and isinstance(value[0], bool):
            body = "\n".join(f"    {'true' if v else 'false'}," for v in value)
            parts.append(
                f"/// `{name.lower()}`: whether each capability (by index) comes from\n"
                f"/// termcap, from `include/Caps` by `MKtermsort.sh`'s rules.\n"
                f"pub const {name}: [bool; {len(value)}] = [\n{body}\n];\n"
            )
        else:
            body = "\n".join(f"    {v}," for v in value)
            parts.append(
                f"/// `{name.lower()}`: the capabilities' indices in that name order,\n"
                f"/// from `include/Caps` by `MKtermsort.sh`'s rules.\n"
                f"pub const {name}: [usize; {len(value)}] = [\n{body}\n];\n"
            )
    print("termsort:", ", ".join(f"{k}={len(v) if isinstance(v, list) else v}" for k, v in sorts.items()))
    params = parametrized(caps)
    strings = sum(1 for n, t, _, _ in tables[0] if t == 2)
    if len(params) != strings:
        sys.exit(f"gen-terminfo-captab: {len(params)} parametrized entries, {strings} strings")
    body = "\n".join(f"    {p}," for p in params)
    parts.append(
        "/// `parametrized`, from `include/Caps` by `MKparametrized.sh`'s rules: how\n"
        "/// `tic` translates each string capability (by its index) from termcap --\n"
        "/// -1 not at all, 0 its padding only, 1 its padding and its `%` codes.\n"
        f"pub const PARAMETRIZED: [i8; {len(params)}] = [\n{body}\n];\n"
    )
    print(f"parametrized: {len(params)}")
    body = "\n".join(f"    ({o}, {c}, {rust_str(n)})," for o, c, n in fkeys)
    parts.append(
        "/// `_nc_tinfo_fkeys`: each function key's string capability (by index)\n"
        "/// and curses key code, in the table's order, with `keyname`'s name for\n"
        "/// the code.\n"
        f"pub const TINFO_FKEYS: [(usize, u32, &str); {len(fkeys)}] = [\n{body}\n];\n"
    )
    print(f"function keys: {len(fkeys)}")
    if len(unctrl) != 256:
        sys.exit(f"gen-terminfo-captab: {len(unctrl)} unctrl entries, not 256")
    body = "\n".join(f"    {rust_str(s)}," for s in unctrl)
    parts.append(
        "/// `unctrl (c)` with no curses screen, for every byte: `^X` for a control\n"
        "/// character, `~X` for the eight-bit ones, the character itself otherwise.\n"
        f"pub const UNCTRL: [&str; 256] = [\n{body}\n];\n"
    )
    header = (
        "//! The terminfo compiler's tables: generated from the reference's libtinfo\n"
        "//! (Ubuntu 24.04's ncurses 6.4+20240113) by `scripts/gen-terminfo-captab.py`,\n"
        "//! which asks the library's own lookups for every name. Not edited by hand:\n"
        "//! rerun the script.\n\n"
        "use crate::Kind;\n\n"
        "/// One capability of a name table: its name there, its type, its index\n"
        "/// among its type's capabilities in `term.h`.\n"
        "#[derive(Clone, Copy, Debug, PartialEq, Eq)]\n"
        "pub struct Cap {\n"
        "    /// The name.\n"
        "    pub name: &'static str,\n"
        "    /// The type.\n"
        "    pub kind: Kind,\n"
        "    /// The index.\n"
        "    pub index: usize,\n"
        "}\n\n"
        "impl Cap {\n"
        "    /// A table row -- a call, so that each stays one line.\n"
        "    #[must_use]\n"
        "    pub const fn new(name: &'static str, kind: Kind, index: usize) -> Self {\n"
        "        Self { name, kind, index }\n"
        "    }\n"
        "}\n\n"
        "/// One alias of a source file's.\n"
        "#[derive(Clone, Copy, Debug, PartialEq, Eq)]\n"
        "pub struct Alias {\n"
        "    /// The name as written.\n"
        "    pub from: &'static str,\n"
        "    /// The name it stands for; `None` to drop it.\n"
        "    pub to: Option<&'static str>,\n"
        "    /// Whose name it is (`AT&T`, `BSD`...).\n"
        "    pub source: &'static str,\n"
        "}\n\n"
        "impl Alias {\n"
        "    /// A table row.\n"
        "    #[must_use]\n"
        "    pub const fn new(from: &'static str, to: Option<&'static str>, source: &'static str) -> Self {\n"
        "        Self { from, to, source }\n"
        "    }\n"
        "}\n\n"
        "/// One user-definable capability.\n"
        "#[derive(Clone, Copy, Debug, PartialEq, Eq)]\n"
        "pub struct UserCap {\n"
        "    /// Its name.\n"
        "    pub name: &'static str,\n"
        "    /// Its types: a bit for each of `BOOLEAN` (1), `NUMBER` (2), `STRING` (4).\n"
        "    pub kinds: u8,\n"
        "    /// How many parameters it takes.\n"
        "    pub argc: u32,\n"
        "    /// Which of them are strings, a bit each.\n"
        "    pub args: u32,\n"
        "    /// Its index in its own table.\n"
        "    pub index: usize,\n"
        "}\n\n"
        "impl UserCap {\n"
        "    /// A table row.\n"
        "    #[must_use]\n"
        "    pub const fn new(name: &'static str, kinds: u8, argc: u32, args: u32, index: usize) -> Self {\n"
        "        Self { name, kinds, argc, args, index }\n"
        "    }\n"
        "}\n\n"
    )
    with open(DST, "w", encoding="utf-8", newline="\n") as f:
        f.write(header + "\n".join(parts))
    # As rustfmt lays it out, which is what the format gate holds the tree to.
    rustfmt = shutil.which("rustfmt") or os.path.expanduser("~/.cargo/bin/rustfmt")
    if not os.path.exists(rustfmt):
        sys.exit("gen-terminfo-captab: rustfmt is needed to lay out the result, and is not here")
    subprocess.run([rustfmt, "--edition", "2024", DST], check=True)
    print("wrote", os.path.normpath(DST))
    return 0


if __name__ == "__main__":
    sys.exit(main())
