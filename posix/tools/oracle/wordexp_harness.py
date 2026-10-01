"""glibc 2.39's `wordexp` (`<wordexp.h>`) -- with the fixes to it Ubuntu's
2.39 carries (CVE-2025-15281, CVE-2026-6368, CVE-2026-6791) -- as the oracle
for posix/src/wordexp.rs's:

    python posix/tools/oracle/wordexp_harness.py   # writes posix/src/wordexp_oracle.txt

The program runs in a sandbox (`unshare -r -m`, no root needed) where:

- `/bin/sh` -- which glibc runs a command substitution with, as `sh -c
  <text>` -- is a wrapper that runs dash on the text and logs the text and
  every byte of its output, so each case can say which commands it ran and
  what they answered, for the tests to answer them the same way;
- `/etc/passwd` holds two users of the harness's own, `root` (home `/root`)
  and `wexuser` (home `/home/wex`), with `nsswitch.conf` reading files only;
- the working directory holds the files `one`, `two` and `three`, and
  nothing else, for pathname expansion.

One line a case:

    <flags> <ifs> <var> <home> <pre> <words> = <result> | <var after> | <commands>

`<flags>` is the flags' number; `<ifs>`, `<var>` and `<home>` are what IFS,
`var` and HOME are in the environment (`-` for unset); `<pre>` is `-`, or
the words of an earlier call the case builds on (with the case's flags but
WRDE_APPEND, and `we_offs` 3 when WRDE_DOOFFS is asked). `<result>` is
`<rc> <wordc> <offs> <word>...` -- the words from `we_offs` on -- when the
call answered 0 or WRDE_NOSPACE, and `<rc> kept` or `<rc> changed` when it
failed otherwise, by whether it left the structure as it was, as glibc's
own test asks. `<var after>` is `var` after the call (`${var=...}` sets
it), and `<commands>` the commands run, each `<text>=<output>`, `-` for
none. Text is written as posix/src/resolv.rs's tests write it (a byte from
`!` to `~` as itself but `\\`, any other as `\\xHH`, the empty text `\\x`).
"""

import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from _wsl import POSIX_SRC, run, workdir, wsl_path  # noqa: E402

OUT = POSIX_SRC / "wordexp_oracle.txt"

WRDE_DOOFFS, WRDE_APPEND, WRDE_NOCMD, WRDE_REUSE, WRDE_SHOWERR, WRDE_UNDEF = 1, 2, 4, 8, 16, 32
IFS = " \n\t"

