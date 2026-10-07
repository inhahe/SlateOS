"""libxcrypt 4.4.36's crypt -- Ubuntu 24.04's libcrypt.so.1 -- as the oracle
for posix/src/crypt.rs and posix/src/yescrypt.rs, and the hashing behind
them in posix/pwhash.

    python posix/tools/oracle/crypt_harness.py   # writes posix/src/crypt_oracle.txt

glibc 2.39 has no crypt of its own; Ubuntu's comes from libxcrypt, which is
the library posix's `crypt` answers as (`crypt.rs`'s module docs). So the
oracle here is libxcrypt, asked through `crypt` itself: its failure tokens
and errno included.

One line a call: `<password> <setting> = <result> <errno>`. The password is
its bytes in hex, or `-` for none; the setting and the result are as given
and as returned, none of them holding a space; errno is its name, or `0`.

The cases:

- libxcrypt's own known-answer table's 92 passwords (test/ka-table.inc in
  its source) under that table's settings for the methods posix implements
  -- MD5, SHA-256, SHA-512, scrypt, yescrypt and bcrypt's four variants;
- bcrypt's variants over keys built for their key setup, its 72-byte key,
  its salt's last character and the settings it refuses;
- yescrypt and scrypt settings that reach each parameter -- flavour, N, r,
  p, t -- and each of `yescrypt_kdf`'s paths: classic scrypt and WORM lanes
  one by one, RW lanes sharing V, the prehash a large hash starts with;
- salts of every length a `$y$` salt can decode from, and the ones it
  cannot; passwords of every length to the 511-byte limit, and past it;
- settings each check refuses, in libxcrypt's order.

Left out: a `$7$` setting long enough for libxcrypt's 384 bytes of output and
not posix's 256 (`crypt.rs`, "The output's room"), and any setting asking
for more memory than a test machine has -- posix's tests take those on
separately.
"""

import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from _wsl import POSIX_SRC, run, workdir, wsl_path  # noqa: E402

OUT = POSIX_SRC / "crypt_oracle.txt"

ITOA64 = "./0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz"

# libxcrypt 4.4.36's test/ka-table.inc: the passwords, in its order.
KA_PASSWORDS = [
    b'',
    b' ',
    b'a',
    b'ab',
    b'abc',
    b'U*U',
    b'U*U*',
    b'U*U*U',
    b'.....',
    b'dragon',
    b'dRaGoN',
    b'DrAgOn',
    b'PAROLX',
    b'U*U***U',
    b'abcdefg',
    b'01234567',
    b'726 even',
    b'zyxwvuts',
    b'ab1234567',
    b'alexander',
    b'beautiful',
    b'challenge',
    b'chocolate',
    b'cr1234567',
    b'katherine',
    b'stephanie',
    b'sunflower',
    b'basketball',
    b'porsche911',
    b'|_337T`/p3',
    b'thunderbird',
    b'Hello world!',
    b'pleaseletmein',
    b'a short string',
    b'zxyDPWgydbQjgq',
    b'photojournalism',
    b'ecclesiastically',
    b'congregationalism',
    b'dihydrosphingosine',
    b'semianthropological',
    b'palaeogeographically',
    b'electromyographically',
    b'noninterchangeableness',
    b'abcdefghijklmnopqrstuvwxyz',
    b'electroencephalographically',
    b'antidisestablishmentarianism',
    b'cyclotrimethylenetrinitramine',
    b'dichlorodiphenyltrichloroethane',
    b'multiple words seperated by spaces',
    b'supercalifragilisticexpialidocious',
    b'we have a short salt string but not a short password',
    b'multiple word$ $eperated by $pace$ and $pecial character$',
    b'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789',
    b'12345678901234567890123456789012345678901234567890123456789012345678901234567890',
    b'a very much longer text to encrypt.  This one even stretches over morethan one line.',
    b'\xd0\xc1\xd2\xcf\xcc\xd8',
    b'\xd5\xaa\xd5\xaa\xaa\xaa\xd5\xaa',
    b'\xe1\xec\xe5\xf8\xe1\xee\xe4\xe5\xf2',
    b'\xf3\xf4\xe5\xf0\xe8\xe1\xee\xe9\xe5',
    b'\xaa\xd5\xaa\xd5\xaa\xd5\xaa\xd5\xaa\xd5\xaa\xd5\xaa\xd5\xaa\xd5\xaa',
    b'\xc3\xa9tude',
    b'C)tude',
    b'Chl\xc3\xb6e',
    b'ChlC6e',
    b'\xc3\x85ngstr\xc3\xb6m',
    b'C\x05ngstrC6m',
    b'C\x05ngstrCU*U***U*',
    b'U*U***U*ignored',
    b'U*U*U*U*',
    b'U*U*U*U*ignored',
    b'*U*U*U*U',
    b'*U*U*U*U*',
    b'*U*U*U*U*U*U*U*U',
    b'*U*U*U*U*U*U*U*U*',
    b'\xa3',
    b'\xa3a',
    b'\xd1\x91',
    b'\xa3ab',
    b'\xff\xff\xa3',
    b'1\xa3345',
    b'\xff\xa3345',
    b'\xff\xa334\xff\xff\xff\xa3345',
    b'U\xaa\xff' * 24,
    b'\xaaU' * 36,
    b'0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789',
    b'0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789chars after 72 are ignored',
    b'\xaa' * 72,
    b'\xaa' * 72 + b'chars after 72 are ignored as usual',
    b'THE YEAR 1866 was marked by a bizarre development, an unexplained and downright inexplicable phenomenon that surely no one has forgotten.',
    b'THE YEAR',
    b'THE YEAR 1866 was marked by a bizarre development, an unexplained and do',
    b'THE YEAR 1866 was marked by a bizarre development, an unexplained and downright inexplicable phenomenon that surely no one has f',
]

