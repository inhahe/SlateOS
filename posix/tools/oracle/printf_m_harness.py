"""glibc 2.39's `%m` and `%#m`, as the oracle for posix/src/printf.rs.

    python posix/tools/oracle/printf_m_harness.py   # writes posix/src/printf_m_oracle.txt

`%m` prints the text of the `errno` a call began with, and `%#m` its name --
or, for a number that names no error, the number, formatted as `%d` formats
it. Every format below, with every `errno`, through `snprintf`: widths,
precisions, flags, a `*` width, length modifiers (ignored), two `%m`s in
one format, and numbers that are errors, are not, and are negative.

    <errno> <format-hex> | <ret> <output-hex>

A `*` takes the argument 12.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from _wsl import POSIX_SRC, run, workdir, wsl_path  # noqa: E402

OUT = POSIX_SRC / "printf_m_oracle.txt"

ERRNOS = [0, 1, 2, 22, 133, 134, 255, 9999, -1, -2147483648]
FORMATS = [
    "%m", "[%10m]", "[%-10m]", "[%.3m]", "[%10.3m]", "[%-10.3m]", "[%.0m]",
    "%#m", "[%#10m]", "[%#-10m]", "[%#.2m]", "[%#+m]", "[% #m]", "[%#08m]",
    "[%#.5m]", "[%lm]", "[%hhm]", "x%my%mz", "%m %#m", "[%*m]", "[%-*m]",
]

C_MAIN = r'''
#define _GNU_SOURCE
#include <errno.h>
#include <stdio.h>
#include <string.h>

static void hex(const char *s, int n) {
    if (n <= 0) { fputs("\"\"", stdout); return; }
    for (int i = 0; i < n; i++) printf("%02x", (unsigned char)s[i]);
}

int main(void) {
    static const int errs[] = { @ERRS@ };
    static const char *fmts[] = { @FMTS@ };
    for (unsigned i = 0; i < sizeof errs / sizeof *errs; i++) {
        for (unsigned j = 0; j < sizeof fmts / sizeof *fmts; j++) {
            char buf[512];
            const char *f = fmts[j];
            int n;
            errno = errs[i];
            if (strchr(f, '*'))
                n = snprintf(buf, sizeof buf, f, 12);
            else
                n = snprintf(buf, sizeof buf, f);
            printf("%d ", errs[i]);
            hex(f, (int)strlen(f));
            printf(" | %d ", n);
            hex(buf, n);
            putchar('\n');
        }
    }
    return 0;
}
'''


def main() -> None:
    errs = ", ".join("(-2147483647 - 1)" if e == -2147483648 else str(e) for e in ERRNOS)
    fmts = ", ".join('"' + f + '"' for f in FORMATS)
    with workdir() as d:
        d = Path(d)
        (d / "pm.c").write_text(C_MAIN.replace("@ERRS@", errs).replace("@FMTS@", fmts),
                                encoding="utf-8", newline="\n")
        built = run(f"cd {wsl_path(d)} && gcc -O1 -Wno-format -Wno-format-security -o pm pm.c")
        if built.returncode or built.stdout or built.stderr:
            sys.exit(f"pm.c: {built.stdout}{built.stderr}")
        r = run(f"cd {wsl_path(d)} && ./pm")
        if r.returncode or r.stderr:
            sys.exit(f"pm: {r.stdout}{r.stderr}")
    OUT.write_text(r.stdout, encoding="utf-8", newline="\n")
    print(f"wrote {OUT}: {len(r.stdout.splitlines())} lines")


if __name__ == "__main__":
    main()