# (words, var, flags, ifs) -- glibc's posix/wordexp-test.c, its table in order.
GLIBC_TABLE = [
    ("one", None, 0, IFS), ("one two", None, 0, IFS), ("one two three", None, 0, IFS),
    (" \tfoo\t\tbar ", None, 0, IFS), ("red , white blue", None, 0, " ,"),
    ("one two three", None, 0, ""), ("one \"two three\"", None, 0, IFS),
    ("one \"two three\"", None, 0, ""), ("one \"$var\"", "two three", 0, IFS),
    ("one $var", "two three", 0, IFS), ("one \"$var\"", "two three", 0, ""),
    ("one $var", "two three", 0, ""), ("$var", ":abc:", 0, ":"),
    ("$(echo :abc:)", None, 0, ":"), ("$(echo :abc:\\ )", None, 0, ": "),
    ("$(echo :abc\\ )", None, 0, ": "), ("$(echo $var)", ":abc:", 0, ":"),
    (":abc:", None, 0, ":"), ("$(echo :abc:)def", None, 0, ":"), ("$(echo abc:de)f", None, 0, ":"),
    ("$(echo abc:de)f:ghi", None, 0, ":"), ("abc:d$(echo ef:ghi)", None, 0, ":"),
    ("$var$(echo def:ghi)", "abc:", 0, ":"), ("$var$(echo ef:ghi)", "abc:d", 0, ":"),
    ("$(echo abc:)$var", "def:ghi", 0, ":"), ("$(echo abc:d)$var", "ef:ghi", 0, ":"),
    ("${var}", "foo", 0, IFS), ("$var", "foo", 0, IFS), ("\\\"$var\\\"", "foo", 0, IFS),
    ("%$var%", "foo", 0, IFS), ("-$var-", "foo", 0, IFS),
    ("\"quoted\"", None, 0, IFS), ("\"$var\"\"$var\"", "foo", 0, IFS), ("'singly-quoted'", None, 0, IFS),
    ("contin\\\nuation", None, 0, IFS), ("explicit ''", None, 0, IFS), ("explicit \"\"", None, 0, IFS),
    ("explicit ``", None, 0, IFS),
    ("$(echo hello)", None, 0, IFS), ("$( (echo hello) )", None, 0, IFS),
    ("$((echo hello);(echo there))", None, 0, IFS), ("`echo one two`", None, 0, IFS),
    ("$(echo ')')", None, 0, IFS), ("$(echo hello; echo)", None, 0, IFS), ("a$(echo b)c", None, 0, IFS),
    ("$((1 + 1))", None, 0, IFS), ("$((2-3))", None, 0, IFS), ("$((-1))", None, 0, IFS),
    ("$[50+20]", None, 0, IFS), ("$(((2+3)*(4+5)))", None, 0, IFS), ("$((010))", None, 0, IFS),
    ("$((0x10))", None, 0, IFS), ("$((010+0x10))", None, 0, IFS), ("$((-010+0x10))", None, 0, IFS),
    ("$((-0x10+010))", None, 0, IFS), ("$(())", None, 0, IFS), ("$[]", None, 0, IFS),
    ("${var:-bar}", None, 0, IFS), ("${var-bar}", None, 0, IFS), ("${var:-bar}", "", 0, IFS),
    ("${var:-bar}", "foo", 0, IFS), ("${var-bar}", "", 0, IFS), ("${var:=bar}", None, 0, IFS),
    ("${var=bar}", None, 0, IFS), ("${var:=bar}", "", 0, IFS), ("${var:=bar}", "foo", 0, IFS),
    ("${var=bar}", "", 0, IFS), ("${var:?bar}", "foo", 0, IFS), ("${var:+bar}", None, 0, IFS),
    ("${var+bar}", None, 0, IFS), ("${var:+bar}", "", 0, IFS), ("${var:+bar}", "foo", 0, IFS),
    ("${var+bar}", "", 0, IFS), ("${#var}", "12345", 0, IFS), ("${var:-'}'}", None, 0, IFS),
    ("${var-}", None, 0, IFS), ("${a?}", None, 0, IFS), ("${#a=}", None, 0, IFS),
    ("${var#${var}}", "pizza", 0, IFS), ("${var%$(echo oni)}", "pepperoni", 0, IFS),
    ("${var#$((6))}", "6pack", 0, IFS), ("${var##b*}", "b*witched", 0, IFS),
    ("${var##\"b*\"}", "b*witched", 0, IFS), ("${var%na*}", "banana", 0, IFS),
    ("${var%%na*}", "banana", 0, IFS), ("${var#*bora}", "borabora-island", 0, IFS),
    ("${var##*bora}", "borabora-island", 0, IFS), ("${var##\\*co}", "coconut", 0, IFS),
    ("${var%0%}", "100%", 0, IFS),
    ("???", None, 0, IFS), ("[ot]??", None, 0, IFS), ("t*", None, 0, IFS), ("\"t\"*", None, 0, IFS),
    ("$var", "one two", 0, IFS), ("$var", "one two three", 0, IFS), ("$var", " \tfoo\t\tbar ", 0, IFS),
    ("$var", "  red  , white blue", 0, ", \n\t"), ("\"$var\"", "  red  , white blue", 0, ", \n\t"),
    ("\"$(echo hello there)\"", None, 0, IFS), ("\"$(echo \"hello there\")\"", None, 0, IFS),
    ("${var=one two} \"$var\"", None, 0, IFS), ("$(( $(echo 3)+$var ))", "1", 0, IFS),
    ("\"$(echo \"*\")\"", None, 0, IFS), ("\"a\n\n$(echo)b\"", None, 0, IFS), ("*$var*", "foo", 0, IFS),
    ("*$var*", "o thr", 0, IFS),
    ("$var", "a b\tc\nd  ", 0, None), ("$var", "a b\tc d  ", 0, ""), ("$var", "a,b c\n, d", 0, "\t\n,"),
    ("\\*\"|&;<>\"\\(\\)\\{\\}", None, 0, IFS), ("$var", "???", 0, IFS), ("$var", None, 0, IFS),
    ("\"\\n\"", None, 0, IFS), ("", None, 0, IFS), ("${1234567890123456789012}", None, 0, IFS),
    ("one two", None, WRDE_DOOFFS, IFS), ("appended", None, WRDE_APPEND, IFS),
    ("appended", None, WRDE_DOOFFS | WRDE_APPEND, IFS),
    ("new\nline", None, 0, ""), ("pipe|symbol", None, 0, IFS), ("&ampersand", None, 0, IFS),
    ("semi;colon", None, 0, IFS), ("<greater", None, 0, IFS), ("less>", None, 0, IFS),
    ("(open-paren", None, 0, IFS), ("close-paren)", None, 0, IFS), ("{open-brace", None, 0, IFS),
    ("close-brace}", None, 0, IFS), ("$var", None, WRDE_UNDEF, IFS), ("$9", None, WRDE_UNDEF, IFS),
    ("$[50+20))", None, 0, IFS), ("${%%noparam}", None, 0, IFS), ("${missing-brace", None, 0, IFS),
    ("$(for i in)", None, 0, IFS), ("$((2+))", None, 0, IFS), ("`", None, 0, IFS),
    ("$((010+4+))", None, 0, IFS), ("`\\", None, 0, IFS), ("${", None, 0, IFS), ("L${a:", None, 0, IFS),
]