# The table's settings for the methods posix implements.
KA_SETTINGS = [
    "$1$CCCCCCCC",
    "$1$abcdefgh",
    "$5$rounds=1000$saltstring",
    "$5$rounds=1000$short",
    "$5$saltstring",
    "$5$short",
    "$6$rounds=1000$saltstring",
    "$6$rounds=1000$short",
    "$6$saltstring",
    "$6$short",
    "$7$66..../....SodiumChloride",
    "$7$66..../....unUNunUNunUNun",
    "$7$76..../....SodiumChloride",
    "$7$76..../....unUNunUNunUNun",
    "$y$j75$.......",
    "$y$j75$LdJMENpBABJJ3hIHjB1Bi.",
    "$y$j85$.......",
    "$y$j85$LdJMENpBABJJ3hIHjB1Bi.",
    "$2a$04$CCCCCCCCCCCCCCCCCCCCC.",
    "$2a$04$abcdefghijklmnopqrstuu",
    "$2a$05$CCCCCCCCCCCCCCCCCCCCC.",
    "$2a$05$abcdefghijklmnopqrstuu",
    "$2b$04$CCCCCCCCCCCCCCCCCCCCC.",
    "$2b$04$abcdefghijklmnopqrstuu",
    "$2b$05$CCCCCCCCCCCCCCCCCCCCC.",
    "$2b$05$abcdefghijklmnopqrstuu",
    "$2x$04$CCCCCCCCCCCCCCCCCCCCC.",
    "$2x$04$abcdefghijklmnopqrstuu",
    "$2x$05$CCCCCCCCCCCCCCCCCCCCC.",
    "$2x$05$abcdefghijklmnopqrstuu",
    "$2y$04$CCCCCCCCCCCCCCCCCCCCC.",
    "$2y$04$abcdefghijklmnopqrstuu",
    "$2y$05$CCCCCCCCCCCCCCCCCCCCC.",
    "$2y$05$abcdefghijklmnopqrstuu",
]

# bcrypt's own base-64 alphabet: crypt's characters, in another order.
BF_ITOA64 = "./ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789"


def enc_u32(src: int, minimum: int) -> str:
    """alg-yescrypt-common.c's encode64_uint32: a number in the code whose
    first character says how many follow."""
    start, end, chars, bits = 0, 47, 1, 0
    src -= minimum
    while True:
        count = (end + 1 - start) << bits
        if src < count:
            break
        start = end + 1
        end = start + (62 - end) // 2
        src -= count
        chars += 1
        bits += 6
    out = ITOA64[start + (src >> bits)]
    while chars > 1:
        chars -= 1
        bits -= 6
        out += ITOA64[(src >> bits) & 0x3F]
    return out


def enc_fixed(src: int, nbits: int) -> str:
    """encode64_uint32_fixed: `nbits` bits, six a character, least first."""
    out = ""
    for _ in range(0, nbits, 6):
        out += ITOA64[src & 0x3F]
        src >>= 6
    return out


