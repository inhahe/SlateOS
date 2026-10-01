/*
 * The reference side of scripts/smartcols-diff.sh: a libsmartcols table
 * built from a script on stdin and printed, with the answer of every call
 * that can refuse written to stderr. The Rust side,
 * userspace/smartcols/examples/scols-probe.rs, reads the same script and
 * must write the same bytes to both.
 *
 * One command per line, fields separated by one space. Data is hex, so a
 * cell can hold any byte but NUL (which a C string cannot); "-" is none,
 * and "." is empty.
 *
 *   col HEXNAME FLAGS WHINT    scols_table_new_column; FLAGS is "-" or
 *                              letters: t tree, r right, c trunc, w wrap,
 *                              n noextremes, s strictwidth, h hidden,
 *                              N newline wrapping (wrapnl functions, and
 *                              "\n" as a safe char)
 *   jtype COL TYPE             scols_column_set_json_type: string, number,
 *                              boolean, array-string, array-number,
 *                              boolean-optional
 *   line PARENT                scols_table_new_line; PARENT is a line
 *                              number (lines count from 0) or -1
 *   data LINE COL HEX          scols_line_set_column_data
 *   udata LINE N U64           scols_cell_set_userdata on the line's Nth
 *                              cell, as lsblk's set_sortdata_u64
 *   group LINE MEMBER          scols_table_group_lines; LINE may be -1
 *   link LINE MEMBER           scols_line_link_group
 *   cmp COL str|u64            scols_column_set_cmpfunc: scols_cmpstr_cells
 *                              or lsblk's cmp_u64_cells
 *   sort COL                   scols_sort_table; COL may be -1
 *   sorttree                   scols_sort_table_by_tree
 *   ascii|json|raw|export|noheadings|maxout|minout|nowrap|noencoding|
 *   nolinesep|shellvar         the table's switches, turned on
 *   name HEX                   scols_table_set_name
 *   term WIDTH                 laid out for a terminal WIDTH cells wide
 *   print                      scols_print_table
 *
 * Built by the harness against the installed libsmartcols.so.1 and the
 * header from util-linux 2.39.3's source.
 */
#include <errno.h>
#include <inttypes.h>
#include <locale.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "libsmartcols.h"

#define MAXOBJ 4096

static struct libscols_column *cols[MAXOBJ];
static size_t ncols;
static struct libscols_line *lines[MAXOBJ];
static size_t nlines;

static char *unhex(const char *s)
{
	size_t n = strlen(s) / 2, i;
	char *out;

	if (strcmp(s, "-") == 0)
		return NULL;
	out = calloc(n + 1, 1);
	if (!out)
		exit(99);
	for (i = 0; i < n; i++) {
		unsigned int b;
		if (sscanf(s + 2 * i, "%2x", &b) != 1)
			exit(98);
		out[i] = (char) b;
	}
	return out;
}

static struct libscols_line *line_at(const char *s)
{
	long i = strtol(s, NULL, 10);
	if (i < 0 || (size_t) i >= nlines)
		return NULL;
	return lines[i];
}

static struct libscols_column *col_at(const char *s)
{
	long i = strtol(s, NULL, 10);
	if (i < 0 || (size_t) i >= ncols)
		return NULL;
	return cols[i];
}

/* lsblk's, word for word. */
static int cmp_u64_cells(struct libscols_cell *a,
			 struct libscols_cell *b,
			 __attribute__((__unused__)) void *data)
{
	uint64_t *adata = (uint64_t *) scols_cell_get_userdata(a),
		 *bdata = (uint64_t *) scols_cell_get_userdata(b);

	if (adata == NULL && bdata == NULL)
		return 0;
	if (adata == NULL)
		return -1;
	if (bdata == NULL)
		return 1;
	return *adata == *bdata ? 0 : *adata >= *bdata ? 1 : -1;
}

static int json_type(const char *s)
{
	if (!strcmp(s, "number"))
		return SCOLS_JSON_NUMBER;
	if (!strcmp(s, "boolean"))
		return SCOLS_JSON_BOOLEAN;
	if (!strcmp(s, "array-string"))
		return SCOLS_JSON_ARRAY_STRING;
	if (!strcmp(s, "array-number"))
		return SCOLS_JSON_ARRAY_NUMBER;
	if (!strcmp(s, "boolean-optional"))
		return SCOLS_JSON_BOOLEAN_OPTIONAL;
	return SCOLS_JSON_STRING;
}