# This harness's own: the edges of each expansion, and what the old
# implementation cut short.
MORE = [
    # Tildes, with HOME and without; users known and not.
    ("~", None, 0, IFS), ("~/x", None, 0, IFS), ("~ ~/foo", None, 0, IFS), ("~root", None, 0, IFS),
    ("~root/sub", None, 0, IFS), ("~wexuser", None, 0, IFS), ("~wexuser/a b", None, 0, IFS),
    ("~nosuchuser/x", None, 0, IFS), ("a~", None, 0, IFS), ("x=~", None, 0, IFS), ("\"~\"", None, 0, IFS),
    ("'~'", None, 0, IFS), ("\\~", None, 0, IFS), ("~\"root\"", None, 0, IFS), ("${var:-~}", None, 0, IFS),
    ("${var#~root}x", "/root", 0, IFS), ("~" + "A" * 5000 + "/rest", None, 0, IFS),
    ("~" + "x" * 2048 + "/", None, 0, IFS),
    # Parameters: special ones, positional ones, lengths, patterns.
    ("$#", None, 0, IFS), ("$1", None, 0, IFS), ("${1}", None, 0, IFS), ("$@", None, 0, IFS),
    ("\"$@\"", None, 0, IFS), ("$*", None, 0, IFS), ("$?", None, 0, IFS), ("$-", None, 0, IFS),
    ("$!", None, 0, IFS), ("${#}", None, 0, IFS), ("${#var}", None, 0, IFS), ("${#var}", "", 0, IFS),
    ("${var:?}", None, 0, IFS), ("${var:?msg}", "", 0, IFS), ("${var?msg}", "", 0, IFS),
    ("${var:+a b}", "x", 0, IFS), ("\"${var:+a b}\"", "x", 0, IFS), ("${var:-a b}", None, 0, IFS),
    ("\"${var:-a b}\"", None, 0, IFS), ("${var#a}", "abc", 0, IFS), ("${var#?}", "abc", 0, IFS),
    ("${var%?}", "abc", 0, IFS), ("${var%%*}", "abc", 0, IFS), ("${var##*}", "abc", 0, IFS),
    ("${var#[ab]}", "abc", 0, IFS), ("${var%[!c]}", "abc", 0, IFS), ("${var#}", "abc", 0, IFS),
    ("${var%'b'c}", "abc", 0, IFS), ("${var#\"a\"}", "abc", 0, IFS), ("${var}x${var}", "a b", 0, IFS),
    ("${var:-$(echo nested)}", None, 0, IFS), ("${var:=$((2*3))}", None, 0, IFS),
    ("$var", None, WRDE_UNDEF, IFS), ("${var}", None, WRDE_UNDEF, IFS),
    ("${var:-x}", None, WRDE_UNDEF, IFS), ("$1", None, WRDE_UNDEF, IFS), ("$var", "set", WRDE_UNDEF, IFS),
    ("$", None, 0, IFS), ("a$", None, 0, IFS), ("$ x", None, 0, IFS), ("$%", None, 0, IFS),
    ("$=", None, 0, IFS), ("${}", None, 0, IFS), ("${var", None, 0, IFS), ("${var:x}", "a", 0, IFS),
    ("${var!}", "a", 0, IFS), ("$var_x", "a", 0, IFS), ("${var_x}", None, 0, IFS),
    # Arithmetic.
    ("$((7%3))", None, 0, IFS), ("$((-7/2))", None, 0, IFS), ("$((2*(3+4)))", None, 0, IFS),
    ("$(( $((1+1)) * 2 ))", None, 0, IFS), ("$((var+1))", "4", 0, IFS), ("$(($var*2))", "4", 0, IFS),
    ("$((1<<2))", None, 0, IFS), ("$((1/0))", None, 0, IFS), ("$((9223372036854775807+1))", None, 0, IFS),
    ("$((-9223372036854775808/-1))", None, 0, IFS), ("$((08))", None, 0, IFS), ("$((0x))", None, 0, IFS),
    ("$((  12  ))", None, 0, IFS), ("$((+5))", None, 0, IFS), ("$((--5))", None, 0, IFS),
    ("\"$((1+2))\"", None, 0, IFS), ("$((1+2", None, 0, IFS), ("$[1+2", None, 0, IFS),
    ("$[(-0)/(-1)]", None, WRDE_NOCMD, IFS), ("$[(-1)/(-1)]", None, WRDE_NOCMD, IFS),
    ("$[(-65536)/(-1)]", None, WRDE_NOCMD, IFS), ("$[(-2147483648)/(-1)]", None, WRDE_NOCMD, IFS),
    ("$[(-42949672969223372036854775808)/(-1)]", None, WRDE_NOCMD, IFS),
    ("$[(-18446744073709551616)/(-1)]", None, WRDE_NOCMD, IFS),
    ("$[(-9223372036854775808)/(-1)]", None, WRDE_NOCMD, IFS),
    ("$[1/0]", None, WRDE_NOCMD, IFS),
    # Command substitution, and its refusal.
    ("$(ls)", None, WRDE_NOCMD, IFS), ("$((`echo 1`))", None, WRDE_NOCMD, IFS),
    ("$((1+`echo 1`))", None, WRDE_NOCMD, IFS), ("$((1+$((`echo 1`))))", None, WRDE_NOCMD, IFS),
    ("`echo x`", None, WRDE_NOCMD, IFS), ("\"$(echo x)\"", None, WRDE_NOCMD, IFS),
    ("'$(echo x)'", None, WRDE_NOCMD, IFS), ("\\$(echo x)", None, WRDE_NOCMD, IFS),
    ("$(echo Test)", None, 0, IFS), ("$(printf 'a\\n\\n\\n')", None, 0, IFS),
    ("$(printf 'a b\\tc')", None, 0, IFS), ("$(echo err >&2; echo out)", None, 0, IFS),
    ("$(echo err >&2; echo out)", None, WRDE_SHOWERR, IFS), ("`echo \\`echo in\\``", None, 0, IFS),
    ("`echo '\\$var'`", "v", 0, IFS), ("$(echo $var)", "a  b", 0, IFS), ("\"$(echo $var)\"", "a  b", 0, IFS),
    ("$(exit 3)", None, 0, IFS), ("x$(printf '')y", None, 0, IFS), ("$(echo \"(\")", None, 0, IFS),
    ("$(case x in x) echo y;; esac)", None, 0, IFS), ("$(echo", None, 0, IFS), ("`echo", None, 0, IFS),
    # Quoting.
    ("'a\"b'", None, 0, IFS), ("\"a'b\"", None, 0, IFS), ("\"a\\\"b\"", None, 0, IFS), ("a\\ b", None, 0, IFS),
    ("\"\\$var\"", "v", 0, IFS), ("'$var'", "v", 0, IFS), ("\"${var}\"", "v w", 0, IFS),
    ("\"a\"'b'c", None, 0, IFS), ("\"\\a\\b\"", None, 0, IFS), ("\\", None, 0, IFS), ("\"unterminated", None, 0, IFS),
    ("'unterminated", None, 0, IFS), ("\"\\`\"", None, 0, IFS), ("\"a|b&c;d\"", None, 0, IFS),
    ("'(){}<>'", None, 0, IFS), ("a\\\nb", None, 0, IFS), ("\"a\\\nb\"", None, 0, IFS),
    ("'a\\\nb'", None, 0, IFS), ("#comment", None, 0, IFS), ("a #b", None, 0, IFS),
    # Splitting, and globbing what splitting made.
    ("$var", "a::b", 0, ":"), ("$var", "a: :b", 0, ": "), ("$var", ":", 0, ":"), ("$var", "::", 0, ":"),
    ("x$var", ":a", 0, ":"), ("$var$var", "a:", 0, ":"), ("\"$var\"x", "a b", 0, IFS),
    ("$var", "t*", 0, IFS), ("\"$var\"", "t*", 0, IFS), ("$var", "nomatch*", 0, IFS),
    ("*", None, 0, IFS), ("[", None, 0, IFS), ("[]", None, 0, IFS), ("\\*", None, 0, IFS),
    ("o[n]e", None, 0, IFS), ("?[wh]*", None, 0, IFS),
    # Big: past the old implementation's 4096 bytes and 256 words.
    ("w" * 5000, None, 0, IFS), (" ".join(f"w{i}" for i in range(300)), None, 0, IFS),
]

