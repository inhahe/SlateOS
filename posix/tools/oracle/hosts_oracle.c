/* glibc's answers for the hosts database, inside a sandbox whose /etc/hosts
   and /etc/host.conf are the test's and whose network is down. */
#define _GNU_SOURCE 1
#include <arpa/inet.h>
#include <errno.h>
#include <netdb.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static void show(const struct hostent *h, int rc, int herr) {
    if (!h) { printf("NULL rc=%d herr=%d\n", rc, herr); return; }
    printf("name=%s type=%d len=%d addrs=[", h->h_name, h->h_addrtype, h->h_length);
    for (int i = 0; h->h_addr_list[i]; i++) {
        char t[64];
        inet_ntop(h->h_addrtype, h->h_addr_list[i], t, sizeof t);
        printf("%s%s", i ? "," : "", t);
    }
    printf("] aliases=[");
    for (int i = 0; h->h_aliases[i]; i++) printf("%s%s", i ? "," : "", h->h_aliases[i]);
    printf("]\n");
}

static const char *arg(const char *s) { return strcmp(s, "-") ? s : ""; }

int main(void) {
    char line[4096];
    static char buf[8192];
    while (fgets(line, sizeof line, stdin)) {
        line[strcspn(line, "\r\n")] = 0;
        char *argv[5] = {0};
        int argc = 0;
        for (char *t = strtok(line, " "); t && argc < 5; t = strtok(NULL, " ")) argv[argc++] = t;
        if (!argc) continue;
        const char *cmd = argv[0];
        if (!strcmp(cmd, "byname")) {
            struct hostent hb, *r = NULL;
            int herr = -99;
            int rc = gethostbyname_r(arg(argv[1]), &hb, buf, sizeof buf, &r, &herr);
            show(r, rc, herr);
        } else if (!strcmp(cmd, "byname2")) {
            struct hostent hb, *r = NULL;
            int herr = -99;
            int rc = gethostbyname2_r(arg(argv[1]), atoi(argv[2]), &hb, buf, sizeof buf, &r, &herr);
            show(r, rc, herr);
        } else if (!strcmp(cmd, "byaddr")) {
            unsigned char a[16] = {0};
            int af = atoi(argv[2]);
            inet_pton(af == 10 ? AF_INET6 : AF_INET, argv[1], a);
            struct hostent hb, *r = NULL;
            int herr = -99;
            int rc = gethostbyaddr_r(a, (socklen_t)atoi(argv[3]), af, &hb, buf, sizeof buf, &r, &herr);
            show(r, rc, herr);
        } else if (!strcmp(cmd, "small")) {
            struct hostent hb, *r = NULL;
            int herr = -99;
            errno = 0;
            int rc = gethostbyname_r(arg(argv[1]), &hb, buf, (size_t)atoi(argv[2]), &r, &herr);
            printf("rc=%d herr=%d found=%d\n", rc, herr, r != NULL);
        } else if (!strcmp(cmd, "nr")) {
            h_errno = -99;
            struct hostent *h = gethostbyname(arg(argv[1]));
            show(h, 0, h_errno);
        } else if (!strcmp(cmd, "nr2")) {
            h_errno = -99;
            struct hostent *h = gethostbyname2(arg(argv[1]), atoi(argv[2]));
            show(h, 0, h_errno);
        } else if (!strcmp(cmd, "ent")) {
            sethostent(0);
            struct hostent *h;
            while ((h = gethostent())) show(h, 0, 0);
            printf("end herr=%d\n", h_errno);
            endhostent();
        } else {
            printf("?\n");
        }
        fflush(stdout);
    }
    return 0;
}
