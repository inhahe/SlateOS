/* glibc's answers for getaddrinfo and getnameinfo, in a sandbox with the
   test's /etc/hosts, /etc/services, /etc/gai.conf and a network of one
   address.  stdin: one command per line; stdout: one line per command. */
#define _GNU_SOURCE 1
#include <arpa/inet.h>
#include <errno.h>
#include <netdb.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/un.h>

static const char *arg(const char *s) {
    if (!strcmp(s, "NULL")) return NULL;
    if (!strcmp(s, "EMPTY")) return "";
    return s;
}

static void show(int rc, struct addrinfo *res) {
    if (rc) { printf("rc=%d\n", rc); return; }
    for (struct addrinfo *a = res; a; a = a->ai_next) {
        char t[64] = "?";
        int port = 0;
        unsigned scope = 0;
        if (a->ai_family == AF_INET) {
            struct sockaddr_in *s = (void *)a->ai_addr;
            inet_ntop(AF_INET, &s->sin_addr, t, sizeof t);
            port = ntohs(s->sin_port);
        } else if (a->ai_family == AF_INET6) {
            struct sockaddr_in6 *s = (void *)a->ai_addr;
            inet_ntop(AF_INET6, &s->sin6_addr, t, sizeof t);
            port = ntohs(s->sin6_port);
            scope = s->sin6_scope_id;
        }
        printf("[f=%x %d %d %d %s %d", a->ai_flags, a->ai_family, a->ai_socktype, a->ai_protocol, t, port);
        if (scope) printf(" %%%u", scope);
        if (a->ai_canonname) printf(" c=%s", a->ai_canonname);
        printf("]");
    }
    printf("\n");
}

int main(void) {
    char line[4096];
    while (fgets(line, sizeof line, stdin)) {
        line[strcspn(line, "\r\n")] = 0;
        char *argv[9] = {0};
        int argc = 0;
        for (char *t = strtok(line, " "); t && argc < 9; t = strtok(NULL, " ")) argv[argc++] = t;
        if (!argc) continue;
        if (!strcmp(argv[0], "gai")) {
            /* gai NAME SERVICE FLAGS FAMILY SOCKTYPE PROTOCOL | gai NAME SERVICE nohints */
            struct addrinfo h, *res = NULL;
            int rc;
            if (argc > 3 && !strcmp(argv[3], "nohints")) {
                rc = getaddrinfo(arg(argv[1]), arg(argv[2]), NULL, &res);
            } else {
                memset(&h, 0, sizeof h);
                h.ai_flags = (int)strtol(argv[3], NULL, 0);
                h.ai_family = atoi(argv[4]);
                h.ai_socktype = atoi(argv[5]);
                h.ai_protocol = atoi(argv[6]);
                rc = getaddrinfo(arg(argv[1]), arg(argv[2]), &h, &res);
            }
            show(rc, res);
            if (!rc) freeaddrinfo(res);
        } else if (!strcmp(argv[0], "gni")) {
            /* gni FAMILY ADDR PORT SCOPE HOSTLEN SERVLEN FLAGS */
            struct sockaddr_storage ss;
            memset(&ss, 0, sizeof ss);
            socklen_t len = 0;
            int fam = atoi(argv[1]);
            if (fam == AF_INET) {
                struct sockaddr_in *s = (void *)&ss;
                s->sin_family = AF_INET;
                inet_pton(AF_INET, argv[2], &s->sin_addr);
                s->sin_port = htons(atoi(argv[3]));
                len = sizeof *s;
            } else if (fam == AF_INET6) {
                struct sockaddr_in6 *s = (void *)&ss;
                s->sin6_family = AF_INET6;
                inet_pton(AF_INET6, argv[2], &s->sin6_addr);
                s->sin6_port = htons(atoi(argv[3]));
                s->sin6_scope_id = (unsigned)atoi(argv[4]);
                len = sizeof *s;
            } else if (fam == AF_UNIX) {
                struct sockaddr_un *s = (void *)&ss;
                s->sun_family = AF_UNIX;
                strcpy(s->sun_path, argv[2]);
                len = sizeof *s;
            } else {
                ss.ss_family = fam;
                len = 16;
            }
            char host[1100], serv[100];
            memset(host, '#', sizeof host);
            memset(serv, '#', sizeof serv);
            int hl = atoi(argv[5]), sl = atoi(argv[6]);
            int rc = getnameinfo((struct sockaddr *)&ss, len, hl ? host : NULL, hl, sl ? serv : NULL, sl,
                                 (int)strtol(argv[7], NULL, 0));
            if (rc) printf("rc=%d\n", rc);
            else printf("host=%s serv=%s\n", hl ? host : "-", sl ? serv : "-");
        } else if (!strcmp(argv[0], "err")) {
            for (int c = -110; c <= 2; c++) printf("%d:%s|", c, gai_strerror(c));
            printf("\n");
        } else {
            printf("?\n");
        }
        fflush(stdout);
    }
    return 0;
}
