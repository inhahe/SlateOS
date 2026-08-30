r"""Compare our printability rule against glibc's `iswprint` under `C.UTF-8`.

`design-decisions.md` §357 chose a rule that needs no Unicode table — a
character is printable iff it is not a Unicode control (`Cc`) and not a
line/paragraph separator (`Zl`/`Zp`, exactly `{U+2028, U+2029}`). The claim
that makes that choice safe is an empirical one: it agrees with the table GNU
actually consults everywhere except *unassigned* code points. This script
checks that claim over every code point rather than over a sampled corpus, and
prints the divergence broken down by Unicode general category, so a claim that
has stopped being true is visible rather than assumed.

Run under a glibc system (WSL), where `iswprint` is glibc's:

    wsl -e python3 scripts/printable-audit.py

It exits non-zero if any code point diverges for a reason other than being
unassigned, which is the condition §357 rests on.
"""

import ctypes
import sys
import unicodedata

libc = ctypes.CDLL("libc.so.6", use_errno=True)
libc.setlocale.restype = ctypes.c_char_p
if not libc.setlocale(6, b"C.UTF-8"):  # 6 == LC_ALL on glibc
    sys.exit("C.UTF-8 is not available on this system")
libc.iswprint.argtypes = [ctypes.c_uint]


def ours(cp: int) -> bool:
    """`coreutils::quote::printable_char`, transcribed."""
    # Rust's `char::is_control` is exactly Unicode category `Cc`.
    return unicodedata.category(chr(cp)) != "Cc" and cp not in (0x2028, 0x2029)


def main() -> None:
    print(f"unicodedata {unicodedata.unidata_version}")
    counts: dict[str, int] = {}
    examples: dict[str, list[str]] = {}
    total = 0
    for cp in range(0x110000):
        if 0xD800 <= cp <= 0xDFFF:  # surrogates: not characters at all
            continue
        glibc = bool(libc.iswprint(cp))
        if glibc == ours(cp):
            continue
        total += 1
        cat = unicodedata.category(chr(cp))
        who = "glibc-only" if glibc else "ours-only"
        key = f"{cat} ({who} prints it)"
        counts[key] = counts.get(key, 0) + 1
        examples.setdefault(key, [])
        if len(examples[key]) < 4:
            examples[key].append(f"U+{cp:04X}")

    if not total:
        print("no divergence at all")
        return
    print(f"{total} code points diverge:")
    for key in sorted(counts):
        print(f"  {counts[key]:>7}  {key}  e.g. {', '.join(examples[key])}")

    # `Cn` is unassigned: no font draws it and no standard has given it a
    # meaning, so neither answer is wrong and §357 accepts the difference.
    # Anything else would be a rule that has drifted from the table.
    unexpected = {k: v for k, v in counts.items() if not k.startswith("Cn ")}
    if unexpected:
        print("\nUNEXPECTED -- §357 only accounts for unassigned code points:")
        for key in sorted(unexpected):
            print(f"  {unexpected[key]:>7}  {key}  e.g. {', '.join(examples[key])}")
        sys.exit(1)


main()
