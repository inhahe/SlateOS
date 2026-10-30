// SPDX-License-Identifier: MIT
/*
 * WPA2's keys and the frames that agree them (IEEE 802.11-2020 12.7): what
 * wpa_supplicant and hostapd compute for a network protected by a
 * passphrase (WPA2-PSK, CCMP), for the layer's WiFi task (lx_wifi.c), which
 * makes the four-way handshake as either side of it.
 *
 * - The PMK, from the passphrase and the network's name: PBKDF2 over
 *   HMAC-SHA1, 4096 rounds, 32 bytes (12.7.1.3, J.4).
 * - The PTK, from the PMK, both sides' addresses and both nonces, by the
 *   standard's PRF over HMAC-SHA1 (12.7.1.2): its KCK signs the handshake's
 *   frames, its KEK wraps the group key sent in them, its TK is the
 *   pairwise key CCMP encrypts with.
 * - EAPOL-Key frames (12.7.2), built with their MIC (HMAC-SHA1-128 by the
 *   KCK: key descriptor version 2) and read back with it checked.
 * - AES key wrap (RFC 3394), for the key data of the handshake's third
 *   frame, padded as 12.7.2 pads it.
 *
 * Linux's own SHA-1 library and AES library do the hashing and the cipher;
 * this file only composes them. Each is checked against a published vector
 * where one exists (lx_wpa_check) -- the two ends of a simulated network
 * share this code, so only a vector can show it right.
 */
#include <crypto/aes.h>
#include <crypto/sha1.h>
#include <crypto/utils.h>
#include <linux/errno.h>
#include <linux/ieee80211.h>
#include <linux/kernel.h>
#include <linux/printk.h>
#include <linux/string.h>
#include <linux/unaligned.h>

#include "lx_emul.h"

/* PBKDF2's rounds for a WPA2 passphrase. */
#define LX_WPA_PBKDF2_ROUNDS 4096

/*
 * PBKDF2-HMAC-SHA1's block `index` (from 1) of the key `key` prepared from
 * the passphrase, salted with the network's name: the XOR of the rounds'
 * HMACs, each of the one before.
 */
static void lx_wpa_pbkdf2_block(const struct hmac_sha1_key *key, const u8 *salt, size_t salt_len,
				u32 index, u8 out[SHA1_DIGEST_SIZE])
{
	struct hmac_sha1_ctx ctx;
	u8 u[SHA1_DIGEST_SIZE];
	__be32 be = cpu_to_be32(index);
	int round, i;

	hmac_sha1_init(&ctx, key);
	hmac_sha1_update(&ctx, salt, salt_len);
	hmac_sha1_update(&ctx, (const u8 *)&be, sizeof(be));
	hmac_sha1_final(&ctx, u);
	memcpy(out, u, sizeof(u));
	for (round = 1; round < LX_WPA_PBKDF2_ROUNDS; round++) {
		hmac_sha1(key, u, sizeof(u), u);
		for (i = 0; i < SHA1_DIGEST_SIZE; i++)
			out[i] ^= u[i];
	}
	memzero_explicit(u, sizeof(u));
}

void lx_wpa_pmk(const char *passphrase, const u8 *ssid, size_t ssid_len, u8 pmk[LX_WPA_PMK_LEN])
{
	struct hmac_sha1_key key;
	u8 second[SHA1_DIGEST_SIZE];

	hmac_sha1_preparekey(&key, (const u8 *)passphrase, strlen(passphrase));
	lx_wpa_pbkdf2_block(&key, ssid, ssid_len, 1, pmk);
	lx_wpa_pbkdf2_block(&key, ssid, ssid_len, 2, second);
	memcpy(pmk + SHA1_DIGEST_SIZE, second, LX_WPA_PMK_LEN - SHA1_DIGEST_SIZE);
	memzero_explicit(second, sizeof(second));
	memzero_explicit(&key, sizeof(key));
}

