"""glibc 2.39's wide-character classes and case mappings under `C.UTF-8`, as
the oracle for posix/src/wctype.rs.

    python posix/tools/oracle/wctype_harness.py   # writes posix/src/wctype_oracle.txt

For every code point from U+0000 to U+10FFFF: which of the twelve classes
glibc puts it in (`iswalnum` ... `iswxdigit`), and what `towlower` and
`towupper` make of it. glibc 2.39's `C.UTF-8` is built into the library,
from Unicode 15.1.0's data: it classifies the characters 15.0 and 15.1
added. (The `i18n_ctype` Ubuntu installs under `/usr/share/i18n` says
14.0.0, and is not what the built-in locale was made from.) These are
glibc's rules applied to 15.1.0; the generator
(`posix/tools/wctype_gen.py`) reproduces them from that version's files
before it applies them to the system's.

    class <first> <last> <mask>    a run of code points with the same classes
    lower <cp> <to>                towlower(cp), where it is not cp
    upper <cp> <to>                towupper(cp), where it is not cp

Code points are hex. The mask's bits are the classes in this order, bit 0
first: alnum alpha blank cntrl digit graph lower print punct space upper
xdigit. A header line records glibc's version and the locale.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from _wsl import POSIX_SRC, run, workdir, wsl_path  # noqa: E402

OUT = POSIX_SRC / "wctype_oracle.txt"

CLASSES = ("alnum", "alpha", "blank", "cntrl", "digit", "graph", "lower", "print", "punct",
           "space", "upper", "xdigit")

C_MAIN = r'''
#include <gnu/libc-version.h>
#include <locale.h>
#include <stdio.h>
#include <wctype.h>

int main(void) {
    if (!setlocale(LC_ALL, "C.UTF-8")) { fputs("no C.UTF-8\n", stderr); return 1; }
    int (*const tests[])(wint_t) = {
        iswalnum, iswalpha, iswblank, iswcntrl, iswdigit, iswgraph,
        iswlower, iswprint, iswpunct, iswspace, iswupper, iswxdigit,
    };
    printf("glibc %s C.UTF-8\n", gnu_get_libc_version());
    unsigned run_first = 0, run_mask = 0;
    for (unsigned cp = 0; cp <= 0x10FFFF; cp++) {
        unsigned mask = 0;
        for (unsigned i = 0; i < sizeof tests / sizeof *tests; i++)
            if (tests[i]((wint_t)cp)) mask |= 1u << i;
        if (cp == 0) run_mask = mask;
        else if (mask != run_mask) {
            printf("class %x %x %x\n", run_first, cp - 1, run_mask);
            run_first = cp;
            run_mask = mask;
        }
    }
    printf("class %x %x %x\n", run_first, 0x10FFFFu, run_mask);
    for (unsigned cp = 0; cp <= 0x10FFFF; cp++) {
        wint_t lo = towlower((wint_t)cp), up = towupper((wint_t)cp);
        if (lo != cp) printf("lower %x %x\n", cp, (unsigned)lo);
        if (up != cp) printf("upper %x %x\n", cp, (unsigned)up);
    }
    return 0;
}
'''


def main() -> None:
    with workdir() as d:
        d = Path(d)
        (d / "wc.c").write_text(C_MAIN, encoding="utf-8", newline="\n")
        built = run(f"cd {wsl_path(d)} && gcc -O1 -Wall -Werror -o wc wc.c")
        if built.returncode or built.stdout or built.stderr:
            sys.exit(f"wc.c: {built.stdout}{built.stderr}")
        r = run(f"cd {wsl_path(d)} && ./wc")
        if r.returncode or r.stderr:
            sys.exit(f"wc: {r.stdout}{r.stderr}")
    OUT.write_text(r.stdout, encoding="utf-8", newline="\n")
    lines = r.stdout.splitlines()
    kinds = {k: sum(1 for line in lines if line.startswith(k + " ")) for k in ("class", "lower", "upper")}
    print(f"wrote {OUT}: {lines[0]}; {kinds}")


if __name__ == "__main__":
    main()
