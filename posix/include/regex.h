/*
 * SlateOS: <regex.h> -- in place of musl's, whose regex_t is not glibc's.
 *
 * glibc's regex_t is struct re_pattern_buffer, and its fields are part of
 * the interface: a program fills buffer, allocated, fastmap and translate
 * before re_compile_pattern, and reads re_nsub, sets not_bol, not_eol and
 * newline_anchor, after. musl's regex_t is a struct of the same name and
 * size, 64 bytes, with re_nsub first and the rest opaque -- and it is
 * declared once, with musl's regcomp and regexec taking it, so this header
 * cannot be musl's with more after it. It is glibc 2.39's, in its own
 * _REGEX_LARGE_OFFSETS form: regoff_t as wide as ssize_t, as POSIX
 * requires and musl's is (glibc's own is an int), and so the counts of
 * struct re_registers size_t. Everything else is glibc's: its syntax bits
 * and its GNU calls for _GNU_SOURCE, its flags and error codes, and
 * re_comp and re_exec for _REGEX_RE_COMP. musl's REG_OK is kept.
 */

/* A system header, as the musl ones it extends are: warnings in it are not
 * the program's. (scripts/check-libc-overlay.py defines the macro, to hold
 * it to -Wall -Wextra itself.) */
#ifndef _SLATEOS_OVERLAY_WARNINGS
#pragma GCC system_header
#endif

#ifndef _REGEX_H
#define _REGEX_H 1

#include <features.h>

#define __NEED_regoff_t
#define __NEED_size_t

#include <bits/alltypes.h>

