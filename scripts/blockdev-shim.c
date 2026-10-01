/*
 * An `ioctl` interposer for scripts/blockdev-diff.sh. Loaded with
 * LD_PRELOAD into both sides -- util-linux's blockdev and ours, which both
 * reach `ioctl` through glibc's dynamic symbol -- it answers the block
 * device requests for descriptors it recognises as fixtures, as a block
 * device described by the fixture would, and logs every request made on
 * one to $SHIM_LOG. The harness compares the logs, so the requests each
 * side makes, their arguments and their order are measured as well as
 * what is printed.
 *
 * A descriptor is a fixture when it is
 *
 *   * a regular file whose first line is "#blockdev-shim", or
 *   * a character device whose MAJ:MIN $SHIM_DEVMAP maps to such a file
 *     ("1:5=/path/a,1:7=/path/b") -- a device node, unlike a file, has a
 *     st_rdev, which is what `--report` looks a partition's start up by.
 *
 * The fixture's other lines are "REQUEST VALUE" ("BLKSSZGET 512",
 * "BLKRAGET -1") or "REQUEST !ERRNO" ("BLKRAGET !13"). A request that
 * reads a value the fixture does not list fails with ENOTTY, as it would on
 * a regular file; one that sets (BLKROSET, BLKBSZSET, BLKRASET, BLKFRASET,
 * BLKFLSBUF, BLKRRPART) succeeds unless listed with an errno. Every other
 * descriptor goes to the real ioctl untouched.
 *
 * Each value is written with the size the kernel writes it with, whatever
 * the caller's buffer -- so a port that passes too small a buffer, or
 * reads back too wide a one, prints something else and is caught.
 */
#define _GNU_SOURCE
#include <dlfcn.h>
#include <errno.h>
#include <fcntl.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/sysmacros.h>
#include <unistd.h>
#include <linux/fs.h>

#define MAGIC "#blockdev-shim\n"

struct req {
	unsigned long nr;
	const char *name;
	int size;	/* bytes the kernel writes (or reads, for a set) */
	int set;	/* a request that sets: succeeds unless failed */
	int byval;	/* the argument is the value, not a pointer */
};

static const struct req reqs[] = {
	{ BLKROSET,         "BLKROSET",         4, 1, 0 },
	{ BLKROGET,         "BLKROGET",         4, 0, 0 },
	{ BLKRRPART,        "BLKRRPART",        0, 1, 1 },
	{ BLKGETSIZE,       "BLKGETSIZE",       8, 0, 0 },
	{ BLKFLSBUF,        "BLKFLSBUF",        0, 1, 1 },
	{ BLKRASET,         "BLKRASET",         0, 1, 1 },
	{ BLKRAGET,         "BLKRAGET",         8, 0, 0 },
	{ BLKFRASET,        "BLKFRASET",        0, 1, 1 },
	{ BLKFRAGET,        "BLKFRAGET",        8, 0, 0 },
	{ BLKSECTGET,       "BLKSECTGET",       2, 0, 0 },
	{ BLKSSZGET,        "BLKSSZGET",        4, 0, 0 },
	{ BLKBSZGET,        "BLKBSZGET",        4, 0, 0 },
	{ BLKBSZSET,        "BLKBSZSET",        4, 1, 0 },
	{ BLKGETSIZE64,     "BLKGETSIZE64",     8, 0, 0 },
	{ BLKIOMIN,         "BLKIOMIN",         4, 0, 0 },
	{ BLKIOOPT,         "BLKIOOPT",         4, 0, 0 },
	{ BLKALIGNOFF,      "BLKALIGNOFF",      4, 0, 0 },
	{ BLKPBSZGET,       "BLKPBSZGET",       4, 0, 0 },
	{ BLKDISCARDZEROES, "BLKDISCARDZEROES", 4, 0, 0 },
	{ BLKGETDISKSEQ,    "BLKGETDISKSEQ",    8, 0, 0 },
};

