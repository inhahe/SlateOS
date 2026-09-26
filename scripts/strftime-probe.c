/* strftime-probe.c -- the C library's strftime, for strftime-diff.sh.
 *
 * Reads cases on stdin, one per line: SECONDS <TAB> FORMAT. For each, prints
 * one line: strftime (FORMAT) of localtime (SECONDS), with TZ taken from the
 * environment, escaped by `put` below so that a `%n` or a `%t` in the output
 * cannot split or merge lines. `userspace/coreutils/examples/strftime-probe.rs`
 * prints ours the same way, byte for byte.
 *
 * `localtime` failing -- a year past `int` -- prints `ERR`; `strftime`
 * returning 0 for output that did not fit prints `FULL`. (It also returns 0
 * for an empty result, which prints as an empty pair of brackets: the buffer
 * is cleared first, so the two are told apart by whether anything was
 * written.) */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

static void
put (const char *s, size_t n)
{
  putchar ('[');
  for (size_t i = 0; i < n; i++)
    {
      unsigned char c = s[i];
      if (c == '\\')
        fputs ("\\\\", stdout);
      else if (c < 0x20 || c == 0x7f)
        printf ("\\x%02x", c);
      else
        putchar (c);
    }
  puts ("]");
}

int
main (void)
{
  static char line[16384];
  static char out[65536];
  while (fgets (line, sizeof line, stdin))
    {
      size_t n = strlen (line);
      if (n && line[n - 1] == '\n')
        line[--n] = '\0';
      char *tab = strchr (line, '\t');
      if (!tab)
        {
          puts ("BAD");
          continue;
        }
      *tab = '\0';
      time_t t = (time_t) strtoll (line, NULL, 10);
      struct tm tm;
      if (!localtime_r (&t, &tm))
        {
          puts ("ERR");
          continue;
        }
      memset (out, 0, sizeof out);
      size_t len = strftime (out, sizeof out, tab + 1, &tm);
      if (len == 0 && out[0] != '\0')
        puts ("FULL");
      else
        put (out, len);
    }
  return 0;
}
