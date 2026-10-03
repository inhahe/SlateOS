/*
 * The oracle the xz crate's fixtures were judged by: liblzma 5.2.5 itself,
 * driven as `xz -d` drives it. Built and run by generate.py:
 *
 *     gcc -O2 -I<xz-5.2>/src/liblzma/api oracle.c <build>/liblzma.a -lpthread
 *
 *   oracle xz FILE            what `xz -d` makes of FILE: "OK len fnv" or "ERR code"
 *   oracle lzma FILE          the same for an .lzma file
 *   oracle raw2 FILE          the same for raw LZMA2 with preset 6's dictionary
 *                             (`xz -d --format=raw --lzma2=preset=6`)
 *   oracle MODE-mutate FILE   one line per byte of FILE and per XOR of 01, 80
 *                             and FF: "pos xor OK len fnv" or "pos xor ERR code"
 *
 * `xz -d` decodes .xz with LZMA_CONCATENATED (several streams, with stream
 * padding); .lzma with lzma_alone_decoder and raw streams with
 * lzma_raw_decoder, and then refuses any byte after either ("Check that
 * there is no trailing garbage. This is needed for LZMA_Alone and raw
 * streams.", coder.c). Output is capped at 256 MiB, as the crate's tests cap
 * it.
 */

#include <lzma.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define CAP ((size_t)256 << 20)

enum format { XZ, LZMA, RAW2 };

static uint64_t fnv(const uint8_t *p, size_t n)
{
	uint64_t h = 0xcbf29ce484222325ULL;
	for (size_t i = 0; i < n; ++i) {
		h ^= p[i];
		h *= 0x100000001b3ULL;
	}
	return h;
}

/* Decodes `in` as `xz -d` would; returns 0 and sets *out_len/*hash, or the
 * lzma_ret that stopped it. */
static int decode(enum format fmt, const uint8_t *in, size_t n,
		size_t *out_len, uint64_t *hash)
{
	lzma_stream s = LZMA_STREAM_INIT;
	lzma_ret r;
	if (fmt == XZ) {
		r = lzma_stream_decoder(&s, UINT64_MAX, LZMA_CONCATENATED);
	} else if (fmt == LZMA) {
		r = lzma_alone_decoder(&s, UINT64_MAX);
	} else {
		lzma_options_lzma opt;
		lzma_lzma_preset(&opt, 6);
		lzma_filter filters[2] = {
			{ .id = LZMA_FILTER_LZMA2, .options = &opt },
			{ .id = LZMA_VLI_UNKNOWN, .options = NULL },
		};
		r = lzma_raw_decoder(&s, filters);
	}
	if (r != LZMA_OK)
		return (int)r;

	size_t cap = 1 << 16;
	uint8_t *buf = malloc(cap);
	size_t len = 0;
	s.next_in = in;
	s.avail_in = n;
	for (;;) {
		if (len == cap) {
			if (cap >= CAP) {
				r = LZMA_MEMLIMIT_ERROR;
				break;
			}
			cap *= 2;
			buf = realloc(buf, cap);
		}
		s.next_out = buf + len;
		s.avail_out = cap - len;
		r = lzma_code(&s, LZMA_FINISH);
		len = cap - s.avail_out;
		if (r != LZMA_OK)
			break;
	}
	if (r == LZMA_STREAM_END && fmt != XZ && s.avail_in != 0)
		r = LZMA_DATA_ERROR;
	lzma_end(&s);
	if (r == LZMA_STREAM_END) {
		*out_len = len;
		*hash = fnv(buf, len);
		free(buf);
		return 0;
	}
	free(buf);
	return (int)r;
}

static uint8_t *slurp(const char *path, size_t *n)
{
	FILE *f = fopen(path, "rb");
	if (f == NULL) {
		perror(path);
		exit(2);
	}
	fseek(f, 0, SEEK_END);
	long size = ftell(f);
	fseek(f, 0, SEEK_SET);
	uint8_t *p = malloc(size > 0 ? (size_t)size : 1);
	*n = fread(p, 1, (size_t)size, f);
	fclose(f);
	return p;
}

static void verdict(const char *prefix, enum format fmt, const uint8_t *p, size_t n)
{
	size_t len;
	uint64_t hash;
	int r = decode(fmt, p, n, &len, &hash);
	if (r == 0)
		printf("%sOK %zu %016llx\n", prefix, len, (unsigned long long)hash);
	else
		printf("%sERR %d\n", prefix, r);
}

int main(int argc, char **argv)
{
	if (argc != 3) {
		fprintf(stderr, "usage: oracle {xz|lzma|raw2}[-mutate] FILE\n");
		return 2;
	}
	const char *mode = argv[1];
	enum format fmt = strncmp(mode, "lzma", 4) == 0 ? LZMA
		: strncmp(mode, "raw2", 4) == 0 ? RAW2 : XZ;
	size_t n;
	uint8_t *p = slurp(argv[2], &n);
	if (strstr(mode, "-mutate") == NULL) {
		verdict("", fmt, p, n);
		return 0;
	}
	static const uint8_t xors[3] = { 0x01, 0x80, 0xff };
	char prefix[64];
	for (size_t pos = 0; pos < n; ++pos) {
		for (int k = 0; k < 3; ++k) {
			p[pos] ^= xors[k];
			snprintf(prefix, sizeof prefix, "%zu %02x ", pos, xors[k]);
			verdict(prefix, fmt, p, n);
			p[pos] ^= xors[k];
		}
	}
	return 0;
}
