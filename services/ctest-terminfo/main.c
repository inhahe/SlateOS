/*
 * ctest-terminfo -- is the terminfo database on the image, whole, and does
 * each name in it reach its entry?
 *
 * Programs that draw on a terminal look $TERM up in the terminfo database,
 * which scripts/create-ext4-rootfs.sh compiles whole into /usr/share/terminfo
 * (requests/b-d-the-image-has-no-terminfo-database-and-tic-can-now-build-one.md).
 * Each entry is a compiled file, DIR/<its first character>/<name>, and each
 * of its other names a symbolic link to it -- the first symbolic links on
 * the image, some through "..", which the kernel's ext4 walk follows.
 *
 * The directories are searched as ncurses searches them when $TERMINFO_DIRS
 * is set -- its entries, in order -- and otherwise its compiled-in list,
 * /etc/terminfo, /lib/terminfo, /usr/share/terminfo. It passes on Linux
 * with TERMINFO_DIRS naming a tree `tic -x` wrote from the same source.
 *
 * Exit code 42 == every check passed. Anything else is the first failing
 * check: the tens digit names the check, the units which name it was at
 * (NAMES' order) or its step. No check is numbered 4, so that no failure
 * reads as 42.
 *
 *   1x  NAMES[x] is found in one of the directories
 *   2x  NAMES[x] is a compiled entry: the legacy (0432) or the extended
 *       numbers (01036) format, its header whole
 *   3x  NAMES[x]'s names section lists it
 *   5x  vt100-am, another name of vt100: a symbolic link (50), to "vt100"
 *       (51), and through it vt100's entry (52), byte for byte (53)
 *   6x  x1700, a name of diablo1640's: a symbolic link (60) up and across,
 *       to "../d/diablo1640" (61), whose entry lists x1700 (62)
 *   7x  the database is whole: the directory holding xterm-256color has at
 *       least 1800 entries (70) and 1000 other names (71), and opening it
 *       fails nowhere (72)
 */

#define _GNU_SOURCE
#include <dirent.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

/* The terminals anything here is likely to be asked about: the desktop
 * terminal's own, a Linux console's, the DEC ones serial consoles
 * emulate, screen's and tmux's, and the two every system has. */
static const char *const NAMES[] = {
    "xterm-256color", "linux", "vt100", "vt220", "screen", "tmux-256color", "dumb", "ansi",
};
#define NNAMES (sizeof NAMES / sizeof NAMES[0])

static const char *const DEFAULT_DIRS[] = {"/etc/terminfo", "/lib/terminfo",
                                           "/usr/share/terminfo"};

/* The directories to search, NULL-terminated. */
static const char *dirs[16];

static void search_list(void)
{
    const char *env = getenv("TERMINFO_DIRS");
    size_t n = 0;
    if (env != NULL && *env != '\0') {
        static char copy[1024];
        snprintf(copy, sizeof copy, "%s", env);
        for (char *save = NULL, *d = strtok_r(copy, ":", &save);
             d != NULL && n < 15; d = strtok_r(NULL, ":", &save))
            dirs[n++] = d;
    } else {
        for (size_t i = 0; i < 3; i++)
            dirs[n++] = DEFAULT_DIRS[i];
    }
    dirs[n] = NULL;
}

/* Where `name`'s entry is, as DIR/<c>/<name>: 1 and the path in `path`,
 * or 0. */
static int find(const char *name, char *path, size_t cap)
{
    for (size_t i = 0; dirs[i] != NULL; i++) {
        size_t len = strlen(dirs[i]) + 3 + strlen(name) + 1;
        if (len > cap)
            continue;
        strcpy(path, dirs[i]);
        strcat(path, "/");
        size_t at = strlen(path);
        path[at] = name[0];
        path[at + 1] = '/';
        path[at + 2] = '\0';
        strcat(path, name);
        if (access(path, R_OK) == 0)
            return 1;
    }
    return 0;
}

/* The whole of `path`, up to `cap` bytes, into `buf`: its length, or -1. */
static long slurp(const char *path, unsigned char *buf, size_t cap)
{
    int fd = open(path, O_RDONLY | O_CLOEXEC);
    if (fd < 0)
        return -1;
    size_t have = 0;
    for (;;) {
        ssize_t got = read(fd, buf + have, cap - have);
        if (got < 0) {
            close(fd);
            return -1;
        }
        if (got == 0)
            break;
        have += (size_t)got;
        if (have == cap)
            break;
    }
    close(fd);
    return (long)have;
}

static unsigned short le16(const unsigned char *at)
{
    return (unsigned short)(at[0] | (at[1] << 8));
}

/* Whether `entry`, `len` bytes, is a compiled entry with a whole header and
 * names section. */
