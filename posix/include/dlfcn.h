/*
 * SlateOS: what this C library's <dlfcn.h> has that musl's does not
 * declare -- see <math.h> in this directory for why this file exists.
 */

/* A system header, as the musl ones it extends are: warnings in it are not
 * the program's. (scripts/check-libc-overlay.py defines the macro, to hold
 * it to -Wall -Wextra itself.) */
#ifndef _SLATEOS_OVERLAY_WARNINGS
#pragma GCC system_header
#endif

#include_next <dlfcn.h>

#ifndef _SLATEOS_DLFCN_H
#define _SLATEOS_DLFCN_H

#include <bits/slateos-features.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Look an object's own symbols up ahead of the global ones: glibc's
 * <bits/dlfcn.h> defines it in every configuration. */
#define RTLD_DEEPBIND 0x00008

#ifdef _GNU_SOURCE
#define __NEED_size_t
#include <bits/alltypes.h>

/* A namespace, for dlmopen: there is one here, the base. */
typedef long int Lmid_t;
#define LM_ID_BASE 0
#define LM_ID_NEWLM -1

void *dlmopen(Lmid_t, const char *, int);
void *dlvsym(void *__restrict, const char *__restrict, const char *__restrict);

/* dladdr, and what else to store in *extra. */
int dladdr1(const void *, Dl_info *, void **, int);
enum {
	RTLD_DL_SYMENT = 1,
	RTLD_DL_LINKMAP = 2
};

/* dlinfo's requests -- RTLD_DI_LINKMAP, musl's, is a macro already. */
enum {
	RTLD_DI_LMID = 1,
	RTLD_DI_CONFIGADDR = 3,
	RTLD_DI_SERINFO = 4,
	RTLD_DI_SERINFOSIZE = 5,
	RTLD_DI_ORIGIN = 6,
	RTLD_DI_PROFILENAME = 7,
	RTLD_DI_PROFILEOUT = 8,
	RTLD_DI_TLS_MODID = 9,
	RTLD_DI_TLS_DATA = 10,
	RTLD_DI_PHDR = 11,
	RTLD_DI_MAX = 11
};

/* RTLD_DI_SERINFO's answer: the library search path, of which there is
 * none here. */
typedef struct {
	char *dls_name;
	unsigned int dls_flags;
} Dl_serpath;

/* glibc's own shape: where GNU C allows it, the path's entries are a
 * zero-length array in a union keeping the historic size. */
typedef struct {
	size_t dls_size;
	unsigned int dls_cnt;
#if defined(__GNUC__) && __GNUC__ >= 3
	__extension__ union {
		Dl_serpath dls_serpath[0];
		Dl_serpath __dls_serpath_pad[1];
	};
#else
	Dl_serpath dls_serpath[1];
#endif
} Dl_serinfo;

/* glibc 2.35's: the object holding an address, and its unwind index --
 * x86-64's layout, without dlfo_eh_dbase or dlfo_eh_count. The reserved
 * field is glibc's, spelling and all. */
#define DLFO_STRUCT_HAS_EH_DBASE 0
#define DLFO_STRUCT_HAS_EH_COUNT 0
#define DLFO_EH_SEGMENT_TYPE PT_GNU_EH_FRAME

struct link_map;
struct dl_find_object {
	__extension__ unsigned long long dlfo_flags;
	void *dlfo_map_start;
	void *dlfo_map_end;
	struct link_map *dlfo_link_map;
	void *dlfo_eh_frame;
	__extension__ unsigned long long __dflo_reserved[7];
};

int _dl_find_object(void *, struct dl_find_object *);
#endif

#ifdef __cplusplus
}
#endif

#endif /* _SLATEOS_DLFCN_H */