def enc64(data: bytes) -> str:
    """encode64: three bytes to four characters, least significant first."""
    out = ""
    for i in range(0, len(data), 3):
        group = data[i:i + 3]
        value = int.from_bytes(group, "little")
        for _ in range(0, 8 * len(group), 6):
            out += ITOA64[value & 0x3F]
            value >>= 6
    return out


def y(flavor, n_log2, r, p=1, t=0, g=0, nrom_log2=0, salt="LdJMENpBABJJ3hIHjB1Bi."):
    """A `$y$` setting, as yescrypt_encode_params_r writes one."""
    s = "$y$" + enc_u32(flavor, 0) + enc_u32(n_log2, 1) + enc_u32(r, 1)
    have = (p != 1) | (bool(t) << 1) | (bool(g) << 2) | (bool(nrom_log2) << 3)
    if have:
        s += enc_u32(have, 1)
        if p != 1:
            s += enc_u32(p, 2)
        if t:
            s += enc_u32(t, 1)
        if g:
            s += enc_u32(g, 1)
        if nrom_log2:
            s += enc_u32(nrom_log2, 1)
    return s + "$" + salt


def scrypt(n_log2, r, p, salt="SodiumChloride"):
    """A `$7$` setting, as gensalt_scrypt_rn writes one."""
    return "$7$" + ITOA64[n_log2] + enc_fixed(r, 30) + enc_fixed(p, 30) + salt


RW = 47  # flavour 'j': YESCRYPT_RW with libxcrypt's one pwxform flavour
WORM = 1
CLASSIC = 0