void lx_wpa_prf(const u8 *key, size_t key_len, const char *label, const u8 *data, size_t data_len,
		u8 *out, size_t out_len)
{
	struct hmac_sha1_key k;
	u8 block[SHA1_DIGEST_SIZE];
	static const u8 zero;
	u8 i;

	hmac_sha1_preparekey(&k, key, key_len);
	for (i = 0; out_len; i++) {
		struct hmac_sha1_ctx ctx;
		size_t n = min_t(size_t, out_len, sizeof(block));

		hmac_sha1_init(&ctx, &k);
		hmac_sha1_update(&ctx, (const u8 *)label, strlen(label));
		hmac_sha1_update(&ctx, &zero, 1);
		hmac_sha1_update(&ctx, data, data_len);
		hmac_sha1_update(&ctx, &i, 1);
		hmac_sha1_final(&ctx, block);
		memcpy(out, block, n);
		out += n;
		out_len -= n;
	}
	memzero_explicit(block, sizeof(block));
	memzero_explicit(&k, sizeof(k));
}

/* The lesser of two `len`-byte strings, compared as numbers, first; the greater second. */
static void lx_wpa_ordered(u8 *out, const u8 *a, const u8 *b, size_t len)
{
	bool a_first = memcmp(a, b, len) < 0;

	memcpy(out, a_first ? a : b, len);
	memcpy(out + len, a_first ? b : a, len);
}

void lx_wpa_ptk(const u8 pmk[LX_WPA_PMK_LEN], const u8 *aa, const u8 *spa, const u8 *anonce,
		const u8 *snonce, struct lx_wpa_ptk *ptk)
{
	u8 data[2 * ETH_ALEN + 2 * LX_WPA_NONCE_LEN];
	u8 out[sizeof(*ptk)];

	lx_wpa_ordered(data, aa, spa, ETH_ALEN);
	lx_wpa_ordered(data + 2 * ETH_ALEN, anonce, snonce, LX_WPA_NONCE_LEN);
	lx_wpa_prf(pmk, LX_WPA_PMK_LEN, "Pairwise key expansion", data, sizeof(data), out,
		   sizeof(out));
	memcpy(ptk->kck, out, sizeof(ptk->kck));
	memcpy(ptk->kek, out + sizeof(ptk->kck), sizeof(ptk->kek));
	memcpy(ptk->tk, out + sizeof(ptk->kck) + sizeof(ptk->kek), sizeof(ptk->tk));
	memzero_explicit(out, sizeof(out));
}

/* RFC 3394's initial value, which an unwrap must find again. */
static const u8 lx_wpa_wrap_iv[8] = { 0xa6, 0xa6, 0xa6, 0xa6, 0xa6, 0xa6, 0xa6, 0xa6 };

int lx_wpa_wrap(const u8 kek[LX_WPA_KEK_LEN], const u8 *plain, size_t len, u8 *out)
{
	struct crypto_aes_ctx aes;
	size_t n = len / 8, i;
	u8 b[16];
	int j;

	if (len < 16 || len % 8 || aes_expandkey(&aes, kek, LX_WPA_KEK_LEN))
		return -EINVAL;
	/* The plaintext moved before the IV goes in front of it: `out` may be `plain`. */
	memmove(out + 8, plain, len);
	memcpy(out, lx_wpa_wrap_iv, 8);
	for (j = 0; j < 6; j++) {
		for (i = 1; i <= n; i++) {
			u64 t = (u64)n * j + i;
			int k;

			memcpy(b, out, 8);
			memcpy(b + 8, out + 8 * i, 8);
			aes_encrypt(&aes, b, b);
			for (k = 7; k >= 0; k--, t >>= 8)
				b[k] ^= t & 0xff;
			memcpy(out, b, 8);
			memcpy(out + 8 * i, b + 8, 8);
		}
	}
	memzero_explicit(b, sizeof(b));
	memzero_explicit(&aes, sizeof(aes));
	return 0;
}

