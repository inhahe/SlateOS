/*
 * SlateOS: <crypt.h> -- libxcrypt's interface, in place of musl's header.
 *
 * glibc 2.39 has no crypt of its own: Linux distributions ship libxcrypt, and
 * its <crypt.h> is the one a C program written for glibc includes (Ubuntu's
 * libcrypt-dev, which scripts/check-libc-overlay.py's reference is read
 * from). Beyond crypt and crypt_r, which musl's header has, it declares
 * crypt_rn and crypt_ra, which hash into memory the caller gives or malloc's;
 * crypt_gensalt and its _rn and _ra forms, which make the setting a new
 * password is hashed with; crypt_checksalt; and crypt_preferred_method.
 *
 * Its struct crypt_data is 32768 bytes where musl's is 260, and crypt_rn
 * refuses less, so this header cannot be musl's with more after it: a struct
 * is declared once. crypt_r writes its result at the start of the struct --
 * `output` here, the whole of musl's -- and at most 256 bytes, its NUL
 * included, so an object compiled against musl's header still gives it room
 * enough (design-decisions 1182).
 *
 * The methods are those posix/src/crypt.rs hashes: yescrypt, scrypt, bcrypt,
 * SHA-512, SHA-256 and MD5 crypt. crypt_gensalt makes settings for them
 * alone, and refuses a prefix naming another (DES, gost-yescrypt ...) with
 * EINVAL, where libxcrypt would make one (posix/src/gensalt.rs).
 */

/* A system header, as the musl ones it extends are: warnings in it are not
 * the program's. (scripts/check-libc-overlay.py defines the macro, to hold
 * it to -Wall -Wextra itself.) */
#ifndef _SLATEOS_OVERLAY_WARNINGS
#pragma GCC system_header
#endif

#ifndef _CRYPT_H
#define _CRYPT_H 1

#include <features.h>

#ifdef __cplusplus
extern "C" {
#endif

/* The longest result a method may give, its NUL included. (Room for the
 * result, not for crypt_rn's buffer: that is sizeof (struct crypt_data).) */
#define CRYPT_OUTPUT_SIZE 384

/* A passphrase this long or longer is refused (ERANGE). */
#define CRYPT_MAX_PASSPHRASE_SIZE 512

/* The longest setting crypt_gensalt makes, its NUL included: the room
 * crypt_gensalt_rn's buffer should have. */
#define CRYPT_GENSALT_OUTPUT_SIZE 192

/* The two parts of struct crypt_data that bring it to 32768 bytes. */
#define CRYPT_DATA_RESERVED_SIZE 767
#define CRYPT_DATA_INTERNAL_SIZE 30720

/* crypt_r's memory, and crypt_rn's and crypt_ra's (which take it as bytes). */
struct crypt_data {
	char output[CRYPT_OUTPUT_SIZE];          /* where the result is written */
	char setting[CRYPT_OUTPUT_SIZE];         /* the program's: a setting */
	char input[CRYPT_MAX_PASSPHRASE_SIZE];   /* the program's: a passphrase */
	char reserved[CRYPT_DATA_RESERVED_SIZE]; /* zero, before the first call */
	char initialized;                        /* zero, before the first call */
	char internal[CRYPT_DATA_INTERNAL_SIZE]; /* the library's scratch */
};

/* The hash of a passphrase by a setting -- in static memory the next call
 * overwrites; in `data`; in `data`'s `size` bytes, at least sizeof (struct
 * crypt_data); in `*data`, from malloc, made larger when `*size` is less. On
 * failure crypt and crypt_r give a string beginning '*' that no hash is,
 * crypt_rn and crypt_ra NULL; errno says why. */
char *crypt(const char *, const char *);
char *crypt_r(const char *, const char *, struct crypt_data *__restrict);
char *crypt_rn(const char *, const char *, void *, int);
char *crypt_ra(const char *, const char *, void **, int *);

/* A setting for a new passphrase: the method `prefix` names (NULL for
 * crypt_preferred_method's), at cost `count` (0 for the method's default),
 * salted by `nrbytes` random bytes at `rbytes` (NULL for the system's own).
 * In static memory, separate from crypt's; in `output`'s `output_size`
 * bytes; or from malloc. NULL on failure, errno saying why. */
char *crypt_gensalt(const char *, unsigned long, const char *, int);
char *crypt_gensalt_rn(const char *, unsigned long, const char *, int, char *, int);
char *crypt_gensalt_ra(const char *, unsigned long, const char *, int);

/* libxcrypt's other name for crypt_gensalt_rn. (Its header makes it an
 * alias where glibc's <sys/cdefs.h> can, and this macro otherwise.) */
#define crypt_gensalt_r crypt_gensalt_rn

/* Whether a setting is one crypt takes, and for new passphrases. */
int crypt_checksalt(const char *);
#define CRYPT_SALT_OK              0 /* it is */
#define CRYPT_SALT_INVALID         1 /* no method here takes it */
#define CRYPT_SALT_METHOD_DISABLED 2 /* never answered */
#define CRYPT_SALT_METHOD_LEGACY   3 /* a method for old passphrases alone */
#define CRYPT_SALT_TOO_CHEAP       4 /* never answered */

/* The prefix crypt_gensalt uses when given none: "$y$", yescrypt. */
const char *crypt_preferred_method(void);

/* What a portable program tests before relying on it: crypt_gensalt takes
 * NULL for the prefix and for the random bytes, and the two functions above
 * are here. */
#define CRYPT_GENSALT_IMPLEMENTS_DEFAULT_PREFIX 1
#define CRYPT_GENSALT_IMPLEMENTS_AUTO_ENTROPY   1
#define CRYPT_CHECKSALT_AVAILABLE               1
#define CRYPT_PREFERRED_METHOD_AVAILABLE        1

/* The libxcrypt release whose answers these are held to
 * (posix/tools/oracle/crypt_harness.py). */
#define XCRYPT_VERSION_MAJOR 4
#define XCRYPT_VERSION_MINOR 4
#define XCRYPT_VERSION_NUM ((XCRYPT_VERSION_MAJOR << 16) | XCRYPT_VERSION_MINOR)
#define XCRYPT_VERSION_STR "4.4.36"

#ifdef __cplusplus
}
#endif

#endif