def generated():
    """(password, setting) pairs reaching what the table does not."""
    pw = b"pleaseletmein"
    cases = []

    # Each mode over N, r, p and t.
    for n_log2, r, p in [(2, 1, 1), (3, 1, 1), (4, 1, 1), (4, 2, 1), (5, 3, 1), (8, 1, 1),
                         (8, 8, 1), (10, 1, 1), (4, 1, 2), (6, 2, 3), (8, 4, 4), (9, 1, 7)]:
        cases.append((pw, y(CLASSIC, n_log2, r, p)))
    for n_log2, r, p, t in [(2, 1, 1, 0), (2, 1, 1, 1), (2, 1, 1, 2), (2, 1, 1, 3), (8, 8, 1, 0),
                            (8, 8, 1, 1), (8, 8, 1, 5), (6, 2, 3, 0), (6, 2, 3, 2), (9, 1, 7, 1)]:
        cases.append((pw, y(WORM, n_log2, r, p, t)))
    for n_log2, r, p, t in [(2, 1, 1, 0), (3, 1, 1, 0), (4, 1, 1, 0), (4, 1, 1, 1), (4, 1, 1, 2),
                            (4, 1, 1, 3), (5, 1, 1, 0), (5, 2, 1, 0), (6, 3, 1, 0), (8, 1, 1, 0),
                            (8, 8, 1, 0), (8, 8, 1, 1), (8, 8, 1, 4), (10, 8, 1, 0), (10, 16, 1, 0),
                            (4, 1, 2, 0), (5, 1, 2, 0), (5, 1, 3, 0), (6, 2, 3, 0), (6, 2, 3, 1),
                            (7, 1, 5, 0), (8, 4, 4, 2), (9, 2, 6, 0), (10, 1, 9, 3),
                            # N/p not a power of 2, and the last lane longer.
                            (6, 1, 3, 0), (7, 2, 6, 1)]:
        cases.append((pw, y(RW, n_log2, r, p, t)))

    # The prehash: N/p at least 256 and N/p * r at least 2^17.
    cases.append((pw, y(RW, 12, 32, salt="PKXc3hCOSyMqdaEQArI62/")))  # Ubuntu's $y$j9T$
    cases.append((b"", y(RW, 12, 32, salt="MJHnaAkegEVYHsFKkmfzJ1")))
    cases.append((pw, y(RW, 10, 128)))
    cases.append((pw, y(RW, 8, 512)))
    cases.append((pw, y(RW, 11, 128, p=2)))
    cases.append((pw, y(RW, 12, 32, t=1)))
    cases.append((pw, y(RW, 12, 31)))  # just below the prehash
    cases.append((pw, y(RW, 9, 255, p=2)))  # N/p * r just below it

    # Passwords: each length that crosses an HMAC block, the longest, one more.
    for n in [1, 31, 32, 33, 63, 64, 65, 127, 128, 129, 200, 511, 512]:
        word = bytes((i * 7 + 1) % 255 + 1 for i in range(n))
        cases.append((word, y(RW, 5, 2)))
        cases.append((word, y(CLASSIC, 5, 2)))
        cases.append((word, scrypt(5, 2, 1)))
    cases.append((bytes(range(1, 256)), y(RW, 5, 2)))

    # $y$ salts: every length to 64 bytes, and past it.
    for n in range(0, 66):
        salt = enc64(bytes((i * 37 + 11) & 0xFF for i in range(n)))
        cases.append((pw, y(RW, 4, 1, salt=salt)))
    for salt in [".", "/", "z", "..", "/.", "./", "z.", "zz", "...", "../", "..z", "zzz",
                 "....", ".....", "/....", "..../", "ab-cd", "ab_c", "ab#c", "a%",
                 "ab$cd", "ab$", "$", "$$", "a$b$c", "abcd$efgh$", "x" * 86, "z" * 86,
                 "." * 85 + "/", "." * 86 + "/", "." * 87, "." * 88, "." * 89]:
        cases.append((pw, y(RW, 4, 1, salt=salt)))
    # A setting that is a whole hash.
    cases.append((pw, y(RW, 4, 1, salt="LdJMENpBABJJ3hIHjB1Bi.$tUlUF19mIl6XpRTpX7LBp5ABKS8KSmDfP1gXFrZ6Sy8")))

    # $7$: its parameters and salt characters.
    for n_log2, r, p in [(2, 1, 1), (4, 1, 1), (4, 2, 1), (6, 8, 1), (8, 1, 2), (8, 2, 3),
                         (10, 1, 1), (5, 3, 4)]:
        cases.append((pw, scrypt(n_log2, r, p)))
    for salt in ["", "a", "a$b", "a$", "$", "ab$cd$ef", "ab-cd", "ab$-cd", "ab$cd-ef", "-ab",
                 "$-ab", "x" * 100, "ab.cd/ef", "SodiumChloride$"]:
        cases.append((pw, scrypt(6, 8, 1, salt=salt)))

    # Settings refused, in the order libxcrypt checks.
    refused = [
        "$y$", "$y$j", "$y$j7", "$y$j75", "$y$j75x", "$y$$", "$y$j$", "$y$j7$", "$y$j75$",
        "$y$-75$abcd", "$y$j-5$abcd", "$y$j7-$abcd",
        y(2, 4, 1), y(46, 4, 1), y(48, 4, 1), y(RW + 0x3FC // 4, 4, 1), y(RW + 0x3FC // 4 + 1, 4, 1),
        y(3, 4, 1), y(4, 4, 1),
        y(RW, 1, 1), y(CLASSIC, 1, 1), y(WORM, 1, 1),
        y(RW, 64, 1), y(RW, 63, 1), y(RW, 33, 1), y(RW, 32, 1),
        y(RW, 4, 1, p=2), y(RW, 4, 1, p=1 << 20), y(RW, 4, 1 << 20, p=1 << 10),
        y(CLASSIC, 4, 1, t=1), y(RW, 4, 1, g=1), y(WORM, 4, 1, g=1), y(RW, 4, 1, nrom_log2=4),
        y(CLASSIC, 4, 1, nrom_log2=4),
        "$y$j75$abc" + "*" + "d",
        scrypt(0, 1, 1), scrypt(1, 1, 1), scrypt(4, 0, 1), scrypt(4, 1, 0), scrypt(63, 1, 1),
        scrypt(33, 1, 1), scrypt(4, 1 << 29, 4),
        "$7$", "$7$6", "$7$66..../", "$7$66..../...", "$7$66..../....", "$7$-6..../....abc",
        "$7$6-..../....abc", "$7$66..../-...abc",
    ]
    cases += [(pw, s) for s in refused]
    # A have field naming nothing, and one naming a field that is missing.
    cases += [(pw, "$y$j75" + enc_u32(16, 1) + "$abcd"), (pw, "$y$j75" + enc_u32(1, 1) + "$abcd")]

    # bcrypt: each variant at its least cost over keys built for its key
    # setup -- high bytes, which the $2x$ bug sign-extends and $2a$'s
    # safety watches for, and libxcrypt's self-test keys.
    salt = "abcdefghijklmnopqrstuu"
    keys = [b"", b"U*U", bytes(range(1, 73)), b"\xff" * 80, b"\x80" * 4,
            b"8b \xd0\xc1\xd2\xcf\xcc\xd8", b"\xff\xa334\xff\xff\xff\xa3345",
            b"\xa3" * 3 + b"345"]
    for variant in "abxy":
        for word in keys:
            cases.append((word, f"$2{variant}$04${salt}"))
    # It reads 72 bytes of key: 71, 72, 73 and 100 of them.
    for n in (71, 72, 73, 100):
        cases.append((bytes((i * 13) % 255 + 1 for i in range(n)), f"$2b$04${salt}"))
    # The salt's last character carries two bits: every one of the 64.
    for c in BF_ITOA64:
        cases.append((pw, f"$2b$04${salt[:21]}{c}"))
    # A whole hash as the setting, and what follows the salt.
    cases.append((pw, "$2b$05$CCCCCCCCCCCCCCCCCCCCC.E5YPO9kmyuRGyh0XouQYb4YMJKvyOeW"))
    cases.append((pw, f"$2b$04${salt}$"))
    cases.append((pw, f"$2b$04${salt}junk"))
    # Settings it refuses.
    for s in [f"$2b$03${salt}", f"$2b$32${salt}", f"$2b$40${salt}", f"$2b$4${salt}",
              f"$2b$0a${salt}", f"$2b$a4${salt}", f"$2c$04${salt}", f"$2B$04${salt}",
              f"$2b$04${salt[:21]}", "$2b$04$abcdefghij-lmnopqrstuu", "$2", "$2b", "$2b$",
              "$2b$04", "$2b$04$", "$2b$04x" + salt]:
        cases.append((pw, s))
    return cases


PROGRAM = r'''
#include <crypt.h>
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static const char *en(int e)
{
    static char buf[16];
    switch (e) {
    case 0: return "0";
    case EINVAL: return "EINVAL";
    case ERANGE: return "ERANGE";
    case ENOMEM: return "ENOMEM";
    case ENOSYS: return "ENOSYS";
    }
    snprintf(buf, sizeof buf, "%d", e);
    return buf;
}

int main(void)
{
    static char line[16384], pw[8192];
    while (fgets(line, sizeof line, stdin)) {
        line[strcspn(line, "\n")] = 0;
        char *setting = strchr(line, ' ');
        if (!setting)
            return 2;
        *setting++ = 0;
        size_t n = 0;
        if (strcmp(line, "-") != 0)
            for (const char *h = line; h[0] && h[1]; h += 2) {
                unsigned v;
                sscanf(h, "%2x", &v);
                pw[n++] = (char)v;
            }
        pw[n] = 0;
        errno = 0;
        const char *r = crypt(pw, setting);
        int e = errno;
        printf("%s %s = %s %s\n", line, setting, r ? r : "NULL", en(e));
    }
    return 0;
}
'''


def main() -> None:
    cases = [(p, s) for s in KA_SETTINGS for p in KA_PASSWORDS] + generated()
    lines = []
    for word, setting in cases:
        if b"\0" in word or " " in setting or "\n" in setting:
            sys.exit(f"a case the format cannot carry: {word!r} {setting!r}")
        lines.append((word.hex() or "-") + " " + setting)
    with workdir() as t:
        d = Path(t)
        (d / "cr.c").write_text(PROGRAM, encoding="utf-8", newline="\n")
        (d / "cases.txt").write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")
        r = run(f"cd {wsl_path(d)} && gcc -O2 -Wall -Werror -o cr cr.c -lcrypt && ./cr < cases.txt")
        if r.returncode != 0:
            sys.exit(f"the harness failed:\n{r.stderr}\n{r.stdout[-2000:]}")
        body = r.stdout
    if body.count("\n") != len(lines):
        sys.exit(f"{len(lines)} cases but {body.count(chr(10))} answers")
    head = ("# libxcrypt 4.4.36's crypt (Ubuntu 24.04's libcrypt.so.1), for posix/src/crypt.rs.\n"
            "# Generated by posix/tools/oracle/crypt_harness.py; do not edit.\n")
    OUT.write_text(head + body, encoding="utf-8", newline="\n")
    print(f"{OUT.name}: {len(lines)} lines")


if __name__ == "__main__":
    main()