/* The fixture behind `fd`, into `path`; 0 when it is not one. */
static int fixture_of(int fd, char *path, size_t len)
{
	struct stat st;
	char buf[sizeof(MAGIC)];
	int f, n;

	if (fstat(fd, &st) != 0)
		return 0;
	if (S_ISCHR(st.st_mode)) {
		const char *map = getenv("SHIM_DEVMAP");
		char want[32];
		const char *p;

		if (!map)
			return 0;
		snprintf(want, sizeof(want), "%u:%u=", major(st.st_rdev), minor(st.st_rdev));
		for (p = map; p && *p; p = strchr(p, ',') ? strchr(p, ',') + 1 : NULL) {
			if (strncmp(p, want, strlen(want)) == 0) {
				const char *v = p + strlen(want);
				size_t n2 = strcspn(v, ",");

				if (n2 >= len)
					return 0;
				memcpy(path, v, n2);
				path[n2] = '\0';
				return 1;
			}
		}
		return 0;
	}
	if (!S_ISREG(st.st_mode))
		return 0;
	/* Reopened through /proc, so a file bind-mounted over a /dev node is
	 * recognised by what it holds, not by the name it is reached by. */
	snprintf(path, len, "/proc/self/fd/%d", fd);
	f = open(path, O_RDONLY | O_CLOEXEC);
	if (f < 0)
		return 0;
	n = read(f, buf, sizeof(MAGIC) - 1);
	close(f);
	if (n != (int) sizeof(MAGIC) - 1 || memcmp(buf, MAGIC, sizeof(MAGIC) - 1) != 0)
		return 0;
	return 1;
}

/* The fixture's line for `name`: 1 and the value or errno, else 0. */
static int lookup(const char *path, const char *name, unsigned long long *val, int *err)
{
	FILE *f = fopen(path, "re");
	char line[256], key[64], v[64];
	int found = 0;

	if (!f)
		return 0;
	while (fgets(line, sizeof(line), f)) {
		if (sscanf(line, "%63s %63s", key, v) != 2 || strcmp(key, name) != 0)
			continue;
		if (v[0] == '!') {
			*err = atoi(v + 1);
		} else {
			*err = 0;
			*val = v[0] == '-' ? (unsigned long long) strtoll(v, NULL, 0)
					   : strtoull(v, NULL, 0);
		}
		found = 1;
		break;
	}
	fclose(f);
	return found;
}

/* The name the log shows a fixture by: its file's last component. */
static const char *base(const char *path, int fd, char *buf, size_t len)
{
	char link[64];
	ssize_t n;
	const char *slash;

	if (strncmp(path, "/proc/self/fd/", 14) == 0) {
		snprintf(link, sizeof(link), "/proc/self/fd/%d", fd);
		n = readlink(link, buf, len - 1);
		if (n < 0)
			return "?";
		buf[n] = '\0';
		path = buf;
	}
	slash = strrchr(path, '/');
	return slash ? slash + 1 : path;
}

static void logf_(const char *fmt, ...)
{
	const char *log = getenv("SHIM_LOG");
	char line[512];
	va_list ap;
	int fd, n;

	if (!log)
		return;
	va_start(ap, fmt);
	n = vsnprintf(line, sizeof(line), fmt, ap);
	va_end(ap);
	if (n < 0)
		return;
	if ((size_t) n >= sizeof(line))
		n = sizeof(line) - 1;
	fd = open(log, O_WRONLY | O_APPEND | O_CREAT | O_CLOEXEC, 0644);
	if (fd < 0)
		return;
	if (write(fd, line, n) < 0)
		{ /* nothing to be done; the comparison will show it */ }
	close(fd);
}

int ioctl(int fd, unsigned long request, ...)
{
	static int (*real)(int, unsigned long, ...);
	char path[4096], namebuf[4096];
	const struct req *r = NULL;
	unsigned long long val = 0;
	const char *who;
	void *arg;
	va_list ap;
	size_t i;
	int e = 0, saved = errno;

	va_start(ap, request);
	arg = va_arg(ap, void *);
	va_end(ap);

	if (!fixture_of(fd, path, sizeof(path))) {
		if (!real)
			real = (int (*)(int, unsigned long, ...)) dlsym(RTLD_NEXT, "ioctl");
		errno = saved;
		return real(fd, request, arg);
	}
	who = base(path, fd, namebuf, sizeof(namebuf));

	for (i = 0; i < sizeof(reqs) / sizeof(reqs[0]); i++)
		if (reqs[i].nr == request)
			r = &reqs[i];
	if (!r) {
		logf_("%s 0x%lx -> ENOTTY\n", who, request);
		errno = ENOTTY;
		return -1;
	}

	if (r->set) {
		if (r->byval)
			logf_("%s %s arg=0x%lx", who, r->name, (unsigned long) arg);
		else
			logf_("%s %s *arg=%d", who, r->name, *(int *) arg);
		if (lookup(path, r->name, &val, &e) && e) {
			logf_(" -> errno %d\n", e);
			errno = e;
			return -1;
		}
		logf_("\n");
		return 0;
	}

	if (!lookup(path, r->name, &val, &e))
		e = ENOTTY;
	if (e) {
		logf_("%s %s -> errno %d\n", who, r->name, e);
		errno = e;
		return -1;
	}
	logf_("%s %s -> %llu\n", who, r->name, val);
	memcpy(arg, &val, r->size);
	return 0;
}