# (words, flags, pre-words): the structure the call is given, built by an
# earlier call -- WRDE_APPEND onto it, WRDE_REUSE of it, and the state an
# error must leave -- the cases of glibc's own tst-wordexp-append.c and
# tst-wordexp-reuse.c.
SEQUENCES = [
    ("extra )", WRDE_APPEND, "one two three"), ("append )", WRDE_APPEND, "a b c d e f g h"),
    ("delta )", WRDE_APPEND, "alpha beta gamma"), ("world", WRDE_APPEND, "hello"),
    ("bad |", WRDE_APPEND, "first"), ("second third", WRDE_APPEND, "first"),
    ("x )", WRDE_APPEND, "keep this"), ("x |", WRDE_APPEND, "keep this"), ("x ;", WRDE_APPEND, "keep this"),
    ("x &", WRDE_APPEND, "keep this"), ("x <", WRDE_APPEND, "keep this"), ("x >", WRDE_APPEND, "keep this"),
    ("\"unterminated", WRDE_APPEND, "original"), ("|", WRDE_APPEND, "hello world"),
    ("three", WRDE_APPEND | WRDE_DOOFFS, "one two"), ("gamma )", WRDE_APPEND | WRDE_DOOFFS, "alpha beta"),
    ("$var", WRDE_APPEND | WRDE_UNDEF, "one"), ("$(ls)", WRDE_APPEND | WRDE_NOCMD, "one"),
    ("a b", WRDE_REUSE, "one two three"), ("a", WRDE_REUSE | WRDE_APPEND, "x y"),
    ("a b c", WRDE_REUSE | WRDE_DOOFFS, "one"), ("|", WRDE_REUSE, "one two"),
    ("", WRDE_APPEND, "one"), ("", WRDE_REUSE, "one"),
]