int lx_wpa_unwrap(const u8 kek[LX_WPA_KEK_LEN], const u8 *wrapped, size_t len, u8 *out)
{
	struct crypto_aes_ctx aes;
	size_t n = (len - 8) / 8, i;
	u8 a[8], b[16];
	int j, err = 0;

	if (len < 24 || len % 8 || aes_expandkey(&aes, kek, LX_WPA_KEK_LEN))
		return -EINVAL;
	memcpy(a, wrapped, 8);
	memmove(out, wrapped + 8, len - 8);
	for (j = 5; j >= 0; j--) {
		for (i = n; i >= 1; i--) {
			u64 t = (u64)n * j + i;
			int k;

			memcpy(b, a, 8);
			for (k = 7; k >= 0; k--, t >>= 8)
				b[k] ^= t & 0xff;
			memcpy(b + 8, out + 8 * (i - 1), 8);
			aes_decrypt(&aes, b, b);
			memcpy(a, b, 8);
			memcpy(out + 8 * (i - 1), b + 8, 8);
		}
	}
	if (crypto_memneq(a, lx_wpa_wrap_iv, sizeof(a))) {
		memzero_explicit(out, len - 8);
		err = -EBADMSG;
	}
	memzero_explicit(b, sizeof(b));
	memzero_explicit(&aes, sizeof(aes));
	return err;
}

/* Where an EAPOL-Key frame's MIC lies, and its key data. */
#define LX_WPA_MIC_AT offsetof(struct lx_wpa_eapol_key, mic)
#define LX_WPA_KEY_HDR sizeof(struct lx_wpa_eapol_key)

/* The MIC of the `len`-byte frame at `frame`, its own MIC field read as 0. */
static void lx_wpa_mic(const u8 kck[LX_WPA_KCK_LEN], const u8 *frame, size_t len,
		       u8 mic[LX_WPA_MIC_LEN])
{
	static const u8 zeros[LX_WPA_MIC_LEN];
	struct hmac_sha1_ctx ctx;
	u8 digest[SHA1_DIGEST_SIZE];

	hmac_sha1_init_usingrawkey(&ctx, kck, LX_WPA_KCK_LEN);
	hmac_sha1_update(&ctx, frame, LX_WPA_MIC_AT);
	hmac_sha1_update(&ctx, zeros, sizeof(zeros));
	hmac_sha1_update(&ctx, frame + LX_WPA_MIC_AT + LX_WPA_MIC_LEN,
			 len - LX_WPA_MIC_AT - LX_WPA_MIC_LEN);
	hmac_sha1_final(&ctx, digest);
	memcpy(mic, digest, LX_WPA_MIC_LEN);
	memzero_explicit(digest, sizeof(digest));
}

int lx_wpa_key_frame(u8 *buf, size_t room, u16 info, u64 replay, const u8 *nonce,
		     const u8 *data, size_t data_len, const u8 *kck)
{
	struct lx_wpa_eapol_key *k = (struct lx_wpa_eapol_key *)buf;
	size_t len = LX_WPA_KEY_HDR + data_len;

	if (len > room || data_len > U16_MAX)
		return -ENOSPC;
	memset(buf, 0, LX_WPA_KEY_HDR);
	k->version = 2;
	k->type = 3;
	k->length = cpu_to_be16(len - 4);
	k->descriptor = 2;
	k->info = cpu_to_be16(info);
	/* The pairwise cipher's key length in the authenticator's frames; 0 in the supplicant's. */
	k->key_length = cpu_to_be16((info & LX_WPA_INFO_PAIRWISE) && (info & LX_WPA_INFO_ACK) ?
				    LX_WPA_TK_LEN : 0);
	put_unaligned_be64(replay, k->replay);
	if (nonce)
		memcpy(k->nonce, nonce, LX_WPA_NONCE_LEN);
	k->data_length = cpu_to_be16(data_len);
	/* No key data comes as NULL: memcpy() is not to be handed it, whatever the length. */
	if (data_len)
		memcpy(k->data, data, data_len);
	if (kck)
		lx_wpa_mic(kck, buf, len, k->mic);
	return len;
}

const struct lx_wpa_eapol_key *lx_wpa_key_parse(const u8 *buf, size_t len, const u8 *kck)
{
	const struct lx_wpa_eapol_key *k = (const struct lx_wpa_eapol_key *)buf;
	u8 mic[LX_WPA_MIC_LEN];
	bool good;

	if (len < LX_WPA_KEY_HDR || k->type != 3 || k->descriptor != 2 ||
	    be16_to_cpu(k->length) + 4 > len ||
	    LX_WPA_KEY_HDR + be16_to_cpu(k->data_length) > be16_to_cpu(k->length) + 4)
		return NULL;
	if (!kck)
		return k;
	lx_wpa_mic(kck, buf, be16_to_cpu(k->length) + 4, mic);
	good = !crypto_memneq(mic, k->mic, sizeof(mic));
	memzero_explicit(mic, sizeof(mic));
	return good ? k : NULL;
}

