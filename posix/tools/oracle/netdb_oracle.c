/* glibc's answers for the services/protocols/networks/ethers databases.
   Run inside a sandbox whose /etc/{services,protocols,networks,ethers} are
   the test's own files.  stdin: one command per line; stdout: one line per
   command (enumerations print one line per entry and then "end"). */
#define _GNU_SOURCE 1
#include <arpa/inet.h>
#include <errno.h>
#include <netdb.h>
#include <netinet/ether.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static const char *arg(const char *s) { return strcmp(s, "-") ? s : NULL; }

static void list(char **l) {
    printf(" aliases=[");
    for (int i = 0; l && l[i]; i++) printf("%s%s", i ? "," : "", l[i]);
    printf("]");
}

static void serv(struct servent *s, int rc) {
    if (!s) { printf("NULL rc=%d\n", rc); return; }
    printf("name=%s port=%d proto=%s", s->s_name, ntohs((unsigned short)s->s_port), s->s_proto);
    list(s->s_aliases);
    printf("\n");
}

static void proto(struct protoent *p, int rc) {
    if (!p) { printf("NULL rc=%d\n", rc); return; }
    printf("name=%s proto=%d", p->p_name, p->p_proto);
    list(p->p_aliases);
    printf("\n");
}

static void net(struct netent *n, int rc, int herr) {
    if (!n) { printf("NULL rc=%d herr=%d\n", rc, herr); return; }
    printf("name=%s type=%d net=%08x", n->n_name, n->n_addrtype, n->n_net);
    list(n->n_aliases);
    printf("\n");
}

int main(void) {
    char line[4096];
    static char buf[8192];
    while (fgets(line, sizeof line, stdin)) {
        line[strcspn(line, "\r\n")] = 0;
        char *argv[4] = {0};
        int argc = 0;
        for (char *t = strtok(line, " "); t && argc < 4; t = strtok(NULL, " ")) argv[argc++] = t;
        if (!argc) continue;
        const char *cmd = argv[0];
        if (!strcmp(cmd, "serv")) {
            struct servent sb, *r = NULL;
            int rc = getservbyname_r(argv[1], arg(argv[2]), &sb, buf, sizeof buf, &r);
            serv(r, rc);
        } else if (!strcmp(cmd, "port")) {
            struct servent sb, *r = NULL;
            int rc = getservbyport_r(htons(atoi(argv[1])), arg(argv[2]), &sb, buf, sizeof buf, &r);
            serv(r, rc);
        } else if (!strcmp(cmd, "rawport")) {
            struct servent sb, *r = NULL;
            int rc = getservbyport_r((int)strtol(argv[1], NULL, 0), arg(argv[2]), &sb, buf, sizeof buf, &r);
            serv(r, rc);
        } else if (!strcmp(cmd, "servsmall")) {
            struct servent sb, *r = NULL;
            int rc = getservbyname_r(argv[1], arg(argv[2]), &sb, buf, (size_t)atoi(argv[3]), &r);
            printf("rc=%d found=%d\n", rc, r != NULL);
        } else if (!strcmp(cmd, "servent")) {
            setservent(0);
            struct servent *s;
            while ((s = getservent())) serv(s, 0);
            printf("end\n");
            endservent();
        } else if (!strcmp(cmd, "mix")) {
            setservent(0);
            serv(getservent(), 0);
            serv(getservent(), 0);
            serv(getservbyname(argv[1], NULL), 0);
            serv(getservent(), 0);
            endservent();
        } else if (!strcmp(cmd, "proto")) {
            struct protoent pb, *r = NULL;
            int rc = getprotobyname_r(argv[1], &pb, buf, sizeof buf, &r);
            proto(r, rc);
        } else if (!strcmp(cmd, "protonum")) {
            struct protoent pb, *r = NULL;
            int rc = getprotobynumber_r((int)strtol(argv[1], NULL, 0), &pb, buf, sizeof buf, &r);
            proto(r, rc);
        } else if (!strcmp(cmd, "protoent")) {
            setprotoent(0);
            struct protoent *p;
            while ((p = getprotoent())) proto(p, 0);
            printf("end\n");
            endprotoent();
        } else if (!strcmp(cmd, "net")) {
            struct netent nb, *r = NULL;
            int herr = -99;
            int rc = getnetbyname_r(argv[1], &nb, buf, sizeof buf, &r, &herr);
            net(r, rc, herr);
        } else if (!strcmp(cmd, "netaddr")) {
            struct netent nb, *r = NULL;
            int herr = -99;
            int rc = getnetbyaddr_r((uint32_t)strtoul(argv[1], NULL, 16), atoi(argv[2]), &nb, buf, sizeof buf, &r, &herr);
            net(r, rc, herr);
        } else if (!strcmp(cmd, "netent")) {
            setnetent(0);
            struct netent *n;
            while ((n = getnetent())) net(n, 0, 0);
            printf("end\n");
            endnetent();
        } else if (!strcmp(cmd, "ethh")) {
            struct ether_addr e; memset(&e, 0xaa, sizeof e);
            int rc = ether_hostton(argv[1], &e);
            printf("rc=%d addr=%s\n", rc, rc == 0 ? ether_ntoa(&e) : "-");
        } else if (!strcmp(cmd, "ethn")) {
            struct ether_addr e;
            struct ether_addr *p = ether_aton_r(argv[1], &e);
            char host[1100] = "-";
            int rc = p ? ether_ntohost(host, &e) : -2;
            printf("rc=%d host=%s\n", rc, host);
        } else {
            printf("?\n");
        }
        fflush(stdout);
    }
    return 0;
}