def token(b: bytes) -> str:
    if not b:
        return "\\x"
    return "".join(chr(c) if 0x21 <= c <= 0x7E and c != 0x5C else f"\\x{c:02x}" for c in b)


def c_str(b: bytes) -> str:
    return '"' + "".join(f"\\x{c:02x}" for c in b) + '"'


def c_escape(s: str) -> str:
    return s.replace("\\", "\\\\").replace('"', '\\"')


def tok(s):
    return "-" if s is None else token(s.encode())


PASSWD = b"root:x:0:0:root:/root:/bin/sh\nwexuser:x:1000:1000:wex:/home/wex:/bin/sh\n"
NSSWITCH = b"passwd: files\ngroup: files\n"

# The /bin/sh the sandbox gives glibc: dash, logging. `-c <text>` runs the
# text, logged with every byte it printed; `-nc <text>` -- glibc's check,
# when a command printed nothing, of whether it was a syntax error -- is
# logged with its exit status.
WRAPPER = r'''#!/usr/bin/bash
hex() { od -An -tx1 -v | tr -d ' \n'; }
if [ "$1" = "-c" ] || [ "$1" = "-nc" ]; then
    out="$LOGDIR/out.$$"
    "$REALSH" "$1" "$2" > "$out"
    rc=$?
    cat "$out"
    {
        printf '%s ' "${1#-}"
        printf '%s' "$2" | hex
        printf ' '
        if [ "$1" = "-c" ]; then hex < "$out"; printf ' '; fi
        printf '%d\n' "$rc"
    } >> "$LOGDIR/log"
    rm -f "$out"
    exit $rc
fi
exec "$REALSH" "$@"
'''


