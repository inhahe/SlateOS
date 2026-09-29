"""glibc 2.39's `getdate` and `getdate_r`, as the oracle for posix/src/time.rs.

    python posix/tools/oracle/getdate_harness.py   # writes posix/src/getdate_oracle.txt

`getdate` matches its input against the templates -- `strptime` formats, one
per line -- of the file `DATEMSK` names, and fills in what the matching one
left out from the current time: today for a time alone, the next such
weekday for a weekday, the first of the next such month for a month. So each
call is made with the second it ran in recorded, and made again if the
second changed during it; the test replays it with that second as "now".

    T <template-hex> ...                      the templates, in file order
    g <zone> <now> <input-hex> = <ret> <getdate_err> <tm>
    r <zone> <now> <input-hex> = <getdate_r's result> <tm>
    E <case> = <ret> <getdate_err>

`<tm>` is `sec min hour mday mon year wday yday isdst`, or `-` for a NULL
`getdate`. The `E` rows are the errors that need no template: `DATEMSK`
unset or empty, naming no file, naming a directory, naming an empty file.
Built with -fno-builtin.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from _wsl import POSIX_SRC, run, workdir, wsl_path  # noqa: E402

OUT = POSIX_SRC / "getdate_oracle.txt"
ZONES = ["UTC0", "EST5EDT,M3.2.0,M11.1.0"]

TEMPLATES = [
    "%m/%d/%y %H:%M", "%m/%d/%Y", "%Y-%m-%d %H:%M:%S", "%Y-%m-%d", "%A", "%a %H:%M", "%B %Y",
    "%B %d", "%d %B", "%B", "%A %B", "%H:%M:%S", "%H:%M", "%I %p", "%H", "%j %Y", "%U %A %Y",
    "%s seconds", "%c", "%Y", "%y %m %d %H",
]

INPUTS = [
    "10/05/26 14:30", "12/31/1999", "2/29/2024", "2/29/2023", "2/30/2024", "2026-09-28 10:35:00",
    "2026-09-28", "2026-02-30", "2026-13-01", "Monday", "sunday", "SATURDAY", "wed 09:15",
    "Fri 23:59", "September 2030", "march 5", "5 April", "31 June", "September", "January",
    "December", "Tuesday May", "13:45:10", "13:45", "00:00", "23:59", "07", "23", "11 PM",
    "12 AM", "100 2026", "366 2023", "38 Friday 2026", "0 sunday 2026", "0 seconds",
    "1727519700 seconds", "Mon Sep 28 10:35:00 2026", "2027", "1969", "26 09 28 10",
    "  2026-09-28  ", "\t13:45\n", "", " ", "nonsense", "2026-09-28x", "25:00",
    "9999-12-31 23:59:59", "0-01-01",
]


def cstr(s: str) -> str:
    out = []
    for b in s.encode("utf-8"):
        if b in (0x22, 0x5C):
            out.append("\\" + chr(b))
        elif 0x20 <= b < 0x7F and b != 0x3F:
            out.append(chr(b))
        else:
            out.append(f"\\{b:03o}")
    return '"' + "".join(out) + '"'


def gen_c() -> str:
    c = [
        "#define _GNU_SOURCE",
        "#include <errno.h>", "#include <stdio.h>", "#include <stdlib.h>", "#include <string.h>",
        "#include <time.h>", "#include <unistd.h>", "#include <sys/stat.h>",
        "static const char *const zones[] = {" + ", ".join(f'"{z}"' for z in ZONES) + "};",
        "static const char *const ins[] = {" + ", ".join(cstr(s) for s in INPUTS) + "};",
        "static const char *const tmpl[] = {" + ", ".join(cstr(s) for s in TEMPLATES) + "};",
        "static void hexs(const char *s) { if (!*s) { printf(\"-\"); return; }",
        "  for (; *s; s++) printf(\"%02x\", (unsigned char)*s); }",
        "static void put_tm(const struct tm *t) {",
        "  printf(\"%d %d %d %d %d %d %d %d %d\", t->tm_sec, t->tm_min, t->tm_hour, t->tm_mday,",
        "         t->tm_mon, t->tm_year, t->tm_wday, t->tm_yday, t->tm_isdst);",
        "}",
        "int main(int argc, char **argv) {",
        "  (void)argc;",
        "  const char *dir = argv[1];",
        "  char path[4096];",
        "  snprintf(path, sizeof path, \"%s/templates\", dir);",
        "  FILE *f = fopen(path, \"w\");",
        "  for (unsigned i = 0; i < sizeof tmpl / sizeof tmpl[0]; i++) fprintf(f, \"%s\\n\", tmpl[i]);",
        "  fclose(f);",
        "  printf(\"T\");",
        "  for (unsigned i = 0; i < sizeof tmpl / sizeof tmpl[0]; i++) { printf(\" \"); hexs(tmpl[i]); }",
        "  printf(\"\\n\");",
        "  setenv(\"DATEMSK\", path, 1);",
        "  for (unsigned z = 0; z < sizeof zones / sizeof zones[0]; z++) {",
        "    setenv(\"TZ\", zones[z], 1); tzset();",
        "    for (unsigned i = 0; i < sizeof ins / sizeof ins[0]; i++) {",
        "      for (int which = 0; which < 2; which++) {",
        "        for (int attempt = 0; attempt < 5; attempt++) {",
        "          time_t before = time(NULL);",
        "          struct tm t, *r = NULL; int rc = 0, err;",
        "          memset(&t, 0, sizeof t);",
        "          getdate_err = 0;",
        "          if (which) rc = getdate_r(ins[i], &t); else r = getdate(ins[i]);",
        "          err = getdate_err;",
        "          time_t after = time(NULL);",
        "          if (before != after) continue;",
        "          if (which) {",
        "            printf(\"r %u %lld \", z, (long long)before); hexs(ins[i]);",
        "            printf(\" = %d \", rc); put_tm(&t); printf(\"\\n\");",
        "          } else {",
        "            printf(\"g %u %lld \", z, (long long)before); hexs(ins[i]);",
        "            printf(\" = %d %d \", r != NULL, err);",
        "            if (r) put_tm(r); else printf(\"-\");",
        "            printf(\"\\n\");",
        "          }",
        "          break;",
        "        }",
        "      }",
        "    }",
        "  }",
        "  /* The errors that need no template. */",
        "  struct { const char *name; const char *value; } cases[] = {",
        "    {\"unset\", NULL}, {\"empty\", \"\"}, {\"missing\", \"/nonexistent/templates\"},",
        "    {\"directory\", \"/tmp\"}, {\"nofile\", NULL},",
        "  };",
        "  char empty[4096];",
        "  snprintf(empty, sizeof empty, \"%s/empty\", dir);",
        "  fclose(fopen(empty, \"w\"));",
        "  cases[4].name = \"emptyfile\"; cases[4].value = empty;",
        "  for (unsigned i = 0; i < sizeof cases / sizeof cases[0]; i++) {",
        "    if (cases[i].value) setenv(\"DATEMSK\", cases[i].value, 1); else unsetenv(\"DATEMSK\");",
        "    getdate_err = 0;",
        "    struct tm *r = getdate(\"2026-09-28\");",
        "    printf(\"E %s = %d %d\\n\", cases[i].name, r != NULL, getdate_err);",
        "  }",
        "  return 0;",
        "}",
    ]
    return "\n".join(c) + "\n"


def main():
    with workdir() as tmp:
        (Path(tmp) / "getdate_oracle.c").write_text(gen_c(), encoding="utf-8", newline="\n")
        w = wsl_path(tmp)
        r = run(f"cd {w} && gcc -O0 -fno-builtin -w -o getdate_oracle getdate_oracle.c "
                f"&& ./getdate_oracle {w} > '{wsl_path(OUT)}'")
    if r.returncode != 0:
        sys.exit(f"oracle failed:\n{r.stdout[-2000:]}\n{r.stderr[-4000:]}")
    n = sum(1 for _ in OUT.open(encoding="utf-8"))
    print(f"{OUT.name}: {n} lines, {OUT.stat().st_size} bytes")


if __name__ == "__main__":
    main()
