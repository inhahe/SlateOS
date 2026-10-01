"""glibc 2.39's swscanf, as the oracle for the wide scanf family.

    python posix/tools/oracle/wscanf_harness.py           # print the table
    python posix/tools/oracle/wscanf_harness.py --check   # compare with scanf.rs's

Each case is (name, format, input, kinds): format and input are lists of code
points (so an unencodable surrogate can be written), or Python strings.  The
C program runs each in the C.UTF-8 locale -- this library's multibyte
encoding is UTF-8 -- with every destination filled with 0x55 first, and
prints one line per case:

    name ret=<n> errno=<0|EILSEQ|EINVAL|other> <out> <out> ...

where each out is rendered by kind:
    i hhi hi li u    the integer, decimal
    p                the pointer, hex
    f lf             the value's bits, hex (float 8 digits, double 16)
    Lf               the long double's 10 bytes, hex
    s c              the char buffer's first 16 bytes, hex (shows the NUL,
                     and what follows it)
    ls lc            the wchar_t buffer's first 8 elements, hex
    ms mls           the malloc'd string, hex, or "null"
    n                the int, decimal

The Rust test (posix/src/scanf.rs, `wscanf_is_glibcs`) renders ours the same
way and compares the lines.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from _wsl import emit_table, run, workdir, wsl_path  # noqa: E402

CASES = [
    # integers
    ("d", "%d", "42", "i"),
    ("d_two", "%d %d", "10 20", "i i"),
    ("d_sign", "%d", "-17", "i"),
    ("d_space", "%d", "   123", "i"),
    ("d_stop", "%d", "12abc", "i"),
    ("x", "%x", "0x1f", "i"),
    ("x_bare", "%x", "0xg", "i"),
    ("i_octal", "%i", "010", "i"),
    ("i_hex", "%i", "0x10", "i"),
    ("u_neg", "%u", "-1", "u"),
    ("hhd", "%hhd", "300", "hhi"),
    ("hd", "%hd", "70000", "hi"),
    ("ld", "%ld", "123456789012", "li"),
    ("d_width", "%2d%d", "12345", "i i"),
    ("p_nil", "%p", "(nil)", "p"),
    ("p_hex", "%p", "0x10", "p"),
    ("d_wide_digit", "%d", [0x661], "i"),
    # floats
    ("f", "%f", "3.5", "f"),
    ("lf", "%lf", "-2e3", "lf"),
    ("Lf_inf", "%Lf", "inf", "Lf"),
    ("f_nan", "%f", "nan", "f"),
    ("lf_infinity", "%lf", "infinity", "lf"),
    ("la_hex", "%la", "0x1p4", "lf"),
    ("f_e", "%f", "1e", "f"),
    ("f_dot", "%f", ".5", "f"),
    # strings
    ("s", "%s", "abc def", "s"),
    ("s_utf8", "%s", "h\u00e9llo w", "s"),
    ("s_width", "%3s", "abcdef", "s"),
    ("s_euro_width", "%2s", "\u20ac\u20acx", "s"),
    ("ls", "%ls", "ab\u20ac cd", "ls"),
    ("c", "%c", "\u00e9", "c"),
    ("c3", "%3c", "a\u00e9\u20ac", "c"),
    ("c_space", "%c", " x", "c"),
    ("lc", "%lc", "\u20ac", "lc"),
    ("lc2", "%2lc", "a\U0001f600", "lc"),
    ("suppress_s", "%*s%s", "a b", "s"),
    ("ms", "%ms", "h\u00e9llo", "ms"),
    ("mls", "%mls", "x\u20acy", "mls"),
    ("mc", "%m3c", "a\u00e9b", "ms"),
    # characters with no encoding
    ("s_surrogate", "%s", [0x61, 0xD800, 0x62], "s"),
    ("c_surrogate", "%c", [0xD800], "c"),
    ("c_surrogate_second", "%d %c", [0x35, 0x20, 0xD800], "i c"),
    ("suppressed_surrogate", "%*s %d", [0xD800, 0x20, 0x37], "i"),
    ("lc_surrogate", "%lc", [0xD800], "lc"),
    ("ls_surrogate", "%ls", [0x61, 0xD800], "ls"),
    ("set_surrogate", "%[^,]", [0x61, 0xD800, 0x2c], "s"),
    # scansets
    ("set", "%[abc]", "abcd", "s"),
    ("set_not", "%[^,]", "x,y", "s"),
    ("set_bracket_first", "%[]a]", "]a]b", "s"),
    ("set_range", "%[a-c]", "abcd", "s"),
    ("set_descending", "%[c-a]", "a-cb", "s"),
    ("set_dash_first", "%[-a]", "-a-b", "s"),
    ("set_dash_last", "%[a-]", "a-b", "s"),
    ("set_wide_range", "%l[\u00e9-\u00eb]", "\u00e9\u00ea\u00eb\u00ec", "ls"),
    ("set_euro", "%[\u20ac]", "\u20ac\u20acx", "s"),
    ("set_width", "%2[a-z]", "abc", "s"),
    ("set_none", "%[a]", "b", "s"),
    ("set_unclosed", "%[abc", "abc", "s"),
    ("set_empty_input", "%[a]", "", "s"),
    ("set_malloc", "%m[a-z]", "hello1", "ms"),
    ("set_range_start_self", "%[b-d]", "abd", "s"),
    # literals and whitespace
    ("lit", "a%d", "a5", "i"),
    ("lit_miss", "a%d", "b5", "i"),
    ("lit_utf8", "\u00e9%d", "\u00e95", "i"),
    ("lit_utf8_after_space", " \u00e9%d", "  \u00e95", "i"),
    ("lit_after_conv", "%d\u00e9%d", "1\u00e92", "i i"),
    ("percent", "%%%d", "%7", "i"),
    ("percent_space", "%% %d", "% 7", "i"),
    ("n", "%d%n", "12x", "i n"),
    ("n_wide", "%s%n", "\u00e9\u00e9 x", "s n"),
    ("n_after_space", "%d %n", "5   x", "i n"),
    ("space_end", "%d ", "5   ", "i"),
    # the end of the input
    ("eof", "%d", "", "i"),
    ("eof_space", "%d", "   ", "i"),
    ("partial", "%d %d", "1", "i i"),
    ("s_eof", "%s", "", "s"),
    ("c_eof", "%c", "", "c"),
    # positional arguments
    ("pos", "%2$d %1$d", "1 2", "i i"),
]


def cps(x):
    return [ord(ch) for ch in x] if isinstance(x, str) else list(x)


def c_array(name, points):
    return f"static const wchar_t {name}[] = {{{', '.join(hex(p) for p in points + [0])}}};"


def gen_c():
    out = [
        "#define _GNU_SOURCE",
        "#include <errno.h>",
        "#include <locale.h>",
        "#include <stdio.h>",
        "#include <stdlib.h>",
        "#include <string.h>",
        "#include <wchar.h>",
        "",
        "static void hexb(const unsigned char *p, int n) {",
        "  printf(\" \"); for (int i = 0; i < n; i++) printf(\"%02x\", p[i]);",
        "}",
        "static void hexw(const wchar_t *p, int n) {",
        "  printf(\" \"); for (int i = 0; i < n; i++) printf(\"%s%x\", i ? \".\" : \"\", (unsigned)p[i]);",
        "}",
        "static const char *ename(int e) {",
        "  return e == 0 ? \"0\" : e == EILSEQ ? \"EILSEQ\" : e == EINVAL ? \"EINVAL\" : \"other\";",
        "}",
        "",
    ]
    for k, (name, fmt, inp, kinds) in enumerate(CASES):
        out.append(c_array(f"f{k}", cps(fmt)))
        out.append(c_array(f"in{k}", cps(inp)))
    out.append("int main(void) {")
    out.append("  if (!setlocale(LC_ALL, \"C.UTF-8\")) { puts(\"no C.UTF-8\"); return 1; }")
    for k, (name, fmt, inp, kinds) in enumerate(CASES):
        ks = kinds.split()
        out.append("  {")
        decl, args, show = [], [], []
        for j, kind in enumerate(ks):
            v = f"v{j}"
            if kind in ("i", "n"):
                decl.append(f"int {v}; memset(&{v}, 0x55, sizeof {v});")
                args.append(f"&{v}")
                show.append(f"printf(\" %d\", {v});")
            elif kind == "hhi":
                decl.append(f"signed char {v}; memset(&{v}, 0x55, sizeof {v});")
                args.append(f"&{v}")
                show.append(f"printf(\" %d\", {v});")
            elif kind == "hi":
                decl.append(f"short {v}; memset(&{v}, 0x55, sizeof {v});")
                args.append(f"&{v}")
                show.append(f"printf(\" %d\", {v});")
            elif kind == "li":
                decl.append(f"long {v}; memset(&{v}, 0x55, sizeof {v});")
                args.append(f"&{v}")
                show.append(f"printf(\" %ld\", {v});")
            elif kind == "u":
                decl.append(f"unsigned {v}; memset(&{v}, 0x55, sizeof {v});")
                args.append(f"&{v}")
                show.append(f"printf(\" %u\", {v});")
            elif kind == "p":
                decl.append(f"void *{v}; memset(&{v}, 0x55, sizeof {v});")
                args.append(f"&{v}")
                show.append(f"printf(\" %lx\", (unsigned long){v});")
            elif kind == "f":
                decl.append(f"float {v}; memset(&{v}, 0x55, sizeof {v});")
                args.append(f"&{v}")
                show.append(f"{{ unsigned b; memcpy(&b, &{v}, 4); printf(\" %08x\", b); }}")
            elif kind == "lf":
                decl.append(f"double {v}; memset(&{v}, 0x55, sizeof {v});")
                args.append(f"&{v}")
                show.append(f"{{ unsigned long long b; memcpy(&b, &{v}, 8); printf(\" %016llx\", b); }}")
            elif kind == "Lf":
                decl.append(f"long double {v}; memset(&{v}, 0x55, sizeof {v});")
                args.append(f"&{v}")
                show.append(f"hexb((const unsigned char *)&{v}, 10);")
            elif kind in ("s", "c"):
                decl.append(f"char {v}[64]; memset({v}, 0x55, sizeof {v});")
                args.append(v)
                show.append(f"hexb((const unsigned char *){v}, 16);")
            elif kind in ("ls", "lc"):
                decl.append(f"wchar_t {v}[64]; memset({v}, 0x55, sizeof {v});")
                args.append(v)
                show.append(f"hexw({v}, 8);")
            elif kind == "ms":
                decl.append(f"char *{v} = (char *)1;")
                args.append(f"&{v}")
                show.append(f"if ({v} && {v} != (char *)1) {{ hexb((const unsigned char *){v}, (int)strlen({v}) + 1); free({v}); }} else printf(\" %s\", {v} ? \"untouched\" : \"null\");")
            elif kind == "mls":
                decl.append(f"wchar_t *{v} = (wchar_t *)1;")
                args.append(f"&{v}")
                show.append(f"if ({v} && {v} != (wchar_t *)1) {{ hexw({v}, (int)wcslen({v}) + 1); free({v}); }} else printf(\" %s\", {v} ? \"untouched\" : \"null\");")
            else:
                raise SystemExit(f"unknown kind {kind}")
        out.extend("    " + d for d in decl)
        call_args = ", ".join([f"in{k}", f"f{k}"] + args)
        out.append(f"    errno = 0; int r = swscanf({call_args}); int e = errno;")
        out.append(f"    printf(\"{name} ret=%d errno=%s\", r, ename(e));")
        out.extend("    " + s for s in show)
        out.append("    printf(\"\\n\");")
        out.append("  }")
    out.append("  return 0;")
    out.append("}")
    return "\n".join(out) + "\n"


def main():
    with workdir() as tmp:
        (Path(tmp) / "wscanf_oracle.c").write_text(gen_c(), encoding="utf-8", newline="\n")
        r = run(f"cd {wsl_path(tmp)} && gcc -O1 -w -o wscanf_oracle wscanf_oracle.c && "
                "LC_ALL=C.UTF-8 ./wscanf_oracle")
    if r.returncode != 0:
        sys.exit(f"oracle failed:\n{r.stdout}\n{r.stderr}")
    lines = [l for l in r.stdout.splitlines() if l.strip()]
    assert len(lines) == len(CASES), (len(lines), len(CASES))
    rows = []
    for (name, fmt, inp, kinds), line in zip(CASES, lines):
        assert line.split()[0] == name, (name, line)
        f = ", ".join(hex(p) for p in cps(fmt))
        i = ", ".join(hex(p) for p in cps(inp))
        rows.append(f'    ("{name}", &[{f}], &[{i}], "{kinds}", "{line}"),')
    header = ("// Generated by posix/tools/oracle/wscanf_harness.py from glibc 2.39's swscanf\n"
              "// under WSL, C.UTF-8: (name, format, input, kinds, glibc's line).\n")
    # `WscanfCase` is scanf.rs's name for the row type,
    # `(&str, &[u32], &[u32], &str, &str)`.
    body = ("const GLIBC_WSCANF: &[WscanfCase] = &[\n"
            + "\n".join(rows) + "\n];\n")
    emit_table(header + body, "scanf.rs")


if __name__ == "__main__":
    main()