def c_program(cases) -> str:
    L = [r'''#define _GNU_SOURCE
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <wordexp.h>

static void text(const char *s, size_t n)
{
    if (n == 0) {
        fputs("\\x", stdout);
        return;
    }
    for (size_t i = 0; i < n; i++) {
        unsigned char c = (unsigned char) s[i];
        if (c >= 0x21 && c <= 0x7e && c != '\\')
            putchar(c);
        else
            printf("\\x%02x", c);
    }
}

static void set(const char *name, const char *value)
{
    if (value)
        setenv(name, value, 1);
    else
        unsetenv(name);
}

/* `h`, hex, written as text. */
static void unhex(const char *h, size_t len)
{
    size_t n = len / 2;
    char *b = malloc(n + 1);
    for (size_t i = 0; i < n; i++) {
        unsigned v;
        sscanf(h + 2 * i, "%2x", &v);
        b[i] = (char) v;
    }
    text(b, n);
    free(b);
}

/* The commands the logging /bin/sh ran since the last call, space apart --
   `c <text> <output> <status>` for each run, `n <text> <status>` for each
   syntax check -- written when `show`; the log is emptied either way. */
static void commands(const char *log, int show)
{
    FILE *f = fopen(log, "r");
    int any = 0;
    static char line[1 << 20];
    while (f && fgets(line, sizeof line, f)) {
        char *nl = strchr(line, '\n');
        if (nl)
            *nl = '\0';
        char *field[4] = {line, NULL, NULL, NULL};
        int n = 1;
        for (char *p = line; *p && n < 4; p++)
            if (*p == ' ') {
                *p = '\0';
                field[n++] = p + 1;
            }
        int run = strcmp(field[0], "c") == 0;
        if (n != (run ? 4 : 3))
            continue;
        if (show) {
            fputs(any ? " " : "", stdout);
            fputs(run ? "c " : "n ", stdout);
            unhex(field[1], strlen(field[1]));
            putchar(' ');
            if (run) {
                unhex(field[2], strlen(field[2]));
                putchar(' ');
            }
            fputs(field[run ? 3 : 2], stdout);
        }
        any = 1;
    }
    if (!any && show)
        putchar('-');
    if (f)
        fclose(f);
    FILE *t = fopen(log, "w");
    if (t)
        fclose(t);
}

static void one(const char *head, const char *log, const char *words, int flags, const char *ifs,
                const char *var, const char *home, const char *pre)
{
    set("IFS", ifs);
    set("var", var);
    set("HOME", home);
    wordexp_t we;
    char *dummy;
    we.we_wordc = 99;
    we.we_wordv = &dummy;
    we.we_offs = 3;
    if (pre) {
        if (wordexp(pre, &we, flags & ~(WRDE_APPEND | WRDE_REUSE)) != 0) {
            printf("%s = setup-failed\n", head);
            return;
        }
    }
    commands(log, 0);   /* forget the setup's commands */
    wordexp_t sav = we;
    int rc = wordexp(words, &we, flags);
    printf("%s = %d", head, rc);
    if (rc == 0 || rc == WRDE_NOSPACE) {
        printf(" %zu %zu", we.we_wordc, we.we_offs);
        for (size_t i = 0; i < we.we_offs; i++)
            if (we.we_wordv[i] != NULL)
                fputs(" OFFS-NOT-NULL", stdout);
        for (size_t i = 0; i < we.we_wordc; i++) {
            putchar(' ');
            text(we.we_wordv[we.we_offs + i], strlen(we.we_wordv[we.we_offs + i]));
        }
        if (we.we_wordv[we.we_offs + we.we_wordc] != NULL)
            fputs(" UNTERMINATED", stdout);
        wordfree(&we);
    } else {
        int kept = we.we_wordc == sav.we_wordc && we.we_wordv == sav.we_wordv
                   && we.we_offs == sav.we_offs;
        printf(" %s", kept ? "kept" : "changed");
        if (pre)
            wordfree(&we);
    }
    fputs(" | ", stdout);
    const char *after = getenv("var");
    if (after)
        text(after, strlen(after));
    else
        putchar('-');
    fputs(" | ", stdout);
    commands(log, 1);
    putchar('\n');
}

int main(void)
{
    /* The log's path comes in the environment: an argument would be $1. */
    const char *log = getenv("WEXLOG");
''']
    for (words, var, flags, ifs, home, pre) in cases:
        head = " ".join([str(flags), tok(ifs), tok(var), tok(home), tok(pre), token(words.encode())])
        args = [c_str(words.encode()), str(flags), "NULL" if ifs is None else c_str(ifs.encode()),
                "NULL" if var is None else c_str(var.encode()), "NULL" if home is None else c_str(home.encode()),
                "NULL" if pre is None else c_str(pre.encode())]
        L.append(f'    one("{c_escape(head)}", log, {", ".join(args)});')
    L.append("    return 0;\n}")
    return "\n".join(L) + "\n"


