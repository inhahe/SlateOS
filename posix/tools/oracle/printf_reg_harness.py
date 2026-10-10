"""glibc 2.39's printf with conversions, a modifier and a type registered
(`<printf.h>`), as the oracle for posix/src/printf_h.rs's registration.

    python posix/tools/oracle/printf_reg_harness.py   # writes posix/src/printf_reg_oracle.txt

The program registers, before any call:

- `%Y`, a handler that writes what it was given -- `<Y` the `int`, its
  width, precision and `user` bits, and its flags as letters `>` -- with an
  arginfo function saying one `PA_INT`;
- `%P`, glibc's own test of a user type (`tst-vfprintf-user-type.c`): a
  `{ long; double; }` read by a registered reader, one of them -- or, with
  a precision, that many, written `{(i,d),...}` -- `d` written as
  `(long)(d * 100)` so that no float formatting enters it;
- `%d`, a handler answering -2 -- format it as built in -- unless `#` is
  given, and then writing `#d=` and the value;
- `printf_size` and `printf_size_info` on `%b` and `%B`, as glibc's
  `tst-printfsz.c` registers them;
- the modifier `R` (`register_printf_modifier`), whose bit `%Y` reports.

and then formats each case through `snprintf`, recording what it returned
and wrote.

One line a call:

    <format-hex> <args> | <ret> <output-hex or -e<errno>> [<what %n stored>]

`<args>` comma-separated: `i:<int>`, `d:<double, as C's %a>`, `s:<hex>`,
`P:<long>/<double as %a>` (the user type), `n:0` (an `int` for `%n`).
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from _wsl import POSIX_SRC, run, workdir, wsl_path  # noqa: E402

OUT = POSIX_SRC / "printf_reg_oracle.txt"

CASES: list[tuple[str, list[tuple[str, object]]]] = []


def case(fmt: str, *args: tuple[str, object]) -> None:
    CASES.append((fmt, list(args)))


# -- %Y: what a handler is given --
for fmt in ["[%Y]", "[%5Y]", "[%-5.3Y]", "[%#+ 0Y]", "[%'IY]", "[%lY]", "[%hhY]", "[%hY]",
            "[%LY]", "[%llY]", "[%qY]", "[%zY]", "[%RY]", "[%RlY]", "[%.0Y]", "[%0-8Y]"]:
    case(fmt, ("i", 5))
case("[%*.*Y]", ("i", 7), ("i", 2), ("i", 5))
case("[%*Y]", ("i", -3), ("i", 5))
case("[%.*Y]", ("i", -1), ("i", 5))
case("[%1$Y %1$d %2$Y]", ("i", 5), ("i", -6))
case("[%Y|%s|%Y]", ("i", 1), ("s", "mid"), ("i", 2))

# -- %P: a user type, several to a conversion --
case("[%P]", ("P", (1, 2.5)))
case("[%.2P]", ("P", (1, 2.5)), ("P", (3, 4.25)))
case("[%.0P]")
case("[%P %d %P]", ("P", (7, 0.5)), ("i", 9), ("P", (-8, -1.25)))

# -- %d: a handler that hands the plain conversion back --
case("[%d %#d %5d %#5d]", ("i", 5), ("i", 6), ("i", 7), ("i", 8))
case("[%ld %#ld]", ("i", -5), ("i", -6))

# -- printf_size on %b and %B --
for value in [0.0, 1.0, 1023.0, 1024.0, 1536.0, 1000.0, 1e6, 1048576.0, 1e9, 3.5e12,
              1e30, -5.0, -2048.0]:
    for fmt in ["%b", "%B"]:
        case(f"[{fmt}]", ("d", value))
for fmt in ["%.0b", "%.1B", "%10b|", "%-10b|", "%010.2b", "%+b", "% b", "%#.0b", "%-+12.1B|"]:
    case(f"[{fmt}]", ("d", 1536.0))
for value in ["nan", "inf", "-inf"]:
    for fmt in ["%b", "%8b|", "%-8b|", "%+b", "%.10b|"]:
        case(f"[{fmt}]", ("d", value))

# -- the rest of the format, read by the positional pass --
case("[%s %n|]", ("s", "ab"), ("n", 0))
case("[%2$s %1$s]", ("s", "one"), ("s", "two"))
case("abc%")
case("[%d %", ("i", 5))
case("[%w12d]", ("i", 5))
case("[%m]")
case("[%y %5% %c]", ("i", 65))


def hexfloat(x: object) -> str:
    if x == "nan":
        return "NAN"
    if x == "inf":
        return "INFINITY"
    if x == "-inf":
        return "(-INFINITY)"
    return float(x).hex()


def c_arg(t: str, v: object, idx: int) -> tuple[str, str]:
    if t == "i":
        return "", f"(int)({v})"
    if t == "d":
        return "", f"(double){hexfloat(v)}"
    if t == "s":
        body = "".join(f"\\x{b:02x}" for b in str(v).encode())
        return "", f'"{body}"'
    if t == "P":
        i, d = v
        return "", f"(long)({i}L), (double){hexfloat(d)}"
    if t == "n":
        return f"int nv{idx} = 0x55;", f"&nv{idx}"
    raise ValueError(t)


def arg_token(t: str, v: object) -> str:
    if t == "i":
        return f"i:{v}"
    if t == "d":
        return f"d:{v if isinstance(v, str) else float(v).hex()}"
    if t == "s":
        return "s:" + str(v).encode().hex()
    if t == "P":
        i, d = v
        return f"P:{i}/{float(d).hex()}"
    return "n:0"


PRELUDE = r'''
#define _GNU_SOURCE
#include <errno.h>
#include <locale.h>
#include <math.h>
#include <printf.h>
#include <stdio.h>
#include <string.h>

static int user_type;
static int r_bit;

struct pair { long i; double d; };

static void pair_va_arg(void *mem, va_list *ap) {
    struct pair *p = mem;
    p->i = va_arg(*ap, long);
    p->d = va_arg(*ap, double);
}

static int pair_arginfo(const struct printf_info *info, size_t n, int *argtypes, int *size) {
    int count = info->prec >= 0 ? info->prec : 1;
    for (int k = 0; k < count && (size_t) k < n; k++) {
        argtypes[k] = user_type;
        size[k] = sizeof(struct pair);
    }
    return count;
}

static int pair_out(FILE *fp, const struct printf_info *info, const void *const *args) {
    int count = info->prec >= 0 ? info->prec : 1;
    int done = 0;
    if (info->prec >= 0) done += fprintf(fp, "{");
    for (int k = 0; k < count; k++) {
        const struct pair *p = *(void **) args[k];
        done += fprintf(fp, "%s(%ld,%ld)", k ? "," : "", p->i, (long)(p->d * 100));
    }
    if (info->prec >= 0) done += fprintf(fp, "}");
    return done;
}

static int y_arginfo(const struct printf_info *info, size_t n, int *argtypes, int *size) {
    (void) info; (void) size;
    if (n >= 1) argtypes[0] = PA_INT;
    return 1;
}

static int y_out(FILE *fp, const struct printf_info *info, const void *const *args) {
    char flags[16];
    int k = 0;
    if (info->alt) flags[k++] = '#';
    if (info->space) flags[k++] = 's';
    if (info->left) flags[k++] = '-';
    if (info->showsign) flags[k++] = '+';
    if (info->group) flags[k++] = '\'';
    if (info->i18n) flags[k++] = 'I';
    if (info->pad == '0') flags[k++] = '0';
    if (info->is_long_double) flags[k++] = 'L';
    if (info->is_long) flags[k++] = 'l';
    if (info->is_short) flags[k++] = 'h';
    if (info->is_char) flags[k++] = 'c';
    if (info->wide) flags[k++] = 'w';
    flags[k] = 0;
    return fprintf(fp, "<Y%d;w%d;p%d;u%d;%s>", *(const int *) args[0], info->width,
                   info->prec, info->user == r_bit ? 1 : (int) info->user, flags);
}

static int d_out(FILE *fp, const struct printf_info *info, const void *const *args) {
    if (!info->alt) return -2;
    if (info->is_long) return fprintf(fp, "#d=%ld", *(const long *) args[0]);
    return fprintf(fp, "#d=%d", *(const int *) args[0]);
}

static void hex(const char *s, int n) {
    if (n <= 0) { fputs("-", stdout); return; }
    for (int i = 0; i < n; i++) printf("%02x", (unsigned char) s[i]);
}

int main(void) {
    if (!setlocale(LC_ALL, "C.UTF-8")) return 1;
    user_type = register_printf_type(pair_va_arg);
    r_bit = register_printf_modifier(L"R");
    if (user_type < 0 || r_bit < 0
        || register_printf_specifier('P', pair_out, pair_arginfo)
        || register_printf_specifier('Y', y_out, y_arginfo)
        || register_printf_specifier('d', d_out, NULL)
        || register_printf_function('b', printf_size, printf_size_info)
        || register_printf_function('B', printf_size, printf_size_info)) {
        fputs("registration failed\n", stderr);
        return 1;
    }
    char buf[512];
    int n;
'''


def main() -> None:
    lines = []
    for fmt, args in CASES:
        decls, exprs, ns = [], [], []
        for i, (t, v) in enumerate(args):
            d, e = c_arg(t, v, i)
            if d:
                decls.append(d)
                ns.append(f"nv{i}")
            exprs.append(e)
        fmt_c = "".join(f"\\x{b:02x}" for b in fmt.encode())
        call = f'n = snprintf(buf, sizeof buf, "{fmt_c}"{"".join(", " + e for e in exprs)});'
        store = "".join(f' printf(" %d", {nv});' for nv in ns)
        tokens = ",".join(arg_token(t, v) for t, v in args) or "-"
        lines.append(
            f"{{ {' '.join(decls)} errno = 0; {call} "
            f'printf("%s {tokens} | %d ", "{fmt.encode().hex()}", n); '
            f'if (n < 0) printf("-e%d", errno); else hex(buf, n);{store} putchar(\'\\n\'); }}'
        )
    program = PRELUDE + "\n    ".join(lines) + "\n    return 0;\n}\n"
    with workdir() as d:
        d = Path(d)
        (d / "pr.c").write_text(program, encoding="utf-8", newline="\n")
        built = run(f"cd {wsl_path(d)} && gcc -O0 -Wno-format -Wno-format-extra-args "
                    f"-Wno-deprecated-declarations -o pr pr.c -lm 2>&1")
        if built.returncode:
            sys.exit(f"pr.c: {built.stdout}{built.stderr}")
        r = run(f"cd {wsl_path(d)} && ./pr")
        if r.returncode or r.stderr:
            sys.exit(f"pr: {r.stdout}{r.stderr}")
    OUT.write_text(r.stdout, encoding="utf-8", newline="\n")
    print(f"wrote {OUT}: {len(r.stdout.splitlines())} lines ({len(CASES)} cases)")


if __name__ == "__main__":
    main()