int lx_wpa_key_data_wrap(const u8 kek[LX_WPA_KEK_LEN], const u8 *data, size_t len, u8 *out,
			 size_t room)
{
	u8 padded[LX_WPA_KEY_DATA_MAX];
	size_t n = len;
	int err;

	/* 12.7.2: under 16 bytes, or not a multiple of 8, padded with 0xdd and zeros. */
	if (n < 16 || n % 8) {
		n = max_t(size_t, 16, round_up(len + 1, 8));
	}
	if (n > sizeof(padded) || n + 8 > room)
		return -ENOSPC;
	memcpy(padded, data, len);
	if (n > len) {
		padded[len] = 0xdd;
		memset(padded + len + 1, 0, n - len - 1);
	}
	err = lx_wpa_wrap(kek, padded, n, out);
	memzero_explicit(padded, sizeof(padded));
	return err ?: (int)(n + 8);
}

/* The GTK KDE's selector (12.7.2, table 12-10): IEEE's OUI and data type 1. */
static const u8 lx_wpa_gtk_selector[] = { 0x00, 0x0f, 0xac, 0x01 };

size_t lx_wpa_gtk_kde(u8 *out, u8 keyid, const u8 *gtk, size_t gtk_len)
{
	out[0] = WLAN_EID_VENDOR_SPECIFIC;
	out[1] = sizeof(lx_wpa_gtk_selector) + 2 + gtk_len;
	memcpy(out + 2, lx_wpa_gtk_selector, sizeof(lx_wpa_gtk_selector));
	/* The key's id (bits 0-1), not for sending (bit 2 clear): pairwise keys are in use. */
	out[6] = keyid & 0x03;
	out[7] = 0;
	memcpy(out + 8, gtk, gtk_len);
	return 8 + gtk_len;
}

const u8 *lx_wpa_find_gtk(const u8 *data, size_t len, u8 *keyid, size_t *gtk_len)
{
	size_t at = 0;

	while (at + 2 <= len) {
		const u8 *e = data + at;
		size_t elen = e[1];

		/* Padding: the rest is 0xdd and zeros. */
		if (e[0] == 0xdd && elen == 0)
			break;
		if (at + 2 + elen > len)
			break;
		if (e[0] == WLAN_EID_VENDOR_SPECIFIC && elen >= 6 + 5 &&
		    !memcmp(e + 2, lx_wpa_gtk_selector, sizeof(lx_wpa_gtk_selector))) {
			*keyid = e[6] & 0x03;
			*gtk_len = elen - 6;
			return e + 8;
		}
		at += 2 + elen;
	}
	return NULL;
}

/* A check's outcome: said when wrong. */
static bool lx_wpa_same(const char *what, const u8 *got, const u8 *want, size_t len)
{
	if (!memcmp(got, want, len))
		return true;
	pr_err("lx-wpa: check failed: %s: %*phN, not %*phN\n", what, (int)min_t(size_t, len, 64),
	       got, (int)min_t(size_t, len, 64), want);
	return false;
}