static int is_entry(const unsigned char *entry, long len)
{
    if (len < 12)
        return 0;
    unsigned short magic = le16(entry);
    if (magic != 0432 && magic != 01036)
        return 0;
    return le16(entry + 2) > 0 && 12 + (long)le16(entry + 2) <= len;
}

/* Whether the names section of `entry` (is_entry's) lists `name`: one of
 * its '|'-separated fields but the last, which describes the terminal. */
static int lists(const unsigned char *entry, const char *name)
{
    size_t size = le16(entry + 2);
    const char *names = (const char *)entry + 12;
    if (memchr(names, '\0', size) == NULL)
        return 0;
    const char *last = strrchr(names, '|');
    size_t want = strlen(name);
    for (const char *f = names; last != NULL && f <= last;) {
        const char *bar = strchr(f, '|');
        if ((size_t)(bar - f) == want && memcmp(f, name, want) == 0)
            return 1;
        f = bar + 1;
    }
    return 0;
}

static unsigned char entry[65536];
static unsigned char other[65536];

/* Checks 5x and 6x: `alias`, found, is a symbolic link to `target`; its
 * entry lists `alias`; and, when `same` names an entry, is that entry's,
 * byte for byte. 0, or the step that failed (0-3) plus one. */
static int alias_check(const char *alias, const char *target, const char *same)
{
    char path[512];
    struct stat st;
    if (!find(alias, path, sizeof path) || lstat(path, &st) != 0 || !S_ISLNK(st.st_mode))
        return 1;
    char to[256];
    ssize_t n = readlink(path, to, sizeof to - 1);
    if (n < 0)
        return 2;
    to[n] = '\0';
    if (strcmp(to, target) != 0)
        return 2;
    long len = slurp(path, entry, sizeof entry);
    if (!is_entry(entry, len) || !lists(entry, alias))
        return 3;
    if (same != NULL) {
        char spath[512];
        if (!find(same, spath, sizeof spath))
            return 4;
        long slen = slurp(spath, other, sizeof other);
        if (slen != len || memcmp(entry, other, (size_t)len) != 0)
            return 4;
    }
    return 0;
}

/* Check 7x: under `dir`, the entries (regular files) and other names
 * (symbolic links) in its one-character subdirectories, and whether every
 * entry opens. */
static int count(const char *dir, long *files, long *links)
{
    *files = 0;
    *links = 0;
    DIR *top = opendir(dir);
    if (top == NULL)
        return 0;
    struct dirent *d;
    int ok = 1;
    while ((d = readdir(top)) != NULL) {
        if (d->d_name[0] == '.' || d->d_name[1] != '\0')
            continue;
        char sub[512];
        if (strlen(dir) + 3 > sizeof sub)
            continue;
        strcpy(sub, dir);
        strcat(sub, "/");
        strcat(sub, d->d_name);
        DIR *s = opendir(sub);
        if (s == NULL) {
            ok = 0;
            continue;
        }
        int sfd = dirfd(s);
        struct dirent *e;
        while ((e = readdir(s)) != NULL) {
            if (e->d_name[0] == '.')
                continue;
            struct stat st;
            if (fstatat(sfd, e->d_name, &st, AT_SYMLINK_NOFOLLOW) != 0) {
                ok = 0;
                continue;
            }
            if (S_ISLNK(st.st_mode)) {
                (*links)++;
            } else if (S_ISREG(st.st_mode)) {
                (*files)++;
                int fd = openat(sfd, e->d_name, O_RDONLY | O_CLOEXEC);
                if (fd < 0)
                    ok = 0;
                else
                    close(fd);
            }
        }
        closedir(s);
    }
    closedir(top);
    return ok;
}

int main(void)
{
    alarm(50);
    search_list();
    char path[512];
    char where[512] = "";

    for (size_t i = 0; i < NNAMES; i++) {
        if (!find(NAMES[i], path, sizeof path))
            return 10 + (int)i;
        if (i == 0) {
            /* DIR of DIR/x/xterm-256color. */
            strcpy(where, path);
            *strrchr(where, '/') = '\0';
            *strrchr(where, '/') = '\0';
        }
        long len = slurp(path, entry, sizeof entry);
        if (!is_entry(entry, len))
            return 20 + (int)i;
        if (!lists(entry, NAMES[i]))
            return 30 + (int)i;
    }

    int step = alias_check("vt100-am", "vt100", "vt100");
    if (step != 0)
        return 50 + step - 1;
    step = alias_check("x1700", "../d/diablo1640", NULL);
    if (step != 0)
        return 60 + step - 1;

    long files, links;
    int opened = count(where, &files, &links);
    if (files < 1800)
        return 70;
    if (links < 1000)
        return 71;
    if (!opened)
        return 72;
    return 42;
}
