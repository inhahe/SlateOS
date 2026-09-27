/*
 * The reference side of scripts/blkid-diff.sh: libblkid's answers about
 * each image named on the command line, printed in a form the Rust side
 * (userspace/ulblkid/examples/blkid-probe.rs) prints identically.
 *
 * Four questions per image, each on a fresh probe:
 *
 *   safeprobe   blkid_do_safeprobe with every superblock value asked for
 *               (not SBBADCSUM) and partitions with PART_ENTRY_* and
 *               PTMAGIC -- what `blkid -p` asks;
 *   badcsum     the same, accepting bad checksums (BLKID_SUBLKS_BADCSUM);
 *   fullprobe   blkid_do_fullprobe, which does not refuse ambiguity;
 *   walk        blkid_do_probe step by step, each result hidden with a
 *               dry-run blkid_do_wipe before the next -- what `wipefs` does;
 *   parts       the binary partition list, every table and partition;
 *   parts-gpt   the same with BLKID_PARTS_FORCE_GPT.
 *
 * Values are printed with their length and every byte outside printable
 * ASCII (and the backslash) as \xHH, so a trailing NUL, a stray byte or a
 * binary UUID_RAW are all visible.
 *
 * Built by the harness with `gcc -O2 ... -lblkid`.
 */
#include <blkid/blkid.h>
#include <inttypes.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static void put_escaped(const unsigned char *d, size_t len)
{
	for (size_t i = 0; i < len; i++) {
		unsigned char c = d[i];
		if (c >= 0x20 && c < 0x7f && c != '\\')
			putchar(c);
		else
			printf("\\x%02x", c);
	}
}

static void put_str(const char *s)
{
	if (!s || !*s) {
		fputs("-", stdout);
		return;
	}
	put_escaped((const unsigned char *) s, strlen(s));
}

static void dump_values(blkid_probe pr)
{
	int n = blkid_probe_numof_values(pr);

	for (int i = 0; i < n; i++) {
		const char *name = NULL, *data = NULL;
		size_t len = 0;

		if (blkid_probe_get_value(pr, i, &name, &data, &len) != 0)
			continue;
		printf("  %s len=%zu ", name, len);
		put_escaped((const unsigned char *) data, len);
		putchar('\n');
	}
}

static blkid_probe open_probe(const char *path, int badcsum)
{
	blkid_probe pr = blkid_new_probe_from_filename(path);

	if (!pr)
		return NULL;
	blkid_probe_enable_superblocks(pr, 1);
	blkid_probe_set_superblocks_flags(pr,
		BLKID_SUBLKS_LABEL | BLKID_SUBLKS_LABELRAW |
		BLKID_SUBLKS_UUID | BLKID_SUBLKS_UUIDRAW |
		BLKID_SUBLKS_TYPE | BLKID_SUBLKS_SECTYPE |
		BLKID_SUBLKS_USAGE | BLKID_SUBLKS_VERSION |
		BLKID_SUBLKS_MAGIC | BLKID_SUBLKS_FSINFO |
		(badcsum ? BLKID_SUBLKS_BADCSUM : 0));
	blkid_probe_enable_partitions(pr, 1);
	blkid_probe_set_partitions_flags(pr,
		BLKID_PARTS_ENTRY_DETAILS | BLKID_PARTS_MAGIC);
	return pr;
}

/* The table's place in the order tables are first met, walking the
 * partitions; tables with no partitions (AIX's, an empty nested one) are
 * only reachable as the root. */
static void dump_table(blkid_parttable tab)
{
	blkid_partition parent;

	/* A protective MBR on its own is found without making a table. */
	if (!tab) {
		printf("none");
		return;
	}
	parent = blkid_parttable_get_parent(tab);
	printf("type=");
	put_str(blkid_parttable_get_type(tab));
	printf(" offset=%jd id=", (intmax_t) blkid_parttable_get_offset(tab));
	put_str(blkid_parttable_get_id(tab));
	printf(" parent=%d", parent ? blkid_partition_get_partno(parent) : 0);
}

static void dump_parts(const char *path, int flags)
{
	blkid_probe pr = blkid_new_probe_from_filename(path);
	blkid_partlist ls;

	if (!pr) {
		printf("  open failed\n");
		return;
	}
	blkid_probe_enable_superblocks(pr, 0);
	blkid_probe_enable_partitions(pr, 1);
	blkid_probe_set_partitions_flags(pr, flags);
	ls = blkid_probe_get_partitions(pr);
	if (!ls) {
		printf("  none\n");
		blkid_free_probe(pr);
		return;
	}
	printf("  root ");
	dump_table(blkid_partlist_get_table(ls));
	putchar('\n');

	int n = blkid_partlist_numof_partitions(ls);
	for (int i = 0; i < n; i++) {
		blkid_partition par = blkid_partlist_get_partition(ls, i);

		printf("  part %d start=%jd size=%jd type=0x%x typestr=",
			blkid_partition_get_partno(par),
			(intmax_t) blkid_partition_get_start(par),
			(intmax_t) blkid_partition_get_size(par),
			blkid_partition_get_type(par));
		put_str(blkid_partition_get_type_string(par));
		printf(" name=");
		put_str(blkid_partition_get_name(par));
		printf(" uuid=");
		put_str(blkid_partition_get_uuid(par));
		printf(" flags=0x%llx kind=%c%c%c table: ",
			blkid_partition_get_flags(par),
			blkid_partition_is_primary(par) ? 'P' : '-',
			blkid_partition_is_extended(par) ? 'E' : '-',
			blkid_partition_is_logical(par) ? 'L' : '-');
		dump_table(blkid_partition_get_table(par));
		putchar('\n');
	}
	blkid_free_probe(pr);
}

int main(int argc, char **argv)
{
	for (int a = 1; a < argc; a++) {
		const char *path = argv[a];
		const char *base = strrchr(path, '/');
		blkid_probe pr;
		int rc;

		printf("# %s\n", base ? base + 1 : path);

		for (int badcsum = 0; badcsum < 2; badcsum++) {
			pr = open_probe(path, badcsum);
			if (!pr) {
				printf("open failed\n");
				continue;
			}
			rc = blkid_do_safeprobe(pr);
			printf("%s rc=%d\n", badcsum ? "badcsum" : "safeprobe", rc);
			dump_values(pr);
			blkid_free_probe(pr);
		}

		pr = open_probe(path, 0);
		if (pr) {
			rc = blkid_do_fullprobe(pr);
			printf("fullprobe rc=%d\n", rc);
			dump_values(pr);
			blkid_free_probe(pr);
		}

		pr = open_probe(path, 0);
		if (pr) {
			printf("walk\n");
			for (int step = 0; step < 32; step++) {
				rc = blkid_do_probe(pr);
				printf(" step rc=%d\n", rc);
				if (rc != 0)
					break;
				dump_values(pr);
				printf(" wipe rc=%d\n", blkid_do_wipe(pr, 1));
			}
			blkid_free_probe(pr);
		}

		printf("parts\n");
		dump_parts(path, 0);
		printf("parts-gpt\n");
		dump_parts(path, BLKID_PARTS_FORCE_GPT);
	}
	return 0;
}
