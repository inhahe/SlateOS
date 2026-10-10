/*
 * SlateOS: <argp.h> -- GNU's argument parser, which musl has none of
 * (posix/src/argp.rs; design-decisions 1163). The structures are glibc
 * 2.39's, field for field, and the constants its values, so a program's
 * tables are what it would give glibc's; it includes what glibc's header
 * does, as programs that use it may count on.
 */

/* A system header, as the musl ones are: warnings in it are not the
 * program's. (scripts/check-libc-overlay.py defines the macro, to hold it to
 * -Wall -Wextra itself.) */
#ifndef _SLATEOS_OVERLAY_WARNINGS
#pragma GCC system_header
#endif

#ifndef _ARGP_H
#define _ARGP_H 1

#include <stdio.h>
#include <ctype.h>
#include <getopt.h>
#include <limits.h>
#include <errno.h>
#include <bits/slateos-features.h>

#ifndef __error_t_defined
#define __error_t_defined 1
typedef int error_t;
#endif

#ifdef __cplusplus
extern "C" {
#endif

/* An option: its long name, its key -- a printable character is its short
 * option too -- the name of its argument, how it is shown and parsed, its
 * help, and the group of the help it is in (0: the entry before's). A NULL
 * name and key make a header of the doc; an entry of zeros ends a table. */
struct argp_option
{
  const char *name;
  int key;
  const char *arg;
  int flags;
  const char *doc;
  int group;
};

/* The argument may be left out: `--name' alone, or `--name=ARG'. */
#define OPTION_ARG_OPTIONAL 0x1
/* Parsed, and not in the help. */
#define OPTION_HIDDEN 0x2
/* Another name for the option before: its key, argument and doc. */
#define OPTION_ALIAS 0x4
/* Not an option: a line of documentation, its name printed as it is. */
#define OPTION_DOC 0x8
/* Not in the usage line. */
#define OPTION_NO_USAGE 0x10

struct argp;
struct argp_state;
struct argp_child;

/* Called for each option, each argument and the moments around them: 0, an
 * errno value, or ARGP_ERR_UNKNOWN for a key not this parser's. */
typedef error_t (*argp_parser_t) (int __key, char *__arg,
                                  struct argp_state *__state);

#define ARGP_ERR_UNKNOWN E2BIG

/* The keys of the moments. */
#define ARGP_KEY_ARG 0
#define ARGP_KEY_ARGS 0x1000006
#define ARGP_KEY_END 0x1000001
#define ARGP_KEY_NO_ARGS 0x1000002
#define ARGP_KEY_INIT 0x1000003
#define ARGP_KEY_FINI 0x1000007
#define ARGP_KEY_SUCCESS 0x1000004
#define ARGP_KEY_ERROR 0x1000005

/* What argp_parse is given: the options, the parser, what the arguments
 * are (alternatives on lines of their own), the help's text (the part after
 * a \v follows the options), the children, a filter for the help's texts,
 * and the translation domain. */
struct argp
{
  const struct argp_option *options;
  argp_parser_t parser;
  const char *args_doc;
  const char *doc;
  const struct argp_child *children;
  char *(*help_filter) (int __key, const char *__text, void *__input);
  const char *argp_domain;
};

/* The help filter's keys. */
#define ARGP_KEY_HELP_PRE_DOC 0x2000001
#define ARGP_KEY_HELP_POST_DOC 0x2000002
#define ARGP_KEY_HELP_HEADER 0x2000003
#define ARGP_KEY_HELP_EXTRA 0x2000004
#define ARGP_KEY_HELP_DUP_ARGS_NOTE 0x2000005
#define ARGP_KEY_HELP_ARGS_DOC 0x2000006

/* A child: its argp, a header for its options, its group. */
struct argp_child
{
  const struct argp *argp;
  int flags;
  const char *header;
  int group;
};

/* The parse, as each parser sees it. */
struct argp_state
{
  const struct argp *root_argp;
  int argc;
  char **argv;
  int next;
  unsigned flags;
  unsigned arg_num;
  int quoted;
  void *input;
  void **child_inputs;
  void *hook;
  char *name;
  FILE *err_stream;
  FILE *out_stream;
  void *pstate;
};

/* argp_parse's flags. */
#define ARGP_PARSE_ARGV0 0x01
#define ARGP_NO_ERRS 0x02
#define ARGP_NO_ARGS 0x04
#define ARGP_IN_ORDER 0x08
#define ARGP_NO_HELP 0x10
#define ARGP_NO_EXIT 0x20
#define ARGP_LONG_ONLY 0x40
#define ARGP_SILENT (ARGP_NO_EXIT | ARGP_NO_ERRS | ARGP_NO_HELP)

extern error_t argp_parse (const struct argp *__restrict __argp,
                           int __argc, char **__restrict __argv,
                           unsigned __flags, int *__restrict __arg_index,
                           void *__restrict __input);

/* --version prints this, or calls this; set, or defined, by the program. */
extern const char *argp_program_version;
extern void (*argp_program_version_hook) (FILE *__restrict __stream,
                                          struct argp_state *__restrict
                                          __state);
/* The help ends by giving it. */
extern const char *argp_program_bug_address;
/* What a usage error exits with: EX_USAGE, 64. */
extern error_t argp_err_exit_status;

/* argp_help's flags. */
#define ARGP_HELP_USAGE 0x01
#define ARGP_HELP_SHORT_USAGE 0x02
#define ARGP_HELP_SEE 0x04
#define ARGP_HELP_LONG 0x08
#define ARGP_HELP_PRE_DOC 0x10
#define ARGP_HELP_POST_DOC 0x20
#define ARGP_HELP_DOC (ARGP_HELP_PRE_DOC | ARGP_HELP_POST_DOC)
#define ARGP_HELP_BUG_ADDR 0x40
#define ARGP_HELP_LONG_ONLY 0x80
#define ARGP_HELP_EXIT_ERR 0x100
#define ARGP_HELP_EXIT_OK 0x200
#define ARGP_HELP_STD_ERR (ARGP_HELP_SEE | ARGP_HELP_EXIT_ERR)
#define ARGP_HELP_STD_USAGE \
  (ARGP_HELP_SHORT_USAGE | ARGP_HELP_SEE | ARGP_HELP_EXIT_ERR)
#define ARGP_HELP_STD_HELP \
  (ARGP_HELP_SHORT_USAGE | ARGP_HELP_LONG | ARGP_HELP_EXIT_OK \
   | ARGP_HELP_DOC | ARGP_HELP_BUG_ADDR)

extern void argp_help (const struct argp *__restrict __argp,
                       FILE *__restrict __stream, unsigned __flags,
                       char *__restrict __name);
extern void argp_state_help (const struct argp_state *__restrict __state,
                             FILE *__restrict __stream, unsigned int __flags);
extern void argp_usage (const struct argp_state *__state);
extern void argp_error (const struct argp_state *__restrict __state,
                        const char *__restrict __fmt, ...)
     _SLATEOS_PRINTF (2, 3);
extern void argp_failure (const struct argp_state *__restrict __state,
                          int __status, int __errnum,
                          const char *__restrict __fmt, ...)
     _SLATEOS_PRINTF (4, 5);

extern int _option_is_short (const struct argp_option *__opt);
extern int _option_is_end (const struct argp_option *__opt);
extern void *_argp_input (const struct argp *__restrict __argp,
                          const struct argp_state *__restrict __state);

#ifdef __cplusplus
}
#endif

#endif /* _ARGP_H */