int main(void)
{
	struct libscols_table *tb;
	char buf[65536];

	setlocale(LC_ALL, "");
	tb = scols_new_table();
	if (!tb)
		return 99;
	scols_table_set_termforce(tb, SCOLS_TERMFORCE_NEVER);

	while (fgets(buf, sizeof(buf), stdin)) {
		char *f[8] = { 0 };
		size_t n = 0;
		char *p = strtok(buf, " \n");

		while (p && n < 8) {
			f[n++] = p;
			p = strtok(NULL, " \n");
		}
		if (n == 0)
			continue;

		if (!strcmp(f[0], "col") && n == 4) {
			int fl = 0, wrapnl = 0;
			char *name = unhex(f[1]);
			const char *c;
			for (c = f[2]; *c && strcmp(f[2], "-"); c++) {
				switch (*c) {
				case 't': fl |= SCOLS_FL_TREE; break;
				case 'r': fl |= SCOLS_FL_RIGHT; break;
				case 'c': fl |= SCOLS_FL_TRUNC; break;
				case 'w': fl |= SCOLS_FL_WRAP; break;
				case 'n': fl |= SCOLS_FL_NOEXTREMES; break;
				case 's': fl |= SCOLS_FL_STRICTWIDTH; break;
				case 'h': fl |= SCOLS_FL_HIDDEN; break;
				case 'N': wrapnl = 1; break;
				}
			}
			cols[ncols] = scols_table_new_column(tb, name, strtod(f[3], NULL), fl);
			if (wrapnl) {
				scols_column_set_wrapfunc(cols[ncols], scols_wrapnl_chunksize,
							  scols_wrapnl_nextchunk, NULL);
				scols_column_set_safechars(cols[ncols], "\n");
			}
			ncols++;
			free(name);
		} else if (!strcmp(f[0], "jtype") && n == 3) {
			scols_column_set_json_type(col_at(f[1]), json_type(f[2]));
		} else if (!strcmp(f[0], "line") && n == 2) {
			lines[nlines++] = scols_table_new_line(tb, line_at(f[1]));
		} else if (!strcmp(f[0], "data") && n == 4) {
			char *d = unhex(f[3]);
			scols_line_set_column_data(line_at(f[1]), col_at(f[2]), d);
			free(d);
		} else if (!strcmp(f[0], "udata") && n == 4) {
			struct libscols_cell *ce = scols_line_get_cell(line_at(f[1]),
						(size_t) strtoul(f[2], NULL, 10));
			uint64_t *x = malloc(sizeof(*x));
			if (!x)
				return 99;
			*x = strtoull(f[3], NULL, 10);
			scols_cell_set_userdata(ce, x);
		} else if (!strcmp(f[0], "group") && n == 3) {
			fprintf(stderr, "group %s %s: %d\n", f[1], f[2],
				scols_table_group_lines(tb, line_at(f[1]), line_at(f[2]), 0));
		} else if (!strcmp(f[0], "link") && n == 3) {
			fprintf(stderr, "link %s %s: %d\n", f[1], f[2],
				scols_line_link_group(line_at(f[1]), line_at(f[2]), 0));
		} else if (!strcmp(f[0], "cmp") && n == 3) {
			scols_column_set_cmpfunc(col_at(f[1]),
				!strcmp(f[2], "u64") ? cmp_u64_cells : scols_cmpstr_cells, NULL);
		} else if (!strcmp(f[0], "sort") && n == 2) {
			fprintf(stderr, "sort %s: %d\n", f[1], scols_sort_table(tb, col_at(f[1])));
		} else if (!strcmp(f[0], "sorttree") && n == 1) {
			fprintf(stderr, "sorttree: %d\n", scols_sort_table_by_tree(tb));
		} else if (!strcmp(f[0], "ascii")) {
			scols_table_enable_ascii(tb, 1);
		} else if (!strcmp(f[0], "json")) {
			scols_table_enable_json(tb, 1);
		} else if (!strcmp(f[0], "raw")) {
			scols_table_enable_raw(tb, 1);
		} else if (!strcmp(f[0], "export")) {
			scols_table_enable_export(tb, 1);
		} else if (!strcmp(f[0], "noheadings")) {
			scols_table_enable_noheadings(tb, 1);
		} else if (!strcmp(f[0], "maxout")) {
			scols_table_enable_maxout(tb, 1);
		} else if (!strcmp(f[0], "minout")) {
			scols_table_enable_minout(tb, 1);
		} else if (!strcmp(f[0], "nowrap")) {
			scols_table_enable_nowrap(tb, 1);
		} else if (!strcmp(f[0], "noencoding")) {
			scols_table_enable_noencoding(tb, 1);
		} else if (!strcmp(f[0], "nolinesep")) {
			scols_table_enable_nolinesep(tb, 1);
		} else if (!strcmp(f[0], "shellvar")) {
			scols_table_enable_shellvar(tb, 1);
		} else if (!strcmp(f[0], "name") && n == 2) {
			char *d = unhex(f[1]);
			scols_table_set_name(tb, d);
			free(d);
		} else if (!strcmp(f[0], "term") && n == 2) {
			scols_table_set_termforce(tb, SCOLS_TERMFORCE_ALWAYS);
			scols_table_set_termwidth(tb, (size_t) strtoul(f[1], NULL, 10));
		} else if (!strcmp(f[0], "print")) {
			fprintf(stderr, "print: %d\n", scols_print_table(tb));
		} else {
			fprintf(stderr, "bad command: %s\n", f[0]);
			return 2;
		}
	}
	fflush(stdout);
	return 0;
}