int lx_wpa_check(void)
{
	/* IEEE 802.11-2020 J.4.2: passphrase "password", network "IEEE". */
	static const u8 pmk_want[LX_WPA_PMK_LEN] = {
		0xf4, 0x2c, 0x6f, 0xc5, 0x2d, 0xf0, 0xeb, 0xef, 0x9e, 0xbb, 0x4b, 0x90,
		0xb3, 0x8a, 0x5f, 0x90, 0x2e, 0x83, 0xfe, 0x1b, 0x13, 0x5a, 0x70, 0xe2,
		0x3a, 0xed, 0x76, 0x2e, 0x97, 0x10, 0xa1, 0x2e,
	};
	/* The PRF's first test case (J.3): key 0x0b x 20, "prefix", "Hi There", 64 bytes. */
	static const u8 prf_want[64] = {
		0xbc, 0xd4, 0xc6, 0x50, 0xb3, 0x0b, 0x96, 0x84, 0x95, 0x18, 0x29, 0xe0,
		0xd7, 0x5f, 0x9d, 0x54, 0xb8, 0x62, 0x17, 0x5e, 0xd9, 0xf0, 0x06, 0x06,
		0xe1, 0x7d, 0x8d, 0xa3, 0x54, 0x02, 0xff, 0xee, 0x75, 0xdf, 0x78, 0xc3,
		0xd3, 0x1e, 0x0f, 0x88, 0x9f, 0x01, 0x21, 0x20, 0xc0, 0x86, 0x2b, 0xeb,
		0x67, 0x75, 0x3e, 0x74, 0x39, 0xae, 0x24, 0x2e, 0xdb, 0x83, 0x73, 0x69,
		0x83, 0x56, 0xcf, 0x5a,
	};
	/*
	 * The PTK from that PMK for the addresses 02:00:00:00:00:00 (the access
	 * point) and 02:00:00:00:01:00, ANonce 0x00..0x1f, SNonce 0x20..0x3f:
	 * worked out apart from this code, with Python's hmac and hashlib.
	 */
	static const u8 ptk_want[sizeof(struct lx_wpa_ptk)] = {
		0x7c, 0xe0, 0x2a, 0xcf, 0x53, 0x96, 0xba, 0x89, 0x73, 0x54, 0x54, 0xd0,
		0xac, 0xb3, 0x30, 0x32, 0xd1, 0x43, 0x9f, 0x4f, 0x97, 0xb4, 0x6a, 0x95,
		0x2a, 0x81, 0xd9, 0xbc, 0x52, 0x5d, 0x32, 0xa7, 0xce, 0x4f, 0x03, 0x8d,
		0x83, 0x2d, 0xc6, 0x8f, 0x69, 0xeb, 0x30, 0x34, 0x41, 0x0a, 0x60, 0x07,
	};
	/* RFC 3394 4.1: a 128-bit key wrapped with a 128-bit KEK. */
	static const u8 kek[16] = {
		0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07,
		0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f,
	};
	static const u8 key[16] = {
		0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77,
		0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff,
	};
	static const u8 wrapped_want[24] = {
		0x1f, 0xa6, 0x8b, 0x0a, 0x81, 0x12, 0xb4, 0x47, 0xae, 0xf3, 0x4b, 0xd8,
		0xfb, 0x5a, 0x7b, 0x82, 0x9d, 0x3e, 0x86, 0x23, 0x71, 0xd2, 0xcf, 0xe5,
	};
	static const u8 aa[ETH_ALEN] = { 0x02, 0, 0, 0, 0, 0 };
	static const u8 spa[ETH_ALEN] = { 0x02, 0, 0, 0, 0x01, 0 };
	static const u8 prf_key[20] = {
		0x0b, 0x0b, 0x0b, 0x0b, 0x0b, 0x0b, 0x0b, 0x0b, 0x0b, 0x0b,
		0x0b, 0x0b, 0x0b, 0x0b, 0x0b, 0x0b, 0x0b, 0x0b, 0x0b, 0x0b,
	};
	u8 pmk[LX_WPA_PMK_LEN], prf[64], anonce[LX_WPA_NONCE_LEN], snonce[LX_WPA_NONCE_LEN];
	u8 wrapped[24], unwrapped[16], in_place[24], frame[LX_WPA_KEY_HDR + 8];
	struct lx_wpa_ptk ptk, ptk_other_way;
	bool ok = true;
	int i, len, err;

	for (i = 0; i < LX_WPA_NONCE_LEN; i++) {
		anonce[i] = i;
		snonce[i] = 0x20 + i;
	}
	lx_wpa_pmk("password", (const u8 *)"IEEE", 4, pmk);
	ok &= lx_wpa_same("the PMK of \"password\" on \"IEEE\" (802.11 J.4.2)", pmk, pmk_want,
			  sizeof(pmk));
	lx_wpa_prf(prf_key, sizeof(prf_key), "prefix", (const u8 *)"Hi There", 8, prf, sizeof(prf));
	ok &= lx_wpa_same("the PRF's first test case (802.11 J.3)", prf, prf_want, sizeof(prf));
	lx_wpa_ptk(pmk_want, aa, spa, anonce, snonce, &ptk);
	ok &= lx_wpa_same("a PTK", (const u8 *)&ptk, ptk_want, sizeof(ptk));
	/* Either side derives the same PTK, whichever it names first. */
	lx_wpa_ptk(pmk_want, spa, aa, snonce, anonce, &ptk_other_way);
	ok &= lx_wpa_same("the PTK derived from the other side", (const u8 *)&ptk_other_way,
			  ptk_want, sizeof(ptk));
	err = lx_wpa_wrap(kek, key, sizeof(key), wrapped);
	if (err)
		pr_err("lx-wpa: check failed: RFC 3394's key could not be wrapped: %d\n", err);
	ok &= !err && lx_wpa_same("RFC 3394's key wrapped", wrapped, wrapped_want, sizeof(wrapped));
	err = lx_wpa_unwrap(kek, wrapped_want, sizeof(wrapped_want), unwrapped);
	if (err)
		pr_err("lx-wpa: check failed: RFC 3394's wrapped key did not unwrap: %d\n", err);
	ok &= !err && lx_wpa_same("RFC 3394's key unwrapped", unwrapped, key, sizeof(key));
	/* Wrapped where it lies, as well as into a buffer of its own. */
	memcpy(in_place, key, sizeof(key));
	err = lx_wpa_wrap(kek, in_place, sizeof(key), in_place);
	ok &= !err && lx_wpa_same("RFC 3394's key wrapped in place", in_place, wrapped_want,
				  sizeof(wrapped_want));
	/* A group key's KDE built, and found again in key data after another element. */
	{
		u8 data[4 + 8 + LX_WPA_TK_LEN] = { WLAN_EID_RSN, 2, 0x01, 0x00 };
		size_t gtk_len = 0;
		const u8 *gtk;
		u8 keyid = 0;

		lx_wpa_gtk_kde(data + 4, 2, key, sizeof(key));
		gtk = lx_wpa_find_gtk(data, sizeof(data), &keyid, &gtk_len);
		if (!gtk || keyid != 2 || gtk_len != sizeof(key) || memcmp(gtk, key, sizeof(key))) {
			pr_err("lx-wpa: check failed: a group key's KDE built was not found again as built\n");
			ok = false;
		}
	}
	memcpy(wrapped, wrapped_want, sizeof(wrapped));
	wrapped[23] ^= 1;
	if (lx_wpa_unwrap(kek, wrapped, sizeof(wrapped), unwrapped) != -EBADMSG) {
		pr_err("lx-wpa: check failed: a wrapped key with a bit changed was unwrapped\n");
		ok = false;
	}
	/* A frame's MIC holds for the frame, and not for it with a byte changed. */
	len = lx_wpa_key_frame(frame, sizeof(frame), LX_WPA_INFO_VERSION | LX_WPA_INFO_PAIRWISE |
			       LX_WPA_INFO_MIC, 1, snonce, key, 8, ptk.kck);
	if (len < 0 || !lx_wpa_key_parse(frame, len, ptk.kck)) {
		pr_err("lx-wpa: check failed: a frame's own MIC did not hold\n");
		ok = false;
	} else {
		frame[LX_WPA_KEY_HDR] ^= 1;
		if (lx_wpa_key_parse(frame, len, ptk.kck)) {
			pr_err("lx-wpa: check failed: a frame's MIC held with its key data changed\n");
			ok = false;
		}
	}
	memzero_explicit(pmk, sizeof(pmk));
	memzero_explicit(&ptk, sizeof(ptk));
	memzero_explicit(&ptk_other_way, sizeof(ptk_other_way));
	if (!ok)
		return -EBADMSG;
	pr_info("lx-wpa: PBKDF2, the PRF and the PTK, AES key wrap and the frames' MIC: as the vectors have them\n");
	return 0;
}
