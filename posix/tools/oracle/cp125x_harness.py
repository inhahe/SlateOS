"""glibc 2.39's iconv for CP1255 and CP1258, as the oracle.

    python posix/tools/oracle/cp125x_harness.py           # print the table
    python posix/tools/oracle/cp125x_harness.py --check   # compare with iconv.rs's

A case is (name, to, from, chunks, outsize, reset): each chunk is converted
by one iconv call into a fresh output buffer of `outsize` bytes, then the
descriptor is reset -- `reset` "out": iconv(cd, NULL, NULL, &out, &left) into
a fresh `outsize` buffer; "null": iconv(cd, NULL, NULL, NULL, NULL).  One
line per case:

    name <call>;<call>;...|<reset>

where a call is `ret,errno,used,hex` (used = input bytes consumed) and the
reset `ret,errno,hex` (hex empty for "null").
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from _wsl import emit_table, run, workdir, wsl_path  # noqa: E402

U = lambda s: s.encode("utf-8")  # noqa: E731

CASES = [
    # CP1255 -> UTF-8: a letter is kept back until what follows is known.
    ("he_letters", "UTF-8", "CP1255", [b"\xe0\xe1\xe2"], 64, "out"),
    ("he_letters_null_reset", "UTF-8", "CP1255", [b"\xe0\xe1\xe2"], 64, "null"),
    ("he_alef_qamats", "UTF-8", "CP1255", [b"\xe0\xc8"], 64, "out"),
    ("he_shin_shindot", "UTF-8", "CP1255", [b"\xf9\xd1"], 64, "out"),
    ("he_shin_shindot_dagesh", "UTF-8", "CP1255", [b"\xf9\xd1\xcc"], 64, "out"),
    ("he_shin_dagesh_shindot", "UTF-8", "CP1255", [b"\xf9\xcc\xd1"], 64, "out"),
    ("he_letter_ascii", "UTF-8", "CP1255", [b"\xe0a"], 64, "out"),
    ("he_mark_alone", "UTF-8", "CP1255", [b"\xc8"], 64, "out"),
    ("he_two_marks", "UTF-8", "CP1255", [b"\xe0\xc8\xc8"], 64, "out"),
    ("he_across_calls", "UTF-8", "CP1255", [b"\xe0", b"\xc8"], 64, "out"),
    ("he_across_calls_noncomb", "UTF-8", "CP1255", [b"\xe0", b"b"], 64, "out"),
    ("he_invalid", "UTF-8", "CP1255", [b"\xe0\xca\xe1"], 64, "out"),
    ("he_invalid_ignore", "UTF-8//IGNORE", "CP1255", [b"\xe0\xca\xc8"], 64, "out"),
    ("he_symbols", "UTF-8", "CP1255", [b"\x80\xa4\x99\xfd\xfe"], 64, "out"),
    ("he_tight3", "UTF-8", "CP1255", [b"\xe0\xe1\xe2a"], 3, "out"),
    ("he_tight4", "UTF-8", "CP1255", [b"\xe0ab"], 4, "out"),
    ("he_to_wchar", "WCHAR_T", "CP1255", [b"\xe0\xc8\xe1"], 64, "out"),
    ("he_to_wchar_tight", "WCHAR_T", "CP1255", [b"a\xe0b"], 4, "out"),
    ("he_reset_no_room", "UTF-8", "CP1255", [b"\xe0"], 1, "out"),
    ("he_to_ascii_held", "ASCII", "CP1255", [b"a\xe0"], 64, "out"),
    ("he_to_ascii_translit_held", "ASCII//TRANSLIT", "CP1255", [b"a\xe0"], 64, "out"),
    # UTF-8 -> CP1255: a precomposed character is its letter and marks.
    ("he_enc_plain", "CP1255", "UTF-8", [U("\u05d0\u05d1a\u20aa")], 64, "out"),
    ("he_enc_fb2c", "CP1255", "UTF-8", [U("\ufb2c")], 64, "out"),
    ("he_enc_fb2f", "CP1255", "UTF-8", [U("\ufb2f")], 64, "out"),
    ("he_enc_fb4b", "CP1255", "UTF-8", [U("\ufb4b\ufb4c")], 64, "out"),
    ("he_enc_unwritable", "CP1255", "UTF-8", [U("a\u4e00b")], 64, "out"),
    ("he_enc_translit", "CP1255//TRANSLIT", "UTF-8", [U("a\u4e00b")], 64, "out"),
    ("he_enc_ignore", "CP1255//IGNORE", "UTF-8", [U("a\u4e00b")], 64, "out"),
    ("he_enc_tight_decomp", "CP1255", "UTF-8", [U("a\ufb2c")], 3, "out"),
    ("he_enc_tag", "CP1255", "UTF-8", [U("a\U000e0041b")], 64, "out"),
    # CP1258 -> UTF-8
    ("vi_a_grave", "UTF-8", "CP1258", [b"a\xcc"], 64, "out"),
    ("vi_e_tilde", "UTF-8", "CP1258", [b"e\xde"], 64, "out"),
    ("vi_A_dot", "UTF-8", "CP1258", [b"A\xf2"], 64, "out"),
    ("vi_ow_hook", "UTF-8", "CP1258", [b"\xf5\xd2"], 64, "out"),
    ("vi_word", "UTF-8", "CP1258", [b"Vi\xf2e\xecet"], 64, "out"),
    ("vi_mark_alone", "UTF-8", "CP1258", [b"\xcc"], 64, "out"),
    ("vi_nocompose", "UTF-8", "CP1258", [b"q\xcc"], 64, "out"),
    ("vi_across_calls", "UTF-8", "CP1258", [b"o", b"\xec"], 64, "out"),
    ("vi_invalid", "UTF-8", "CP1258", [b"a\xd0"], 64, "out"),
    ("vi_symbols", "UTF-8", "CP1258", [b"\x80\xfe\x99\xa0"], 64, "out"),
    ("vi_last_letter_null", "UTF-8", "CP1258", [b"xyz"], 64, "null"),
    # UTF-8 -> CP1258
    ("vi_enc_direct", "CP1258", "UTF-8", [U("\u00e0\u0102\u20ab")], 64, "out"),
    ("vi_enc_decomp", "CP1258", "UTF-8", [U("\u1ea1\u1ec7\u1ef9")], 64, "out"),
    ("vi_enc_tone_marks", "CP1258", "UTF-8", [U("a\u0340e\u0341")], 64, "out"),
    ("vi_enc_unwritable", "CP1258", "UTF-8", [U("a\u05d0")], 64, "out"),
    ("vi_enc_tight_decomp", "CP1258", "UTF-8", [U("ab\u1ea1")], 3, "out"),
    ("vi_enc_names", "WINDOWS-1258", "UTF-8", [U("\u1ea1")], 64, "out"),
    ("he_names", "UTF-8", "MS-HEBR", [b"\xe0\xc8"], 64, "out"),
    ("he_names2", "UTF-8", "WINDOWS-1255", [b"\xe0"], 64, "out"),
]


def c_bytes(b: bytes) -> str:
    return '"' + "".join(f"\\x{x:02x}" for x in b) + '"'


def gen_c() -> str:
    out = [
        "#define _GNU_SOURCE",
        "#include <errno.h>",
        "#include <iconv.h>",
        "#include <stdio.h>",
        "#include <string.h>",
        "static const char *en(int e) { return e == 0 ? \"0\" : e == EILSEQ ? \"EILSEQ\" : e == EINVAL ? \"EINVAL\" : e == E2BIG ? \"E2BIG\" : \"other\"; }",
        "static void hex(const unsigned char *p, size_t n) { for (size_t i = 0; i < n; i++) printf(\"%02x\", p[i]); }",
        "int main(void) {",
        "  setvbuf(stdout, NULL, _IONBF, 0);",
    ]
    for name, to, frm, chunks, outsize, reset in CASES:
        out.append("  {")
        out.append(f"    iconv_t cd = iconv_open(\"{to}\", \"{frm}\");")
        out.append(f"    if (cd == (iconv_t)-1) {{ printf(\"{name} open-failed\\n\"); }} else {{")
        out.append(f"    printf(\"{name} \");")
        for k, ch in enumerate(chunks):
            out.append("    {")
            out.append(f"      char in[] = {c_bytes(ch)}; size_t inl = {len(ch)};")
            out.append(f"      unsigned char ob[{max(outsize, 1)}]; size_t ol = {outsize};")
            out.append("      char *ip = in; char *op = (char *)ob; errno = 0;")
            out.append("      size_t r = iconv(cd, &ip, &inl, &op, &ol); int e = errno;")
            sep = ";" if k else ""
            out.append(f"      printf(\"{sep}%ld,%s,%zu,\", (long)r, r == (size_t)-1 ? en(e) : \"0\", (size_t)(ip - in));")
            out.append("      hex(ob, (size_t)(op - (char *)ob));")
            out.append("    }")
        if reset == "out":
            out.append("    {")
            out.append(f"      unsigned char ob[{max(outsize, 1)}]; size_t ol = {outsize};")
            out.append("      char *op = (char *)ob; errno = 0;")
            out.append("      size_t r = iconv(cd, NULL, NULL, &op, &ol); int e = errno;")
            out.append("      printf(\"|%ld,%s,\", (long)r, r == (size_t)-1 ? en(e) : \"0\");")
            out.append("      hex(ob, (size_t)(op - (char *)ob));")
            out.append("    }")
        else:
            out.append("    { errno = 0; size_t r = iconv(cd, NULL, NULL, NULL, NULL); int e = errno;")
            out.append("      printf(\"|%ld,%s,\", (long)r, r == (size_t)-1 ? en(e) : \"0\"); }")
        out.append("    printf(\"\\n\"); iconv_close(cd); }")
        out.append("  }")
    out.append("  return 0;")
    out.append("}")
    return "\n".join(out) + "\n"


def glibc_lines(stem: str) -> list:
    """Build and run the C program for `CASES`; glibc's line for each."""
    with workdir() as tmp:
        (Path(tmp) / f"{stem}.c").write_text(gen_c(), encoding="utf-8", newline="\n")
        r = run(f"cd {wsl_path(tmp)} && gcc -O1 -w -o {stem} {stem}.c && ./{stem}")
    if r.returncode != 0:
        sys.exit(f"oracle failed:\n{r.stdout}\n{r.stderr}")
    lines = [l for l in r.stdout.splitlines() if l.strip()]
    assert len(lines) == len(CASES), (len(lines), len(CASES))
    return lines


def table(const: str, harness: str, lines: list) -> str:
    """The Rust table of `CASES` and glibc's `lines`, as the test pastes it."""
    rows = []
    for (name, to, frm, chunks, outsize, reset), line in zip(CASES, lines):
        assert line.split(" ", 1)[0] == name, (name, line)
        ch = ", ".join("&[" + ", ".join(f"0x{b:02x}" for b in c) + "]" for c in chunks)
        rows.append(f'    ("{name}", "{to}", "{frm}", &[{ch}], {outsize}, "{reset}", "{line}"),')
    header = (f"// Generated by posix/tools/oracle/{harness} from glibc 2.39's iconv under WSL:\n"
              "// (name, to, from, chunks, outsize, reset, glibc's line).\n")
    # `OracleCase` is iconv.rs's name for the row type,
    # `(&str, &str, &str, &[&[u8]], usize, &str, &str)`.
    return (header + f"const {const}: &[OracleCase] = &[\n"
            + "\n".join(rows) + "\n];\n")


def main() -> None:
    lines = glibc_lines("cp125x_oracle")
    emit_table(table("GLIBC_CP125X", "cp125x_harness.py", lines), "iconv.rs")


if __name__ == "__main__":
    main()
