/* curses-probe.c -- the reference side of scripts/curses-diff.sh: Ubuntu's
 * libncursesw driven by a script of curses calls, one per line, which the
 * crate's own probe (userspace/curses/examples/curses-probe.rs) reads the
 * same way.
 *
 * What the library writes to the terminal goes to standard output; what each
 * call returns goes to standard error, one line per call, as "NAME VALUE".
 *
 * A line is a call's name and its arguments, separated by single spaces. The
 * last argument of a call that takes text is the rest of the line, with the
 * escapes \n \t \e \\ and \xHH. Numbers are decimal, attributes hexadecimal.
 * Text for the wide calls is UTF-8, decoded here rather than by the locale,
 * so that both sides see the same characters whatever the locale. Blank
 * lines and lines starting with '#' are skipped.
 *
 * Besides the curses calls: "winch ROWS COLS" sets the size of the terminal
 * on standard output and raises SIGWINCH; "suspend", "interrupt" and
 * "terminate" raise SIGTSTP, SIGINT and SIGTERM; "tty" prints the modes of
 * the terminal on standard input -- c_iflag, c_oflag, c_cflag and c_lflag in
 * hexadecimal, then VMIN and VTIME -- or "tty -1" when it is no terminal;
 * and "tab3" sets that terminal to expand tabs itself, as `stty tab3`.
 * "tigetstr NAME" prints "tigetstr NAME" and the string's bytes in
 * hexadecimal, or "absent" or "cancelled" for what ncurses answers with a
 * null pointer and with (char *) -1.
 *
 * The calls on a window name it by number, first: 0 is stdscr, and each
 * window "subwin" or "derwin" makes gets the next, which the call prints
 * (-1 when it made none). "acs C" prints what acs_map holds for the letter
 * C; "wgetyx W" a window's cursor; "wgetmaxyx W" its size and where it
 * begins. The keyboard calls read standard input; "get_wch" prints what it
 * returned and the character or key, in hexadecimal.
 */
#define _XOPEN_SOURCE_EXTENDED 1
#include <curses.h>
#include <locale.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <termios.h>
#include <wchar.h>

static size_t unescape (const char *s, char *out)
{
  size_t n = 0;
  while (*s)
    {
      if (*s != '\\' || !s[1])
        {
          out[n++] = *s++;
          continue;
        }
      s++;
      switch (*s)
        {
        case 'n': out[n++] = '\n'; s++; break;
        case 't': out[n++] = '\t'; s++; break;
        case 'e': out[n++] = '\033'; s++; break;
        case '\\': out[n++] = '\\'; s++; break;
        case 'x':
          {
            char hex[3] = { 0, 0, 0 };
            if (s[1] && s[2])
              {
                hex[0] = s[1];
                hex[1] = s[2];
                out[n++] = (char) strtoul (hex, NULL, 16);
                s += 3;
              }
            else
              out[n++] = *s++;
            break;
          }
        default: out[n++] = '\\'; break;
        }
    }
  out[n] = 0;
  return n;
}

/* UTF-8 to wide characters, by hand: a lead byte and as many continuation
 * bytes as it promises make one character; any other byte stands for
 * itself. The crate's probe decodes the same way. */
static size_t utf8_to_wide (const char *s, size_t len, wchar_t *out)
{
  size_t i = 0, n = 0;
  while (i < len)
    {
      unsigned char c = (unsigned char) s[i];
      size_t need, k;
      unsigned long wc;
      if (c < 0x80)
        need = 0, wc = c;
      else if ((c & 0xe0) == 0xc0)
        need = 1, wc = c & 0x1f;
      else if ((c & 0xf0) == 0xe0)
        need = 2, wc = c & 0x0f;
      else if ((c & 0xf8) == 0xf0)
        need = 3, wc = c & 0x07;
      else
        need = 4, wc = c;
      for (k = 1; need < 4 && k <= need; k++)
        {
          unsigned char d;
          if (i + k >= len)
            break;
          d = (unsigned char) s[i + k];
          if ((d & 0xc0) != 0x80)
            break;
          wc = (wc << 6) | (d & 0x3f);
        }
      if (need == 4 || k <= need)
        {
          out[n++] = c;
          i++;
          continue;
        }
      out[n++] = (wchar_t) wc;
      i += need + 1;
    }
  out[n] = 0;
  return n;
}

