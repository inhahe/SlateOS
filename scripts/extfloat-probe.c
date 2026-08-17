/* The glibc side of the `extfloat` differential test.
 *
 * `scripts/extfloat-diff.sh` compiles this inside WSL and runs it against
 * `userspace/coreutils/examples/extfloat-probe.rs` over the same cases. The two
 * must agree byte for byte: that is the whole claim `extfloat` makes.
 *
 * Both modes are described in the Rust file. Keep the two in step -- an output
 * line here that the Rust side cannot also produce is a harness bug that reads
 * as a real difference.
 *
 * Build: gcc -O2 -o extfloat-probe extfloat-probe.c
 *
 * There is deliberately no -lm and no <math.h>: every conversion used here is
 * in libc proper, and pulling in the math library would risk measuring its
 * rounding rather than printf's. */

#define _GNU_SOURCE
#include <errno.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* gnulib's `xstrtold (s, NULL, ...)`, which is how `seq` reads an operand: the
 * whole string must be one numeral, and a range error is fatal only when the
 * value came out nonzero -- an underflow all the way to zero is a normal
 * answer, one that stops at a subnormal is not. */
static bool
whole_string (char const *s, long double *out)
{
  char *end;
  errno = 0;
  long double v = strtold (s, &end);
  if (end == s || *end != '\0')
    return false;
  if (errno == ERANGE && v != 0)
    return false;
  *out = v;
  return true;
}

static void
read_case (char const *line)
{
  char *end;
  errno = 0;
  long double v = strtold (line, &end);
  printf ("consumed=%td range=%d value=%La\n", end - line,
          errno == ERANGE ? 1 : 0, v);
}

/* Copy FMT, inserting an `L` before the conversion character if it has none.
 * This is what `seq`'s `long_double_format` does, and doing it here lets the
 * generator emit both spellings and prove they are the same directive. */
static bool
with_length_modifier (char const *fmt, char *out, size_t cap)
{
  size_t i = 0;
  if (fmt[i] != '%')
    return false;
  i++;
  i += strspn (fmt + i, "-+#0 '");
  i += strspn (fmt + i, "0123456789");
  if (fmt[i] == '.')
    {
      i++;
      i += strspn (fmt + i, "0123456789");
    }
  size_t at = i;
  bool has_L = fmt[i] == 'L';
  i += has_L;
  if (!fmt[i] || !strchr ("efgaEFGA", fmt[i]) || fmt[i + 1] != '\0')
    return false;
  if (strlen (fmt) + 2 > cap)
    return false;
  memcpy (out, fmt, at);
  out[at] = 'L';
  strcpy (out + at + 1, fmt + at + has_L);
  return true;
}

static void
write_case (char *line)
{
  char *tab = strchr (line, '\t');
  if (!tab)
    {
      puts ("!no-tab");
      return;
    }
  *tab = '\0';
  char fmt[256];
  if (!with_length_modifier (line, fmt, sizeof fmt))
    {
      puts ("!bad-format");
      return;
    }
  long double v;
  if (!whole_string (tab + 1, &v))
    {
      puts ("!bad-literal");
      return;
    }
  printf (fmt, v);
  putchar ('\n');
}

int
main (int argc, char **argv)
{
  if (argc != 2)
    {
      fprintf (stderr, "extfloat-probe: expected 'read' or 'write'\n");
      return 2;
    }
  bool reading = strcmp (argv[1], "read") == 0;
  if (!reading && strcmp (argv[1], "write") != 0)
    {
      fprintf (stderr, "extfloat-probe: expected 'read' or 'write'\n");
      return 2;
    }

  char *line = NULL;
  size_t cap = 0;
  ssize_t len;
  while ((len = getline (&line, &cap, stdin)) > 0)
    {
      while (len > 0 && (line[len - 1] == '\n' || line[len - 1] == '\r'))
        line[--len] = '\0';
      /* An empty line is a case -- `strtold ("")` consumes nothing -- and not
       * an absence of one. Skipping it here would shift every later answer up
       * by one line against the case file and report the file as all
       * different, which is exactly what the first run of this harness did. */
      if (reading)
        read_case (line);
      else
        write_case (line);
    }
  free (line);
  return ferror (stdout) ? 1 : 0;
}
