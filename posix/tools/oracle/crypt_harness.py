"""libxcrypt 4.4.36's crypt -- Ubuntu 24.04's libcrypt.so.1 -- as the oracle
for posix/src/crypt.rs, yescrypt.rs, bcrypt.rs and des.rs, and the hashing
behind them in posix/pwhash; and its crypt_gensalt, crypt_checksalt,
crypt_preferred_method (gensalt.rs), encrypt and setkey (des.rs).

    python posix/tools/oracle/crypt_harness.py
        # writes posix/src/crypt_oracle.txt, gensalt_oracle.txt and
        # encrypt_oracle.txt

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
  -- MD5, SHA-256, SHA-512, scrypt, yescrypt, bcrypt's four variants,
  traditional DES, bigcrypt and BSDi's DES;
- the DES methods over salts at each end of the alphabet, passwords around
  the eight characters each block reads, to bigcrypt's sixteen blocks and
  past them, under traditional and bigcrypt settings, BSDi's counts, salts
  and folding, and what each refuses;
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
    "CC..............",
    "ab..............",
    "_/...CCCC",
    "_/...abcd",
    "_B...CCCC",
    "_B...abcd",
    "CC",
    "ab",
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

    # Traditional DES and bigcrypt: salts reaching each end of the alphabet
    # in each place, and what follows a salt.
    for s in ["..", "./", "/.", "//", "09", "9A", "AZ", "Za", "az", "zz", "z.", ".z",
              "ab$", "ab$cd", "ab-x", "abJnggxhB/yWI", "abJnggxhB/yWIjunk"]:
        cases.append((pw, s))
    cases.append((b"password", "ab"))
    # Each character's top bit, which the key loses.
    for word in [b"\x80", b"\xff" * 8, b"\xe1\xe2\xe3", b"\xc3\xa9t\xc3\xa9", bytes(range(128, 140))]:
        cases.append((word, "ab"))
        cases.append((word, "ab" + "." * 12))
    # Passwords around the eight characters traditional DES reads and
    # bigcrypt's blocks of eight, to its sixteen and past them; under a
    # traditional setting (two characters, or thirteen), which truncates,
    # and under bigcrypt's (fourteen or more).
    for n in [1, 7, 8, 9, 15, 16, 17, 24, 25, 64, 120, 127, 128, 129, 200, 511]:
        word = bytes((i * 11) % 94 + 33 for i in range(n))
        for s in ["ab", "ab" + "." * 11, "ab" + "." * 12, "ab" + "." * 22, "zz" + "." * 175]:
            cases.append((word, s))
    # BSDi's: counts, salts, the characters after the nine, and passwords
    # folded at each length around its blocks.
    for s in ["_J9..abcd", "_J9..abcdXYZ", "_....abcd", "_/...abcd", "_0...abcd", "_zz..abcd",
              "_zzz.abcd", "_..0.abcd", "_J9......", "_J9..zzzz", "_J9../...", "_J9.....z"]:
        cases.append((pw, s))
    for n in [0, 1, 7, 8, 9, 15, 16, 17, 64, 200, 511]:
        word = bytes((i * 13) % 94 + 33 for i in range(n))
        cases.append((word, "_J9..abcd"))
    for word in [b"\x80", b"\xff" * 9, b"\xc3\xa9t\xc3\xa9"]:
        cases.append((word, "_J9..abcd"))
    # What the three refuse: no salt character, too short, a character
    # outside the alphabet where one is read.
    for s in ["", "a", "a{", "{a", "a$", ".", "_", "_J9..abc", "_J9.-abcd", "_J9..abc-",
              "_J9..ab{d", "_-9..abcd"]:
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


GENSALT_OUT = POSIX_SRC / "gensalt_oracle.txt"

# crypt_gensalt_rn, crypt_checksalt and crypt_preferred_method.  A probe a
# line in, an answer a line out: the probe, " = ", what the call returned
# (NULL, or the setting), errno's name or 0, and -- for crypt_gensalt_rn --
# what the output buffer held after, up to its first NUL, as `out=...`.
GENSALT_PROGRAM = r'''
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
    }
    snprintf(buf, sizeof buf, "%d", e);
    return buf;
}

static int unhex(const char *h, char *out)
{
    int n = 0;
    for (; h[0] && h[1]; h += 2) {
        unsigned v;
        sscanf(h, "%2x", &v);
        out[n++] = (char)v;
    }
    return n;
}

int main(void)
{
    static char line[4096];
    while (fgets(line, sizeof line, stdin)) {
        line[strcspn(line, "\n")] = 0;
        char kind[16], a[1024], b[1024];
        unsigned long count;
        int nrbytes, size;
        if (sscanf(line, "gensalt %1023s %lu %1023s %d %d", a, &count, b, &nrbytes, &size) == 5) {
            static char rbytes[512], output[256];
            const char *prefix = strcmp(a, "NULL") ? (strcmp(a, "-") ? a : "") : NULL;
            const char *rb = NULL;
            if (strcmp(b, "NULL") != 0) {
                unhex(strcmp(b, "-") ? b : "", rbytes);
                rb = rbytes;
            }
            memset(output, 'Q', sizeof output);
            output[sizeof output - 1] = 0;
            errno = 0;
            char *r = crypt_gensalt_rn(prefix, count, rb, nrbytes, output, size);
            int e = errno;
            printf("%s = %s %s out=%s\n", line, r ? r : "NULL", en(e), size > 0 ? output : "");
        } else if (sscanf(line, "checksalt %1023s", a) == 1) {
            const char *setting = strcmp(a, "NULL") ? (strcmp(a, "-") ? a : "") : NULL;
            printf("%s = %d\n", line, crypt_checksalt(setting));
        } else if (strcmp(line, "preferred") == 0) {
            printf("%s = %s\n", line, crypt_preferred_method());
        } else {
            (void)kind;
            return 2;
        }
    }
    return 0;
}
'''


def gensalt_cases():
    """Probes for crypt_gensalt_rn, crypt_checksalt and crypt_preferred_method.
    A prefix or a setting is written as is, `-` for the empty string, NULL
    for a null pointer; random bytes in hex, likewise."""
    rb = bytes((i * 29 + 7) & 0xFF for i in range(255)).hex()

    def rbytes(n):
        return rb[:2 * n] or "-"

    def aborts(prefix, count, size):
        """Whether libxcrypt's gensalt_sha_rn aborts on this probe: its room
        check lets through a buffer of the length it computes -- a byte too
        few, or two for a power of ten -- which its salt loop's assertion
        then refuses.  gensalt.rs answers ERANGE there, and its own tests
        say so.  (MD5 refuses a nonzero count before it gets that far.)"""
        tags = {"$6$": (5000, 1000, 999999999), "$5$": (5000, 1000, 999999999), "$1$": (1000, 1000, 1000)}
        if prefix not in tags or (prefix == "$1$" and count != 0):
            return False
        default, low, high = tags[prefix]
        count = min(max(count or default, low), high)
        output_len = 8
        if count != default:
            output_len += 9
            ceiling = 10
            while ceiling < count:
                output_len += 1
                ceiling *= 10
        if size < output_len:
            return False
        written = 3 if count == default else len(f"$x$rounds={count}$")
        return not written + 5 < size

    lines = []
    # Every method's prefix: BSDi's DES's `_`, and traditional DES's none.
    prefixes = ["$y$", "$7$", "$2b$", "$2y$", "$2a$", "$2x$", "$6$", "$5$", "$1$", "_", "-"]
    counts = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 31, 32, 999, 1000, 4999, 5000,
              5001, 999999999, 1000000000, 4294967296, 18446744073709551615]
    # Every method at every cost, with the bytes it takes by default.
    for p in prefixes:
        for c in counts:
            lines.append(f"gensalt {p} {c} {rbytes(16)} 16 192")
    # Every method's byte counts, at its default cost.
    for p in prefixes:
        for n in [0, 2, 3, 4, 8, 9, 10, 14, 15, 16, 17, 22, 64, 65, 255]:
            lines.append(f"gensalt {p} 0 {rbytes(n)} {n} 192")
    # Every method's room, around its setting's length.
    for p in prefixes:
        for size in [0, 1, 2, 3, 4, 7, 8, 9, 16, 17, 18, 20, 21, 25, 26, 28, 29, 30, 31,
                     39, 40, 44, 45, 74, 75, 76, 117, 138, 139, 140, 192]:
            lines.append(f"gensalt {p} 0 {rbytes(64)} 64 {size}")
    # Rounds fields, by size.
    for size in [24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34]:
        lines.append(f"gensalt $6$ 999999999 {rbytes(15)} 15 {size}")
        lines.append(f"gensalt $5$ 1001 {rbytes(15)} 15 {size}")
    # The default method, and prefixes that are more than a prefix, or none.
    for p in ["NULL", "$6$rounds=9999$x", "$y$j9T$abc", "$2b$10$", "$gy$", "_", "-", "ab",
              "$3$", "$md5", "$sha1", "$", "$2", "$2c$", "x", "$9$", "$Y$"]:
        lines.append(f"gensalt {p} 0 {rbytes(16)} 16 192")
    # crypt_checksalt: the prefix, the characters, nothing.
    for s in ["NULL", "-", "$y$j9T$PKXc3hCOSyMqdaEQArI62/", "$y$", "$gy$j9T$abc", "$7$CU..../....",
              "$2b$05$CCCCCCCCCCCCCCCCCCCCC.", "$2y$05$x", "$2a$05$x", "$2x$05$x", "$2c$05$x",
              "$6$salt", "$6$rounds=1000$salt", "$5$salt", "$1$salt", "$3$$", "$md5$x", "$sha1$1$x",
              "_J9..abcd", "ab", "a", "abc", "*0", "!$6$salt", "$6$sa:lt", "$6$sa*lt", "$",
              "$y", "$9$x", "xyz$"]:
        lines.append(f"checksalt {s}")
    lines.append("preferred")

    def kept(line):
        words = line.split()
        if words[0] != "gensalt":
            return True
        return not aborts(words[1], int(words[2]), int(words[5]))

    return [line for line in lines if kept(line)]


def gensalt_main() -> None:
    lines = gensalt_cases()
    with workdir() as t:
        d = Path(t)
        (d / "gs.c").write_text(GENSALT_PROGRAM, encoding="utf-8", newline="\n")
        (d / "cases.txt").write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")
        r = run(f"cd {wsl_path(d)} && gcc -O2 -Wall -Werror -o gs gs.c -lcrypt && ./gs < cases.txt")
        if r.returncode != 0:
            sys.exit(f"the gensalt harness failed:\n{r.stderr}\n{r.stdout[-2000:]}")
        body = r.stdout
    if body.count("\n") != len(lines):
        sys.exit(f"{len(lines)} gensalt probes but {body.count(chr(10))} answers")
    head = ("# libxcrypt 4.4.36's crypt_gensalt_rn, crypt_checksalt and crypt_preferred_method\n"
            "# (Ubuntu 24.04's libcrypt.so.1), for posix/src/gensalt.rs. Generated by\n"
            "# posix/tools/oracle/crypt_harness.py; do not edit.\n")
    GENSALT_OUT.write_text(head + body, encoding="utf-8", newline="\n")
    print(f"{GENSALT_OUT.name}: {len(lines)} lines")


ENCRYPT_OUT = POSIX_SRC / "encrypt_oracle.txt"

# encrypt and setkey -- libxcrypt keeps them only as compatibility symbols,
# which a program cannot link to, so they are looked up by version. A probe
# a line in, in order, since setkey's schedule is the process's: `setkey
# <key>` or `encrypt <block> <edflag>`, each 64 bytes in hex; an answer a
# line out, the probe, " = ", the block after (encrypt), and errno's name.
ENCRYPT_PROGRAM = r'''
#define _GNU_SOURCE
#include <dlfcn.h>
#include <errno.h>
#include <stdio.h>
#include <string.h>

static const char *en(int e)
{
    static char buf[16];
    switch (e) {
    case 0: return "0";
    case EINVAL: return "EINVAL";
    case ENOSYS: return "ENOSYS";
    }
    snprintf(buf, sizeof buf, "%d", e);
    return buf;
}

static void unhex(const char *h, char *out)
{
    for (int i = 0; i < 64; i++) {
        unsigned v;
        sscanf(h + 2 * i, "%2x", &v);
        out[i] = (char)v;
    }
}

int main(void)
{
    void *lib = dlopen("libcrypt.so.1", RTLD_NOW);
    if (!lib)
        return 3;
    void (*enc)(char *, int) = (void (*)(char *, int))dlvsym(lib, "encrypt", "GLIBC_2.2.5");
    void (*sk)(const char *) = (void (*)(const char *))dlvsym(lib, "setkey", "GLIBC_2.2.5");
    if (!enc || !sk)
        return 4;
    static char line[4096];
    while (fgets(line, sizeof line, stdin)) {
        line[strcspn(line, "\n")] = 0;
        char hex[256], block[64];
        int edflag;
        if (sscanf(line, "setkey %255s", hex) == 1 && strlen(hex) == 128) {
            unhex(hex, block);
            errno = 0;
            sk(block);
            printf("%s = %s\n", line, en(errno));
        } else if (sscanf(line, "encrypt %255s %d", hex, &edflag) == 2 && strlen(hex) == 128) {
            unhex(hex, block);
            errno = 0;
            enc(block, edflag);
            int e = errno;
            printf("%s = ", line);
            for (int i = 0; i < 64; i++)
                printf("%02x", (unsigned char)block[i]);
            printf(" %s\n", en(e));
        } else {
            return 2;
        }
    }
    return 0;
}
'''


def encrypt_cases():
    """The probes for encrypt and setkey: blocks before any key, then keys
    each followed by blocks. A bit stands as a byte whose lowest bit it is,
    and the bytes vary above it, to show that only that bit is read."""
    def bits(value: int, high: int = 0) -> str:
        out = bytearray()
        for i in range(64):
            out.append((high * (i + 1) * 37 & 0xFE) | ((value >> (63 - i)) & 1))
        return out.hex()

    blocks = [(0, 0), (0x0123456789ABCDEF, 0), (0x4E6F772069732074, 0x55),
              (0xFFFFFFFFFFFFFFFF, 0), (0x8000000000000000, 0xAA), (0x0123456789ABCDEF, 0xFF)]
    lines = []
    for value, high in blocks[:3]:
        for edflag in (0, 1):
            lines.append(f"encrypt {bits(value, high)} {edflag}")
    keys = [(0x133457799BBCDFF1, 0), (0x0123456789ABCDEF, 0x3C), (0x0101010101010101, 0),
            (0xFEFEFEFEFEFEFEFE, 0x81), (0x0000000000000000, 0), (0x8001020304050607, 0xFF)]
    for key, khigh in keys:
        lines.append(f"setkey {bits(key, khigh)}")
        for value, high in blocks:
            for edflag in (0, 1, 2, -1):
                lines.append(f"encrypt {bits(value, high)} {edflag}")
    return lines


def encrypt_main() -> None:
    lines = encrypt_cases()
    with workdir() as t:
        d = Path(t)
        (d / "en.c").write_text(ENCRYPT_PROGRAM, encoding="utf-8", newline="\n")
        (d / "cases.txt").write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")
        r = run(f"cd {wsl_path(d)} && gcc -O2 -Wall -Werror -o en en.c -ldl && ./en < cases.txt")
        if r.returncode != 0:
            sys.exit(f"the encrypt harness failed ({r.returncode}):\n{r.stderr}\n{r.stdout[-2000:]}")
        body = r.stdout
    if body.count("\n") != len(lines):
        sys.exit(f"{len(lines)} encrypt probes but {body.count(chr(10))} answers")
    head = ("# libxcrypt 4.4.36's encrypt and setkey (Ubuntu 24.04's libcrypt.so.1, by their\n"
            "# GLIBC_2.2.5 versions), in order, for posix/src/des.rs. Generated by\n"
            "# posix/tools/oracle/crypt_harness.py; do not edit.\n")
    ENCRYPT_OUT.write_text(head + body, encoding="utf-8", newline="\n")
    print(f"{ENCRYPT_OUT.name}: {len(lines)} lines")


def main() -> None:
    gensalt_main()
    encrypt_main()
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