static void say (const char *name, int value)
{
  fprintf (stderr, "%s %d\n", name, value);
}

/* Windows by number: 0 is stdscr, and each subwin or derwin the script
 * makes gets the next. A window deleted names none any more. */
static WINDOW *wins[64];
static int nwins = 1;

static WINDOW *win_of (int n)
{
  if (n == 0)
    return stdscr;
  return n > 0 && n < nwins ? wins[n] : NULL;
}

/* The next space-separated word of *p, advanced past it. */
static char *word (char **p)
{
  char *w = *p, *sp = strchr (w, ' ');
  if (sp)
    {
      *sp = 0;
      *p = sp + 1;
    }
  else
    *p = w + strlen (w);
  return w;
}

int main (int argc, char **argv)
{
  static char line[65536], text[65536];
  static wchar_t wide[65536];
  FILE *in;
  if (argc != 2 || !(in = fopen (argv[1], "r")))
    {
      fprintf (stderr, "usage: curses-probe SCRIPT\n");
      return 2;
    }
  setlocale (LC_ALL, "");
  while (fgets (line, sizeof line, in))
    {
      char *p = line, *cmd;
      size_t len = strlen (line);
      if (len && line[len - 1] == '\n')
        line[--len] = 0;
      if (!len || line[0] == '#')
        continue;
      cmd = word (&p);
#define N() atoi (word (&p))
#define H() ((chtype) strtoul (word (&p), NULL, 16))
#define TEXT() unescape (p, text)
#define W() win_of (atoi (word (&p)))
      if (!strcmp (cmd, "use_env"))
        { use_env (N () != 0); say (cmd, 0); }
      else if (!strcmp (cmd, "initscr"))
        { initscr (); say (cmd, 0); }
      else if (!strcmp (cmd, "endwin"))
        say (cmd, endwin ());
      else if (!strcmp (cmd, "isendwin"))
        say (cmd, isendwin ());
      else if (!strcmp (cmd, "refresh"))
        say (cmd, refresh ());
      else if (!strcmp (cmd, "wnoutrefresh"))
        say (cmd, wnoutrefresh (W ()));
      else if (!strcmp (cmd, "doupdate"))
        say (cmd, doupdate ());
      else if (!strcmp (cmd, "move"))
        { int y = N (), x = N (); say (cmd, move (y, x)); }
      else if (!strcmp (cmd, "addch"))
        say (cmd, addch (H ()));
      else if (!strcmp (cmd, "mvaddch"))
        { int y = N (), x = N (); say (cmd, mvaddch (y, x, H ())); }
      else if (!strcmp (cmd, "addstr"))
        { TEXT (); say (cmd, addstr (text)); }
      else if (!strcmp (cmd, "addnstr"))
        { int n = N (); TEXT (); say (cmd, addnstr (text, n)); }
      else if (!strcmp (cmd, "mvaddstr"))
        { int y = N (), x = N (); TEXT (); say (cmd, mvaddstr (y, x, text)); }
      else if (!strcmp (cmd, "mvaddnstr"))
        {
          int y = N (), x = N (), n = N ();
          TEXT ();
          say (cmd, mvaddnstr (y, x, text, n));
        }
      else if (!strcmp (cmd, "printw"))
        { TEXT (); say (cmd, printw ("%s", text)); }
      else if (!strcmp (cmd, "addwstr"))
        {
          size_t n = TEXT ();
          utf8_to_wide (text, n, wide);
          say (cmd, addwstr (wide));
        }
      else if (!strcmp (cmd, "addnwstr"))
        {
          int k = N ();
          size_t n = TEXT ();
          utf8_to_wide (text, n, wide);
          say (cmd, addnwstr (wide, k));
        }
      else if (!strcmp (cmd, "mvaddnwstr"))
        {
          int y = N (), x = N (), k = N ();
          size_t n = TEXT ();
          utf8_to_wide (text, n, wide);
          say (cmd, mvaddnwstr (y, x, wide, k));
        }
      else if (!strcmp (cmd, "clear"))
        say (cmd, clear ());
      else if (!strcmp (cmd, "erase"))
        say (cmd, erase ());
      else if (!strcmp (cmd, "clrtobot"))
        say (cmd, clrtobot ());
      else if (!strcmp (cmd, "clrtoeol"))
        say (cmd, clrtoeol ());
      else if (!strcmp (cmd, "attron"))
        say (cmd, attron ((int) H ()));
      else if (!strcmp (cmd, "attroff"))
        say (cmd, attroff ((int) H ()));
      else if (!strcmp (cmd, "attrset"))
        say (cmd, attrset ((int) H ()));
      else if (!strcmp (cmd, "standout"))
        say (cmd, standout ());
      else if (!strcmp (cmd, "standend"))
        say (cmd, standend ());
      else if (!strcmp (cmd, "inch"))
        fprintf (stderr, "inch %lx\n", (unsigned long) inch ());
      else if (!strcmp (cmd, "in_wch"))
        {
          cchar_t c;
          int rc = in_wch (&c);
          fprintf (stderr, "in_wch %d %lx %x %x %d\n", rc,
                   (unsigned long) c.attr, (unsigned) c.chars[0],
                   (unsigned) c.chars[1], c.ext_color);
        }
      else if (!strcmp (cmd, "getyx"))
        {
          int y, x;
          getyx (stdscr, y, x);
          fprintf (stderr, "getyx %d %d\n", y, x);
        }
      else if (!strcmp (cmd, "getmaxyx"))
        {
          int y, x;
          getmaxyx (stdscr, y, x);
          fprintf (stderr, "getmaxyx %d %d\n", y, x);
        }
      else if (!strcmp (cmd, "lines"))
        fprintf (stderr, "lines %d %d %d\n", LINES, COLS, TABSIZE);
      else if (!strcmp (cmd, "curs_set"))
        say (cmd, curs_set (N ()));
      else if (!strcmp (cmd, "nl"))
        say (cmd, nl ());
      else if (!strcmp (cmd, "nonl"))
        say (cmd, nonl ());
      else if (!strcmp (cmd, "echo"))
        say (cmd, echo ());
      else if (!strcmp (cmd, "noecho"))
        say (cmd, noecho ());
      else if (!strcmp (cmd, "cbreak"))
        say (cmd, cbreak ());
      else if (!strcmp (cmd, "nocbreak"))
        say (cmd, nocbreak ());
      else if (!strcmp (cmd, "def_shell_mode"))
        say (cmd, def_shell_mode ());
      else if (!strcmp (cmd, "def_prog_mode"))
        say (cmd, def_prog_mode ());
      else if (!strcmp (cmd, "reset_shell_mode"))
        say (cmd, reset_shell_mode ());
      else if (!strcmp (cmd, "reset_prog_mode"))
        say (cmd, reset_prog_mode ());
      else if (!strcmp (cmd, "idlok"))
        say (cmd, idlok (stdscr, N () != 0));
      else if (!strcmp (cmd, "idcok"))
        { idcok (stdscr, N () != 0); say (cmd, 0); }
      else if (!strcmp (cmd, "use_legacy_coding"))
        say (cmd, use_legacy_coding (N ()));
      else if (!strcmp (cmd, "tigetstr"))
        {
          char *name = word (&p), *s = tigetstr (name);
          if (s == (char *) -1)
            fprintf (stderr, "tigetstr %s cancelled\n", name);
          else if (!s)
            fprintf (stderr, "tigetstr %s absent\n", name);
          else
            {
              fprintf (stderr, "tigetstr %s", name);
              for (; *s; s++)
                fprintf (stderr, " %02x", (unsigned char) *s);
              fprintf (stderr, "\n");
            }
        }
      else if (!strcmp (cmd, "tty"))
        {
          struct termios t;
          if (tcgetattr (0, &t))
            fprintf (stderr, "tty -1\n");
          else
            fprintf (stderr, "tty %lx %lx %lx %lx %d %d\n",
                     (unsigned long) t.c_iflag, (unsigned long) t.c_oflag,
                     (unsigned long) t.c_cflag, (unsigned long) t.c_lflag,
                     t.c_cc[VMIN], t.c_cc[VTIME]);
        }
      else if (!strcmp (cmd, "tab3"))
        {
          struct termios t;
          int rc = tcgetattr (0, &t);
          if (!rc)
            {
              t.c_oflag = (t.c_oflag & ~TABDLY) | TAB3;
              rc = tcsetattr (0, TCSANOW, &t);
            }
          say (cmd, rc ? -1 : 0);
        }
      else if (!strcmp (cmd, "beep"))
        say (cmd, beep ());
      else if (!strcmp (cmd, "has_colors"))
        say (cmd, has_colors ());
      else if (!strcmp (cmd, "can_change_color"))
        say (cmd, can_change_color ());
      else if (!strcmp (cmd, "start_color"))
        say (cmd, start_color ());
      else if (!strcmp (cmd, "use_default_colors"))
        say (cmd, use_default_colors ());
      else if (!strcmp (cmd, "assume_default_colors"))
        { int f = N (), b = N (); say (cmd, assume_default_colors (f, b)); }
      else if (!strcmp (cmd, "init_pair"))
        {
          int pr = N (), f = N (), b = N ();
          say (cmd, init_pair ((short) pr, (short) f, (short) b));
        }
      else if (!strcmp (cmd, "init_color"))
        {
          int c = N (), r = N (), g = N (), b = N ();
          say (cmd, init_color ((short) c, (short) r, (short) g, (short) b));
        }
      else if (!strcmp (cmd, "colors"))
        fprintf (stderr, "colors %d %d\n", COLORS, COLOR_PAIRS);
      else if (!strcmp (cmd, "resizeterm"))
        { int l = N (), c = N (); say (cmd, resizeterm (l, c)); }
      else if (!strcmp (cmd, "resize_term"))
        { int l = N (), c = N (); say (cmd, resize_term (l, c)); }
      else if (!strcmp (cmd, "is_term_resized"))
        { int l = N (), c = N (); say (cmd, is_term_resized (l, c)); }
      /* The window changed size: the terminal on standard output told so,
       * and the signal raised (the kernel sends one too, on a terminal). */
      else if (!strcmp (cmd, "winch"))
        {
          struct winsize ws;
          memset (&ws, 0, sizeof ws);
          ws.ws_row = (unsigned short) N ();
          ws.ws_col = (unsigned short) N ();
          ioctl (1, TIOCSWINSZ, &ws);
          raise (SIGWINCH);
          say (cmd, 0);
        }
      /* Windows: "subwin W L C Y X" and "derwin W L C Y X" print the new
       * window's number, or -1. */
      else if (!strcmp (cmd, "subwin") || !strcmp (cmd, "derwin"))
        {
          WINDOW *o = W (), *w;
          int l = N (), c = N (), y = N (), x = N ();
          w = cmd[0] == 's' ? subwin (o, l, c, y, x) : derwin (o, l, c, y, x);
          if (w && nwins < 64)
            {
              wins[nwins] = w;
              say (cmd, nwins++);
            }
          else
            say (cmd, -1);
        }
      else if (!strcmp (cmd, "delwin"))
        {
          int n = N ();
          int rc = delwin (win_of (n));
          if (rc == OK && n > 0 && n < nwins)
            wins[n] = NULL;
          say (cmd, rc);
        }
      else if (!strcmp (cmd, "touchwin"))
        say (cmd, touchwin (W ()));
      else if (!strcmp (cmd, "wrefresh"))
        say (cmd, wrefresh (W ()));
      else if (!strcmp (cmd, "wclear"))
        say (cmd, wclear (W ()));
      else if (!strcmp (cmd, "werase"))
        say (cmd, werase (W ()));
      else if (!strcmp (cmd, "wmove"))
        { WINDOW *w = W (); int y = N (), x = N (); say (cmd, wmove (w, y, x)); }
      else if (!strcmp (cmd, "waddch"))
        { WINDOW *w = W (); say (cmd, waddch (w, H ())); }
      else if (!strcmp (cmd, "waddstr"))
        { WINDOW *w = W (); TEXT (); say (cmd, waddstr (w, text)); }
      else if (!strcmp (cmd, "mvwaddstr"))
        {
          WINDOW *w = W ();
          int y = N (), x = N ();
          TEXT ();
          say (cmd, mvwaddstr (w, y, x, text));
        }
      else if (!strcmp (cmd, "wclrtoeol"))
        say (cmd, wclrtoeol (W ()));
      else if (!strcmp (cmd, "wattron"))
        { WINDOW *w = W (); say (cmd, wattron (w, (int) H ())); }
      else if (!strcmp (cmd, "box"))
        { WINDOW *w = W (); chtype v = H (), h = H (); say (cmd, box (w, v, h)); }
      else if (!strcmp (cmd, "wborder"))
        {
          WINDOW *w = W ();
          chtype e[8];
          int i;
          for (i = 0; i < 8; i++)
            e[i] = H ();
          say (cmd, wborder (w, e[0], e[1], e[2], e[3], e[4], e[5], e[6], e[7]));
        }
      /* "acs C": what acs_map holds for the letter C. */
      else if (!strcmp (cmd, "acs"))
        {
          char *c = word (&p);
          fprintf (stderr, "acs %s %lx\n", c,
                   (unsigned long) NCURSES_ACS ((unsigned char) c[0]));
        }
      else if (!strcmp (cmd, "wgetyx"))
        {
          WINDOW *w = W ();
          int y = -1, x = -1;
          if (w)
            getyx (w, y, x);
          fprintf (stderr, "wgetyx %d %d\n", y, x);
        }
      else if (!strcmp (cmd, "wgetmaxyx"))
        {
          WINDOW *w = W ();
          fprintf (stderr, "wgetmaxyx %d %d %d %d\n", getmaxy (w), getmaxx (w),
                   getbegy (w), getbegx (w));
        }
      /* The keyboard. */
      else if (!strcmp (cmd, "getch"))
        say (cmd, getch ());
      else if (!strcmp (cmd, "wgetch"))
        say (cmd, wgetch (W ()));
      else if (!strcmp (cmd, "get_wch"))
        {
          wint_t c = 0;
          int rc = get_wch (&c);
          fprintf (stderr, "get_wch %d %lx\n", rc, (unsigned long) c);
        }
      else if (!strcmp (cmd, "ungetch"))
        say (cmd, ungetch (N ()));
      else if (!strcmp (cmd, "keypad"))
        { WINDOW *w = W (); say (cmd, keypad (w, N () != 0)); }
      else if (!strcmp (cmd, "nodelay"))
        { WINDOW *w = W (); say (cmd, nodelay (w, N () != 0)); }
      else if (!strcmp (cmd, "notimeout"))
        { WINDOW *w = W (); say (cmd, notimeout (w, N () != 0)); }
      else if (!strcmp (cmd, "wtimeout"))
        { WINDOW *w = W (); wtimeout (w, N ()); say (cmd, 0); }
      else if (!strcmp (cmd, "halfdelay"))
        say (cmd, halfdelay (N ()));
      else if (!strcmp (cmd, "meta"))
        say (cmd, meta (stdscr, N () != 0));
      else if (!strcmp (cmd, "set_escdelay"))
        say (cmd, set_escdelay (N ()));
      else if (!strcmp (cmd, "flushinp"))
        say (cmd, flushinp ());
      else if (!strcmp (cmd, "suspend"))
        { raise (SIGTSTP); say (cmd, 0); }
      else if (!strcmp (cmd, "interrupt"))
        { raise (SIGINT); say (cmd, 0); }
      else if (!strcmp (cmd, "terminate"))
        { raise (SIGTERM); say (cmd, 0); }
      else
        {
          fprintf (stderr, "curses-probe: unknown call '%s'\n", cmd);
          return 2;
        }
      fflush (stderr);
    }
  return 0;
}