#ifdef __cplusplus
extern "C" {
#endif

/* glibc's _REGEX_LARGE_OFFSETS types: a count of registers, and of the
 * bytes of a compiled pattern. */
typedef size_t __re_size_t;
typedef size_t __re_long_size_t;

/* glibc's, for programs that name them. */
typedef long int s_reg_t;
typedef unsigned long int active_reg_t;

/* The syntax a pattern is read in: the RE_* bits below. */
typedef unsigned long int reg_syntax_t;

#ifdef _GNU_SOURCE
/* \ quotes the next character inside a bracket expression. */
# define RE_BACKSLASH_ESCAPE_IN_LISTS ((unsigned long int) 1)
/* \+ and \? are the operators, + and ? literals; clear, the other way. */
# define RE_BK_PLUS_QM (RE_BACKSLASH_ESCAPE_IN_LISTS << 1)
/* [:alpha:] and the other classes are recognised in brackets. */
# define RE_CHAR_CLASSES (RE_BK_PLUS_QM << 1)
/* ^ and $ are anchors wherever they are; clear, only where POSIX's BREs
 * have them. */
# define RE_CONTEXT_INDEP_ANCHORS (RE_CHAR_CLASSES << 1)
/* * + ? and intervals are operators even where nothing precedes them. */
# define RE_CONTEXT_INDEP_OPS (RE_CONTEXT_INDEP_ANCHORS << 1)
/* ... and are an error there. */
# define RE_CONTEXT_INVALID_OPS (RE_CONTEXT_INDEP_OPS << 1)
/* . matches a newline. */
# define RE_DOT_NEWLINE (RE_CONTEXT_INVALID_OPS << 1)
/* . does not match a NUL. */
# define RE_DOT_NOT_NULL (RE_DOT_NEWLINE << 1)
/* [^...] does not match a newline. */
# define RE_HAT_LISTS_NOT_NEWLINE (RE_DOT_NOT_NULL << 1)
/* {} (or \{\}, by RE_NO_BK_BRACES) is an interval; clear, literal. */
# define RE_INTERVALS (RE_HAT_LISTS_NOT_NEWLINE << 1)
/* + ? and | are not operators. */
# define RE_LIMITED_OPS (RE_INTERVALS << 1)
/* A newline separates alternatives. */
# define RE_NEWLINE_ALT (RE_LIMITED_OPS << 1)
/* {} rather than \{\} make an interval. */
# define RE_NO_BK_BRACES (RE_NEWLINE_ALT << 1)
/* () rather than \(\) make a group. */
# define RE_NO_BK_PARENS (RE_NO_BK_BRACES << 1)
/* \1 ... \9 are literal digits, not back-references. */
# define RE_NO_BK_REFS (RE_NO_BK_PARENS << 1)
/* | rather than \| separates alternatives. */
# define RE_NO_BK_VBAR (RE_NO_BK_REFS << 1)
/* A range whose end comes before its start, [z-a], is an error; clear, it
 * matches nothing. */
# define RE_NO_EMPTY_RANGES (RE_NO_BK_VBAR << 1)
/* A ) with no ( is an ordinary character. */
# define RE_UNMATCHED_RIGHT_PAREN_ORD (RE_NO_EMPTY_RANGES << 1)
/* Succeed at the first match of the whole pattern: accepted, and ignored,
 * as glibc ignores it. */
# define RE_NO_POSIX_BACKTRACKING (RE_UNMATCHED_RIGHT_PAREN_ORD << 1)
/* \w \W \s \S \b \B \< \> \` \' are ordinary characters. */
# define RE_NO_GNU_OPS (RE_NO_POSIX_BACKTRACKING << 1)
/* glibc's debugging switch: no effect. */
# define RE_DEBUG (RE_NO_GNU_OPS << 1)
/* A malformed interval is literal text: a{1 is a\{1. */
# define RE_INVALID_INTERVAL_ORD (RE_DEBUG << 1)
/* Case is ignored. */
# define RE_ICASE (RE_INVALID_INTERVAL_ORD << 1)
/* ^ is an anchor after \( and \|, where a BRE's would not be. */
# define RE_CARET_ANCHORS_HERE (RE_ICASE << 1)
/* An interval is an error after nothing, an alternation, a ( or a }. */
# define RE_CONTEXT_INVALID_DUP (RE_CARET_ANCHORS_HERE << 1)
/* re_search and re_match do not report subexpressions. */
# define RE_NO_SUB (RE_CONTEXT_INVALID_DUP << 1)
#endif

/* The syntax re_compile_pattern and re_comp read a pattern in. */
extern reg_syntax_t re_syntax_options;

#ifdef _GNU_SOURCE
/* The syntaxes of the programs, as glibc defines them. */
# define RE_SYNTAX_EMACS 0

# define RE_SYNTAX_AWK							\
  (RE_BACKSLASH_ESCAPE_IN_LISTS   | RE_DOT_NOT_NULL			\
   | RE_NO_BK_PARENS              | RE_NO_BK_REFS			\
   | RE_NO_BK_VBAR                | RE_NO_EMPTY_RANGES			\
   | RE_DOT_NEWLINE		  | RE_CONTEXT_INDEP_ANCHORS		\
   | RE_CHAR_CLASSES							\
   | RE_UNMATCHED_RIGHT_PAREN_ORD | RE_NO_GNU_OPS)

# define RE_SYNTAX_GNU_AWK						\
  ((RE_SYNTAX_POSIX_EXTENDED | RE_BACKSLASH_ESCAPE_IN_LISTS		\
    | RE_INVALID_INTERVAL_ORD)						\
   & ~(RE_DOT_NOT_NULL | RE_CONTEXT_INDEP_OPS				\
      | RE_CONTEXT_INVALID_OPS ))

# define RE_SYNTAX_POSIX_AWK						\
  (RE_SYNTAX_POSIX_EXTENDED | RE_BACKSLASH_ESCAPE_IN_LISTS		\
   | RE_INTERVALS	    | RE_NO_GNU_OPS				\
   | RE_INVALID_INTERVAL_ORD)

# define RE_SYNTAX_GREP							\
  ((RE_SYNTAX_POSIX_BASIC | RE_NEWLINE_ALT)				\
   & ~(RE_CONTEXT_INVALID_DUP | RE_DOT_NOT_NULL))

# define RE_SYNTAX_EGREP						\
  ((RE_SYNTAX_POSIX_EXTENDED | RE_INVALID_INTERVAL_ORD | RE_NEWLINE_ALT) \
   & ~(RE_CONTEXT_INVALID_OPS | RE_DOT_NOT_NULL))

/* POSIX grep -E behavior is no longer incompatible with GNU.  */
# define RE_SYNTAX_POSIX_EGREP						\
  RE_SYNTAX_EGREP

/* P1003.2/D11.2, section 4.20.7.1, lines 5078ff.  */
# define RE_SYNTAX_ED RE_SYNTAX_POSIX_BASIC

# define RE_SYNTAX_SED RE_SYNTAX_POSIX_BASIC

/* Syntax bits common to both basic and extended POSIX regex syntax.  */
# define _RE_SYNTAX_POSIX_COMMON					\
  (RE_CHAR_CLASSES | RE_DOT_NEWLINE      | RE_DOT_NOT_NULL		\
   | RE_INTERVALS  | RE_NO_EMPTY_RANGES)

# define RE_SYNTAX_POSIX_BASIC						\
  (_RE_SYNTAX_POSIX_COMMON | RE_BK_PLUS_QM | RE_CONTEXT_INVALID_DUP)

/* Differs from ..._POSIX_BASIC only in that RE_BK_PLUS_QM becomes
   RE_LIMITED_OPS, i.e., \? \+ \| are not recognized.  */
# define RE_SYNTAX_POSIX_MINIMAL_BASIC					\
  (_RE_SYNTAX_POSIX_COMMON | RE_LIMITED_OPS)

# define RE_SYNTAX_POSIX_EXTENDED					\
  (_RE_SYNTAX_POSIX_COMMON  | RE_CONTEXT_INDEP_ANCHORS			\
   | RE_CONTEXT_INDEP_OPS   | RE_NO_BK_BRACES				\
   | RE_NO_BK_PARENS        | RE_NO_BK_VBAR				\
   | RE_CONTEXT_INVALID_OPS | RE_UNMATCHED_RIGHT_PAREN_ORD)

/* Differs from ..._POSIX_EXTENDED in that RE_CONTEXT_INDEP_OPS is
   removed and RE_NO_BK_REFS is added.  */
# define RE_SYNTAX_POSIX_MINIMAL_EXTENDED				\
  (_RE_SYNTAX_POSIX_COMMON  | RE_CONTEXT_INDEP_ANCHORS			\
   | RE_CONTEXT_INVALID_OPS | RE_NO_BK_BRACES				\
   | RE_NO_BK_PARENS        | RE_NO_BK_REFS				\
   | RE_NO_BK_VBAR	    | RE_UNMATCHED_RIGHT_PAREN_ORD)

/* The largest count an interval may give (<limits.h>'s is musl's 255, the
 * least POSIX allows; this is what regcomp takes, as glibc's header has
 * it). */
# ifdef RE_DUP_MAX
#  undef RE_DUP_MAX
# endif
# define RE_DUP_MAX (0x7fff)
#endif

/* regcomp's flags. */
#define REG_EXTENDED 1
#define REG_ICASE (1 << 1)
#define REG_NEWLINE (1 << 2)
#define REG_NOSUB (1 << 3)

/* regexec's: ^ does not match at the string's start; $ not at its end;
 * pmatch[0] bounds the string -- the search begins at rm_so, the string
 * ends at rm_eo, and a NUL before it is an ordinary byte. */
#define REG_NOTBOL 1
#define REG_NOTEOL (1 << 1)
#define REG_STARTEND (1 << 2)

/* The error codes: regcomp never returns REG_EEND, and reports an unmatched
 * ) -- REG_ERPAREN -- as REG_EPAREN, as glibc's does. */
typedef enum
{
  _REG_ENOSYS = -1,
  _REG_NOERROR = 0,
  _REG_NOMATCH,
  _REG_BADPAT,
  _REG_ECOLLATE,
  _REG_ECTYPE,
  _REG_EESCAPE,
  _REG_ESUBREG,
  _REG_EBRACK,
  _REG_EPAREN,
  _REG_EBRACE,
  _REG_BADBR,
  _REG_ERANGE,
  _REG_ESPACE,
  _REG_BADRPT,
  _REG_EEND,
  _REG_ESIZE,
  _REG_ERPAREN
} reg_errcode_t;

#define REG_ENOSYS _REG_ENOSYS
#define REG_NOERROR _REG_NOERROR
#define REG_OK _REG_NOERROR
#define REG_NOMATCH _REG_NOMATCH
#define REG_BADPAT _REG_BADPAT
#define REG_ECOLLATE _REG_ECOLLATE
#define REG_ECTYPE _REG_ECTYPE
#define REG_EESCAPE _REG_EESCAPE
#define REG_ESUBREG _REG_ESUBREG
#define REG_EBRACK _REG_EBRACK
#define REG_EPAREN _REG_EPAREN
#define REG_EBRACE _REG_EBRACE
#define REG_BADBR _REG_BADBR
#define REG_ERANGE _REG_ERANGE
#define REG_ESPACE _REG_ESPACE
#define REG_BADRPT _REG_BADRPT
#define REG_EEND _REG_EEND
#define REG_ESIZE _REG_ESIZE
#define REG_ERPAREN _REG_ERPAREN

/* The type of a translate table: 256 bytes, each byte's stand-in. A
 * program may define both names itself first, as glibc's header lets it. */
#ifndef RE_TRANSLATE_TYPE
# define __RE_TRANSLATE_TYPE unsigned char *
# ifdef _GNU_SOURCE
#  define RE_TRANSLATE_TYPE __RE_TRANSLATE_TYPE
# endif
#endif

/* The fields' names: glibc's for _GNU_SOURCE, with __ before them for a
 * program that has not asked for them. */
#ifdef _GNU_SOURCE
# define __REPB_PREFIX(name) name
#else
# define __REPB_PREFIX(name) __##name
#endif

/* A compiled pattern. Before re_compile_pattern a program may set buffer
 * and allocated (a malloc'ed block to compile into, or NULL and 0),
 * fastmap (256 bytes, or NULL) and translate; after it, re_nsub, not_bol
 * and not_eol are its to read and set, and newline_anchor. The rest is the
 * library's. */
struct re_pattern_buffer
{
  /* The compiled pattern, the library's own. */
  struct re_dfa_t *__REPB_PREFIX(buffer);
  /* The size of the block buffer points to, and how much of it is used. */
  __re_long_size_t __REPB_PREFIX(allocated);
  __re_long_size_t __REPB_PREFIX(used);
  /* The syntax the pattern was read in. */
  reg_syntax_t __REPB_PREFIX(syntax);
  /* Which bytes a match can begin with, if not NULL: re_search skips the
   * rest. */
  char *__REPB_PREFIX(fastmap);
  /* Each byte's stand-in, pattern and string alike, if not NULL. */
  __RE_TRANSLATE_TYPE __REPB_PREFIX(translate);
  /* The number of subexpressions. */
  size_t re_nsub;
  /* Whether the pattern can match the empty string, as the fastmap says. */
  unsigned __REPB_PREFIX(can_be_null) : 1;
#ifdef _GNU_SOURCE
# define REGS_UNALLOCATED 0
# define REGS_REALLOCATE 1
# define REGS_FIXED 2
#endif
  /* Whether re_search allocates a struct re_registers's arrays, grows
   * them, or uses them as they are. */
  unsigned __REPB_PREFIX(regs_allocated) : 2;
  /* Whether fastmap is up to date. */
  unsigned __REPB_PREFIX(fastmap_accurate) : 1;
  /* No subexpressions are reported. */
  unsigned __REPB_PREFIX(no_sub) : 1;
  /* ^ does not match at the string's start; $ not at its end. */
  unsigned __REPB_PREFIX(not_bol) : 1;
  unsigned __REPB_PREFIX(not_eol) : 1;
  /* ^ and $ match at a newline. */
  unsigned __REPB_PREFIX(newline_anchor) : 1;
};

typedef struct re_pattern_buffer regex_t;

#ifdef _GNU_SOURCE
/* Where re_search and re_match report the subexpressions: num_regs of
 * each, start[i] and end[i] for subexpression i, -1 for one that took no
 * part. */
struct re_registers
{
  __re_size_t num_regs;
  regoff_t *start;
  regoff_t *end;
};

/* How many registers re_search allocates the first time, at least. */
# ifndef RE_NREGS
#  define RE_NREGS 30
# endif
#endif

/* POSIX's: one subexpression's match, -1 and -1 for one that took no
 * part. */
typedef struct
{
  regoff_t rm_so;
  regoff_t rm_eo;
} regmatch_t;

#ifdef _GNU_SOURCE
/* re_syntax_options set, the old value returned. */
extern reg_syntax_t re_set_syntax (reg_syntax_t __syntax);

/* `__length` bytes of `__pattern` compiled in re_syntax_options's syntax
 * into `__buffer`: NULL, or what regerror would say of the error. */
extern const char *re_compile_pattern (const char *__pattern, size_t __length,
				       struct re_pattern_buffer *__buffer);

/* __buffer's fastmap filled in: 0. */
extern int re_compile_fastmap (struct re_pattern_buffer *__buffer);

/* The first match in `__String` (`__length` bytes) beginning at `__start`
 * or after it, `__range` bytes on at most (before it, if negative): where it
 * begins, -1 for none, -2 for an internal error -- and its subexpressions
 * in `__regs`, if not NULL. */
extern regoff_t re_search (struct re_pattern_buffer *__buffer,
			   const char *__String, regoff_t __length,
			   regoff_t __start, regoff_t __range,
			   struct re_registers *__regs);

/* re_search in the two strings taken as one, a match ending by `__stop`. */
extern regoff_t re_search_2 (struct re_pattern_buffer *__buffer,
			     const char *__string1, regoff_t __length1,
			     const char *__string2, regoff_t __length2,
			     regoff_t __start, regoff_t __range,
			     struct re_registers *__regs,
			     regoff_t __stop);

/* A match beginning at `__start` itself: its length, -1 for none. */
extern regoff_t re_match (struct re_pattern_buffer *__buffer,
			  const char *__String, regoff_t __length,
			  regoff_t __start, struct re_registers *__regs);

/* re_match in the two strings taken as one. */
extern regoff_t re_match_2 (struct re_pattern_buffer *__buffer,
			    const char *__string1, regoff_t __length1,
			    const char *__string2, regoff_t __length2,
			    regoff_t __start, struct re_registers *__regs,
			    regoff_t __stop);

/* `__regs` given the program's own arrays, `__num_regs` long (malloc'ed:
 * re_search may grow them), or none, for 0. */
extern void re_set_registers (struct re_pattern_buffer *__buffer,
			      struct re_registers *__regs,
			      __re_size_t __num_regs,
			      regoff_t *__starts, regoff_t *__ends);
#endif

#ifdef _REGEX_RE_COMP
/* 4.2BSD's: one pattern at a time, in re_syntax_options's syntax. */
extern char *re_comp (const char *);
extern int re_exec (const char *);
#endif

int regcomp (regex_t *__restrict __preg, const char *__restrict __pattern,
	     int __cflags);
int regexec (const regex_t *__restrict __preg, const char *__restrict __String,
	     size_t __nmatch, regmatch_t *__restrict __pmatch, int __eflags);
size_t regerror (int __errcode, const regex_t *__restrict __preg,
		 char *__restrict __errbuf, size_t __errbuf_size);
void regfree (regex_t *__preg);

#ifdef __cplusplus
}
#endif

#endif /* _REGEX_H */