def cases():
    out = []
    for words, var, flags, ifs in GLIBC_TABLE + MORE:
        pre = "pre1 pre2" if flags & WRDE_APPEND else None
        out.append((words, var, flags, ifs, "/dummy/home", pre))
    # HOME unset: the user's home from passwd.
    for words in ["~", "~/x", "${var:-~}"]:
        out.append((words, None, 0, IFS, None, None))
    for words, flags, pre in SEQUENCES:
        out.append((words, None, flags, IFS, "/dummy/home", pre))
    return out


def main() -> int:
    cs = cases()
    with workdir() as d:
        w = Path(d)
        (w / "wordexp.c").write_text(c_program(cs), encoding="utf-8", newline="\n")
        (w / "sh").write_text(WRAPPER, encoding="utf-8", newline="\n")
        (w / "passwd").write_bytes(PASSWD)
        (w / "nsswitch.conf").write_bytes(NSSWITCH)
        g = w / "glob"
        g.mkdir()
        for n in ("one", "two", "three"):
            (g / n).write_bytes(b"")
        wd = wsl_path(w)
        script = (
            f"cd {wd} && gcc -O0 -Wall -Werror -o wordexp wordexp.c && chmod +x sh && "
            f"cp /usr/bin/dash {wd}/realsh && : > {wd}/log && "
            f"unshare -r -m sh -c '"
            f"e=/tmp/etcw.$$; mkdir -p $e && mount -t tmpfs none $e && (cp -a /etc/. $e/ 2>/dev/null; true) && "
            f"cp {wd}/passwd {wd}/nsswitch.conf $e/ && mount --bind $e /etc && "
            f"mount --bind {wd}/sh /usr/bin/dash && "
            f"cd {wd}/glob && REALSH={wd}/realsh LOGDIR={wd} WEXLOG={wd}/log ../wordexp'"
        )
        r = run(script)
    if r.returncode != 0:
        sys.stderr.write(r.stdout[-4000:] + r.stderr[-4000:])
        return 1
    header = ("# glibc 2.39's wordexp, with Ubuntu's 2026 fixes (posix/tools/oracle/wordexp_harness.py):\n"
              "# one line a case -- see the harness for the forms.\n")
    OUT.write_text(header + r.stdout, encoding="utf-8", newline="\n")
    print(f"wrote {OUT} ({len(r.stdout.splitlines())} cases)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
