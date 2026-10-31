// SPDX-License-Identifier: MIT
/*
 * A WiFi driver process's own task: what the layer asks of Linux's 802.11
 * stack (cfg80211, mac80211) for the radios the driver registered, through
 * nl80211 as wpa_supplicant or iw would ask it (lx_netlink.c).
 *
 * - "scan": the card's wireless interface -- which mac80211 makes once the
 *   driver has registered the card, after its firmware is loaded -- is
 *   brought up and scanned (NL80211_CMD_TRIGGER_SCAN), and once nl80211's
 *   "scan" group says the results are in, every network cfg80211 found is
 *   said -- its name, address, frequency and signal -- and the process
 *   ends: 0 for a scan that finished, whatever it found. Passive (no SSID
 *   asked for): the card only listens, sending nothing on a network nobody
 *   has chosen.
 * - "hwsim": first what the run relies on, at its edges: the layer's
 *   netlink (lx_genl_check), the crypto API's CCM against RFC 3610
 *   (lx_wifi_ccm_check), and WPA2's keys and frames against 802.11's and
 *   RFC 3394's vectors (lx_wpa_check). Then mac80211_hwsim's two simulated
 *   radios on one medium: the first made an open access point
 *   (NL80211_CMD_SET_INTERFACE to AP, then NL80211_CMD_START_AP, beaconing
 *   LX_WIFI_SSID on LX_WIFI_FREQ), the second scanning as above, which must
 *   find that network, from the first radio's address, on that frequency.
 *   The second joins it (NL80211_CMD_CONNECT), the layer answering its
 *   authentication and association on the first as hostapd would
 *   (lx_wifi_join), and a frame crosses the network each way, watched on
 *   the air through hwsim's monitor interface: in clear
 *   (lx_wifi_cross_watched). Then the network is made WPA2-PSK's
 *   (lx_wifi_protect), the second scans and joins it again, the layer
 *   makes the four-way handshake as both sides (lx_wifi_handshake), and a
 *   frame crosses each way protected on the air, its payload never in
 *   clear. The process ends 0 only if every step did: the whole of the
 *   stack, its ciphers and the layer's use of them, where there is no card
 *   (the boot test). The real join's network file and the access points it
 *   judges are checked first, at their edges (lx_wifi_join_check).
 * - "join=<path>": the network the file at <path> names, joined -- WPA2-PSK
 *   from the station's side, against a real access point -- and, with
 *   "serve=" too, the joined interface then served to the network stack
 *   (lx_wifi_join_network, below, has the whole of it).
 * - None of them, and given no card: the process ends once its initcalls
 *   are done. Given a card, the task does nothing.
 *
 * A run that ends otherwise well, but in which Linux code warned anywhere --
 * the driver, the 802.11 stack, the layer -- ends as warned (SLX_EXIT_WARNED),
 * which the kernel's self-test fails, as the e1000e process's does.
 *
 * And what of the kernel's key management cfg80211 asks for: a keyring
 * that trusts nothing (below).
 */
#include <crypto/aead.h>
#include <linux/completion.h>
#include <linux/delay.h>
#include <linux/scatterlist.h>
#include <linux/etherdevice.h>
#include <linux/ieee80211.h>
#include <linux/kernel_read_file.h>
#include <linux/key.h>
#include <linux/kthread.h>
#include <linux/netdevice.h>
#include <linux/random.h>
#include <linux/rtnetlink.h>
#include <linux/unaligned.h>
#include <linux/skbuff.h>
#include <linux/slab.h>
#include <linux/verification.h>
#include <linux/vmalloc.h>
#include <keys/asymmetric-type.h>
#include <net/cfg80211.h>
#include <net/genetlink.h>
#include <net/netlink.h>
#include <uapi/linux/nl80211.h>

#include "slx.h"
#include "lx_emul.h"

/*
 * The keyring cfg80211 checks a regulatory database's signature against
 * (net/wireless/reg.c: its compiled-in certificates loaded into it at
 * start-up, a database read from the firmware directory verified by them).
 * A driver process has no kernel keyring, so one that trusts nothing stands
 * in: it is made, nothing is loaded into it, and every signature checked
 * against it is rejected -- so a regulatory.db, were one staged, is
 * refused, and cfg80211 keeps to the world domain's rules, the strictest.
 */
struct key *keyring_alloc(const char *description, kuid_t uid, kgid_t gid,
			  const struct cred *cred, key_perm_t perm, unsigned long flags,
			  struct key_restriction *restrict_link, struct key *dest)
{
	struct key *keyring = kzalloc(sizeof(*keyring), GFP_KERNEL);

	return keyring ?: ERR_PTR(-ENOMEM);
}

int x509_load_certificate_list(const u8 cert_list[], const unsigned long list_size,
			       const struct key *keyring)
{
	return 0;
}

int verify_pkcs7_signature(const void *data, size_t len, const void *raw_pkcs7, size_t pkcs7_len,
			   struct key *trusted_keys, enum key_being_used_for usage,
			   int (*view_content)(void *ctx, const void *data, size_t len,
					       size_t asn1hdrlen),
			   void *ctx)
{
	return -EKEYREJECTED;
}

void key_put(struct key *key)
{
	kfree(key);
}

/* How long the radios have to register, and a scan to finish. */
#define LX_WIFI_REGISTER_MS 30000
#define LX_WIFI_SCAN_S 30

/* The network the "hwsim" run's access point beacons, and where: channel 1. */
#define LX_WIFI_SSID "slateos-hwsim"
#define LX_WIFI_FREQ 2412
#define LX_WIFI_CHANNEL 1

/*
 * How long the second radio has to join it, and a frame to cross it; the
 * association id the access point gives its one station; what the frames
 * carry.
 */
#define LX_WIFI_JOIN_S 20
#define LX_WIFI_FRAME_MS 2000
#define LX_WIFI_AID 1
#define LX_WIFI_PAYLOAD "slateos-hwsim: a frame across the network"

/*
 * The first `n` wireless interfaces, in the order mac80211 made them, once
 * there are that many -- waiting up to LX_WIFI_REGISTER_MS for the driver to
 * register its radios -- each held (dev_hold), so that one unregistered
 * while the run waits on it is not freed under it: lx_wifi_put() gives them
 * back. False, said, when there never are.
 */
static bool lx_wifi_devs(struct net_device **devs, int n)
{
	int waited, found = 0;

	for (waited = 0; waited < LX_WIFI_REGISTER_MS; waited += 100) {
		struct net_device *dev;

		found = 0;
		rtnl_lock();
		for_each_netdev(&init_net, dev)
			if (dev->ieee80211_ptr && found < n)
				devs[found++] = dev;
		if (found == n) {
			for (found = 0; found < n; found++)
				dev_hold(devs[found]);
			rtnl_unlock();
			return true;
		}
		rtnl_unlock();
		msleep(100);
	}
	pr_err("lx-wifi: %d of the %d wireless interface(s) wanted in %d s: the radios did not register\n",
	       found, n, LX_WIFI_REGISTER_MS / 1000);
	return false;
}

/* What lx_wifi_devs() held, given back. */
static void lx_wifi_put(struct net_device **devs, int n)
{
	while (n--)
		dev_put(devs[n]);
}

static int lx_wifi_open(struct net_device *dev)
{
	int err;

	rtnl_lock();
	err = dev_open(dev, NULL);
	rtnl_unlock();
	if (err)
		netdev_err(dev, "could not be brought up: %d\n", err);
	else
		netdev_info(dev, "up, %pM\n", dev->dev_addr);
	return err;
}

/*
 * nl80211's command `cmd` for `dev`'s interface, with `fill` (when not NULL)
 * adding its other attributes to the request, given `arg`: the handler's
 * answer, said -- as what `dev` would not `what` -- when it is an error.
 * The replies are not wanted.
 */
static int lx_wifi_ask(const struct genl_family *nl80211, u8 cmd, struct net_device *dev,
		       int (*fill)(struct sk_buff *req, struct net_device *dev, const void *arg),
		       const void *arg, const char *what)
{
	struct sk_buff *req = lx_genl_request(nl80211, cmd);
	struct sk_buff_head replies;
	int err;

	if (!req || nla_put_u32(req, NL80211_ATTR_IFINDEX, dev->ifindex) ||
	    (fill && fill(req, dev, arg))) {
		nlmsg_free(req);
		return -ENOMEM;
	}
	skb_queue_head_init(&replies);
	err = lx_genl_doit(nl80211, req, &replies);
	nlmsg_free(req);
	skb_queue_purge(&replies);
	if (err)
		netdev_err(dev, "nl80211 would not %s: %d\n", what, err);
	return err;
}

/* The nl80211 command a message nl80211 sent carries. */
static u8 lx_wifi_cmd(struct sk_buff *msg)
{
	return ((struct genlmsghdr *)nlmsg_data(nlmsg_hdr(msg)))->cmd;
}

/* A message's attribute `type` (nl80211 has no header of its own); NULL for none. */
static const struct nlattr *lx_wifi_attr(struct sk_buff *msg, int type)
{
	return nlmsg_find_attr(nlmsg_hdr(msg), GENL_HDRLEN, type);
}

/*
 * The jiffies left until `until`, read once -- a tick between a test and a
 * subtraction would wrap it -- and 0 once it has passed.
 */
static unsigned long lx_wifi_left(unsigned long until)
{
	long left = (long)(until - jiffies);

	return left > 0 ? left : 0;
}

/*
 * The next message nl80211's "scan" group sends that ends a scan: the
 * results in, or the scan aborted -- the command it carries; 0 for none in
 * time.
 */
static u8 lx_wifi_scan_end(const struct genl_family *nl80211)
{
	unsigned long until = jiffies + LX_WIFI_SCAN_S * HZ;

	for (;;) {
		unsigned long left = lx_wifi_left(until);
		struct sk_buff *msg;
		u8 cmd;

		if (!left)
			break;
		msg = lx_genl_event(nl80211, NL80211_MULTICAST_GROUP_SCAN, left);
		if (!msg)
			break;
		cmd = lx_wifi_cmd(msg);
		nlmsg_free(msg);
		if (cmd == NL80211_CMD_NEW_SCAN_RESULTS || cmd == NL80211_CMD_SCAN_ABORTED)
			return cmd;
	}
	return 0;
}

/* nl80211, as cfg80211 registered it; NULL, said, when it did not. */
static const struct genl_family *lx_wifi_nl80211(void)
{
	const struct genl_family *nl80211 = lx_genl_family(NL80211_GENL_NAME);

	if (!nl80211)
		pr_err("lx-wifi: nl80211 is not registered\n");
	return nl80211;
}

/* A network's name, as a scan asks for it (lx_wifi_scan_on). */
struct lx_wifi_ssid {
	const u8 *ssid;
	size_t len;
};

/* NL80211_CMD_TRIGGER_SCAN's SSIDs: the one name `arg` (a struct lx_wifi_ssid), asked for. */
static int lx_wifi_scan_ssid(struct sk_buff *req, struct net_device *dev, const void *arg)
{
	const struct lx_wifi_ssid *wanted = arg;
	struct nlattr *ssids = nla_nest_start(req, NL80211_ATTR_SCAN_SSIDS);

	if (!ssids || nla_put(req, 1, wanted->len, wanted->ssid))
		return -EMSGSIZE;
	nla_nest_end(req, ssids);
	return 0;
}

/*
 * A scan on `dev`, which is up, to its end: passive, asking for no network,
 * when `wanted` is NULL; else asking for that one by name, as a join does --
 * which an access point that hides its name answers, and one only listened
 * to never shows. 0, or why not.
 */
static int lx_wifi_scan_on(const struct genl_family *nl80211, struct net_device *dev,
			   const struct lx_wifi_ssid *wanted)
{
	int err = lx_genl_listen(nl80211, NL80211_MULTICAST_GROUP_SCAN);
	u8 end;

	if (!err)
		err = lx_wifi_ask(nl80211, NL80211_CMD_TRIGGER_SCAN, dev,
				  wanted ? lx_wifi_scan_ssid : NULL, wanted, "scan");
	if (err)
		return err;
	end = lx_wifi_scan_end(nl80211);
	if (end != NL80211_CMD_NEW_SCAN_RESULTS) {
		netdev_err(dev, "the scan %s\n", end ? "was aborted" : "did not finish in time");
		return -ETIMEDOUT;
	}
	return 0;
}

/*
 * What the networks a scan found are told against: the one wanted, if
 * any -- and whether it advertised itself protected (privacy, and an RSN
 * element).
 */
struct lx_wifi_found {
	unsigned int networks;
	const u8 *bssid;
	bool wanted;
	bool protected;
};

/*
 * One network cfg80211 has: said, its name escaped as Linux prints one;
 * and whether it is the wanted one -- LX_WIFI_SSID from `bssid`, on
 * LX_WIFI_FREQ -- and protected.
 */
static void lx_wifi_bss(struct wiphy *wiphy, struct cfg80211_bss *bss, void *data)
{
	struct lx_wifi_found *found = data;
	const u8 *ssid;

	rcu_read_lock();
	ssid = ieee80211_bss_get_ie(bss, WLAN_EID_SSID);
	pr_info("lx-wifi: \"%*pE\" %pM %u MHz %d dBm%s\n", ssid ? ssid[1] : 0,
		ssid ? ssid + 2 : NULL, bss->bssid, bss->channel->center_freq, bss->signal / 100,
		bss->capability & WLAN_CAPABILITY_PRIVACY ? ", protected" : "");
	if (found->bssid && ssid && ssid[1] == strlen(LX_WIFI_SSID) &&
	    !memcmp(ssid + 2, LX_WIFI_SSID, ssid[1]) && ether_addr_equal(bss->bssid, found->bssid) &&
	    bss->channel->center_freq == LX_WIFI_FREQ) {
		found->wanted = true;
		found->protected = (bss->capability & WLAN_CAPABILITY_PRIVACY) &&
				   ieee80211_bss_get_ie(bss, WLAN_EID_RSN);
	}
	rcu_read_unlock();
	found->networks++;
}

/* The networks `dev`'s wiphy has, said and told against `found`. */
static void lx_wifi_results(struct net_device *dev, struct lx_wifi_found *found)
{
	cfg80211_bss_iter(dev->ieee80211_ptr->wiphy, NULL, lx_wifi_bss, found);
	pr_info("lx-wifi: scan: %u network(s)\n", found->networks);
}

/*
 * The station `sta_dev`'s scan told against the network the access point
 * `ap_dev` runs: found -- its name, from its address, on its frequency --
 * and advertising itself protected exactly when `protect`. 0, or -ENOENT,
 * said.
 */
static int lx_wifi_heard(struct net_device *sta_dev, struct net_device *ap_dev, bool protect)
{
	struct lx_wifi_found found = { .bssid = ap_dev->dev_addr };

	lx_wifi_results(sta_dev, &found);
	if (found.wanted && found.protected == protect) {
		pr_info("lx-wifi: hwsim: \"%s\" found%s, beaconed by the first radio and heard by the second\n",
			LX_WIFI_SSID, protect ? ", protected" : "");
		return 0;
	}
	if (!found.wanted)
		pr_err("lx-wifi: the scan did not find \"%s\" from %pM on %u MHz\n", LX_WIFI_SSID,
		       ap_dev->dev_addr, LX_WIFI_FREQ);
	else
		pr_err("lx-wifi: the scan heard \"%s\" %s, not %s\n", LX_WIFI_SSID,
		       found.protected ? "protected" : "open", protect ? "protected" : "open");
	return -ENOENT;
}

static int lx_wifi_scan(void)
{
	const struct genl_family *nl80211 = lx_wifi_nl80211();
	struct lx_wifi_found found = {};
	struct net_device *dev;
	int err;

	if (!nl80211 || !lx_wifi_devs(&dev, 1))
		return -ENODEV;
	err = lx_wifi_open(dev);
	if (!err)
		err = lx_wifi_scan_on(nl80211, dev, NULL);
	if (!err)
		lx_wifi_results(dev, &found);
	lx_wifi_put(&dev, 1);
	return err;
}

/* The access point's interface made one (NL80211_CMD_SET_INTERFACE). */
static int lx_wifi_ap_type(struct sk_buff *req, struct net_device *dev, const void *unused)
{
	return nla_put_u32(req, NL80211_ATTR_IFTYPE, NL80211_IFTYPE_AP);
}

/*
 * A management frame of subtype `stype`, from `dev` as the access point --
 * its address the sender's and the network's -- to `da`.
 */
static void lx_wifi_mgmt_header(struct ieee80211_mgmt *m, u16 stype, const u8 *da,
				const struct net_device *dev)
{
	m->frame_control = cpu_to_le16(IEEE80211_FTYPE_MGMT | stype);
	ether_addr_copy(m->da, da);
	ether_addr_copy(m->sa, dev->dev_addr);
	ether_addr_copy(m->bssid, dev->dev_addr);
}

/* The rates the access point offers, as an element: 1, 2, 5.5 and 11 Mb/s, all basic. */
static const u8 lx_wifi_rates[] = { WLAN_EID_SUPP_RATES, 4, 0x82, 0x84, 0x8b, 0x96 };

/*
 * A WPA2-PSK network's RSN element, as the station asks for it: version 1,
 * CCMP its group and its one pairwise cipher, PSK its one key management,
 * no capabilities.
 */
static const u8 lx_wifi_rsn[] = {
	WLAN_EID_RSN, 20,
	0x01, 0x00,
	0x00, 0x0f, 0xac, 0x04,
	0x01, 0x00, 0x00, 0x0f, 0xac, 0x04,
	0x01, 0x00, 0x00, 0x0f, 0xac, 0x02,
	0x00, 0x00,
};

/*
 * The same, as the access point advertises it -- in its beacons, and in
 * the handshake's third message, which must repeat what it advertised --
 * with the capabilities many access points set (16 replay counters for
 * each of the two kinds of traffic, 0x000c): not the station's element
 * byte for byte, as a real network's is not.
 */
static const u8 lx_wifi_ap_rsn[] = {
	WLAN_EID_RSN, 20,
	0x01, 0x00,
	0x00, 0x0f, 0xac, 0x04,
	0x01, 0x00, 0x00, 0x0f, 0xac, 0x04,
	0x01, 0x00, 0x00, 0x0f, 0xac, 0x02,
	0x0c, 0x00,
};

/* The "hwsim" run's protected network's passphrase. */
#define LX_WIFI_PASSPHRASE "slateos-hwsim passphrase"

/*
 * What the network asks of a station that joins it -- open, or WPA2-PSK
 * (CCMP) -- and what nl80211 is told of it, starting it or joining it: its
 * privacy, ciphers and key management, the station's port kept closed to
 * all but the handshake's frames until it is let through.
 */
static int lx_wifi_put_wpa2(struct sk_buff *req)
{
	const u32 ccmp = WLAN_CIPHER_SUITE_CCMP, psk = WLAN_AKM_SUITE_PSK;

	return nla_put_flag(req, NL80211_ATTR_PRIVACY) ||
	       nla_put_u32(req, NL80211_ATTR_WPA_VERSIONS, NL80211_WPA_VERSION_2) ||
	       nla_put(req, NL80211_ATTR_CIPHER_SUITES_PAIRWISE, sizeof(ccmp), &ccmp) ||
	       nla_put_u32(req, NL80211_ATTR_CIPHER_SUITE_GROUP, ccmp) ||
	       nla_put(req, NL80211_ATTR_AKM_SUITES, sizeof(psk), &psk) ||
	       nla_put_flag(req, NL80211_ATTR_CONTROL_PORT);
}

/*
 * NL80211_CMD_START_AP's attributes: a network named LX_WIFI_SSID on
 * LX_WIFI_FREQ, without HT, beaconing every 100 TU -- open, or protected
 * by WPA2-PSK when `wpa2` (a bool) is true. The beacon's head is the frame
 * up to its first element and the elements cfg80211 wants there -- the
 * SSID and the rates -- its tail the channel and, protected, the RSN
 * element.
 */
static int lx_wifi_ap_params(struct sk_buff *req, struct net_device *dev, const void *wpa2)
{
	static const u8 broadcast[ETH_ALEN] = { 0xff, 0xff, 0xff, 0xff, 0xff, 0xff };
	const bool protect = wpa2 && *(const bool *)wpa2;
	const size_t ssid_len = strlen(LX_WIFI_SSID);
	u8 head[sizeof(struct ieee80211_hdr_3addr) + 12 + 2 + 32 + sizeof(lx_wifi_rates)];
	u8 tail[3 + sizeof(lx_wifi_ap_rsn)] = { WLAN_EID_DS_PARAMS, 1, LX_WIFI_CHANNEL };
	struct ieee80211_mgmt *mgmt = (struct ieee80211_mgmt *)head;
	u8 *ie = mgmt->u.beacon.variable;

	memset(head, 0, sizeof(head));
	lx_wifi_mgmt_header(mgmt, IEEE80211_STYPE_BEACON, broadcast, dev);
	mgmt->u.beacon.beacon_int = cpu_to_le16(100);
	mgmt->u.beacon.capab_info =
		cpu_to_le16(WLAN_CAPABILITY_ESS | (protect ? WLAN_CAPABILITY_PRIVACY : 0));
	*ie++ = WLAN_EID_SSID;
	*ie++ = ssid_len;
	memcpy(ie, LX_WIFI_SSID, ssid_len);
	ie += ssid_len;
	memcpy(ie, lx_wifi_rates, sizeof(lx_wifi_rates));
	ie += sizeof(lx_wifi_rates);
	if (protect)
		memcpy(tail + 3, lx_wifi_ap_rsn, sizeof(lx_wifi_ap_rsn));
	return nla_put(req, NL80211_ATTR_BEACON_HEAD, ie - head, head) ||
	       nla_put(req, NL80211_ATTR_BEACON_TAIL, protect ? sizeof(tail) : 3, tail) ||
	       nla_put_u32(req, NL80211_ATTR_BEACON_INTERVAL, 100) ||
	       nla_put_u32(req, NL80211_ATTR_DTIM_PERIOD, 1) ||
	       nla_put(req, NL80211_ATTR_SSID, ssid_len, LX_WIFI_SSID) ||
	       nla_put_u32(req, NL80211_ATTR_HIDDEN_SSID, NL80211_HIDDEN_SSID_NOT_IN_USE) ||
	       nla_put_u32(req, NL80211_ATTR_AUTH_TYPE, NL80211_AUTHTYPE_OPEN_SYSTEM) ||
	       nla_put_u32(req, NL80211_ATTR_WIPHY_FREQ, LX_WIFI_FREQ) ||
	       nla_put_u32(req, NL80211_ATTR_WIPHY_CHANNEL_TYPE, NL80211_CHAN_NO_HT) ||
	       (protect && lx_wifi_put_wpa2(req));
}

/*
 * The access point's side of a join, as hostapd keeps it: its interface,
 * whether its network is protected (WPA2-PSK), the one station it is
 * taking -- the first to authenticate -- how far that has got
 * (authenticated; added as a station; associated, and, open, let through
 * the port), its group key, and the first error met answering it. A
 * protected network's station associated with exactly its RSN element,
 * lx_wifi_rsn, or not at all.
 */
struct lx_wifi_ap {
	const struct genl_family *nl80211;
	struct net_device *dev;
	bool wpa2;
	u8 gtk[LX_WPA_TK_LEN];
	u8 sta[ETH_ALEN];
	bool authenticated;
	bool added;
	bool associated;
	int err;
};

/* A management frame for the access point to receive (NL80211_CMD_REGISTER_FRAME), by its type. */
static int lx_wifi_frame_type(struct sk_buff *req, struct net_device *dev, const void *type)
{
	/* Every frame of the type: an empty match. */
	static const u8 any[1];

	return nla_put_u16(req, NL80211_ATTR_FRAME_TYPE, *(const u16 *)type) ||
	       nla_put(req, NL80211_ATTR_FRAME_MATCH, 0, any);
}

/* The frame `len` bytes at `frame` to be sent (NL80211_CMD_FRAME). */
struct lx_wifi_tx {
	const void *frame;
	size_t len;
};

static int lx_wifi_frame(struct sk_buff *req, struct net_device *dev, const void *arg)
{
	const struct lx_wifi_tx *tx = arg;

	return nla_put(req, NL80211_ATTR_FRAME, tx->len, tx->frame);
}

/*
 * An open system authentication's request, from the station the access
 * point takes (lx_wifi_ap_message), answered -- again, should the station
 * ask again: the second frame of the two, success.
 */
static int lx_wifi_ap_auth(struct lx_wifi_ap *ap, const struct ieee80211_mgmt *req, size_t len)
{
	u8 buf[offsetof(struct ieee80211_mgmt, u.auth.variable)] = {};
	struct ieee80211_mgmt *resp = (struct ieee80211_mgmt *)buf;
	const struct lx_wifi_tx tx = { buf, sizeof(buf) };

	if (len < sizeof(buf) || le16_to_cpu(req->u.auth.auth_alg) != WLAN_AUTH_OPEN ||
	    le16_to_cpu(req->u.auth.auth_transaction) != 1) {
		netdev_err(ap->dev, "an authentication that is not an open system's first frame, from %pM\n",
			   req->sa);
		return -EPROTO;
	}
	ether_addr_copy(ap->sta, req->sa);
	lx_wifi_mgmt_header(resp, IEEE80211_STYPE_AUTH, ap->sta, ap->dev);
	resp->u.auth.auth_alg = cpu_to_le16(WLAN_AUTH_OPEN);
	resp->u.auth.auth_transaction = cpu_to_le16(2);
	resp->u.auth.status_code = cpu_to_le16(WLAN_STATUS_SUCCESS);
	ap->authenticated = true;
	return lx_wifi_ask(ap->nl80211, NL80211_CMD_FRAME, ap->dev, lx_wifi_frame, &tx,
			   "answer an authentication");
}

/* The station an association asks for, as NL80211_CMD_NEW_STATION adds it. */
struct lx_wifi_new_sta {
	const u8 *addr;
	const u8 *rates;
	u8 n_rates;
	u16 listen_interval;
	u16 capability;
};

static int lx_wifi_new_sta(struct sk_buff *req, struct net_device *dev, const void *arg)
{
	const struct lx_wifi_new_sta *sta = arg;

	return nla_put(req, NL80211_ATTR_MAC, ETH_ALEN, sta->addr) ||
	       nla_put(req, NL80211_ATTR_STA_SUPPORTED_RATES, sta->n_rates, sta->rates) ||
	       nla_put_u16(req, NL80211_ATTR_STA_LISTEN_INTERVAL, sta->listen_interval) ||
	       nla_put_u16(req, NL80211_ATTR_STA_AID, LX_WIFI_AID) ||
	       nla_put_u16(req, NL80211_ATTR_STA_CAPABILITY, sta->capability);
}

/* The station at `addr`, named (NL80211_CMD_GET_STATION). */
static int lx_wifi_mac(struct sk_buff *req, struct net_device *dev, const void *addr)
{
	return nla_put(req, NL80211_ATTR_MAC, ETH_ALEN, addr);
}

/* The station at `addr` let through the access point's port (NL80211_CMD_SET_STATION). */
static int lx_wifi_authorize(struct sk_buff *req, struct net_device *dev, const void *addr)
{
	const struct nl80211_sta_flag_update flags = {
		.mask = BIT(NL80211_STA_FLAG_AUTHORIZED),
		.set = BIT(NL80211_STA_FLAG_AUTHORIZED),
	};

	return lx_wifi_mac(req, dev, addr) ||
	       nla_put(req, NL80211_ATTR_STA_FLAGS2, sizeof(flags), &flags);
}

/*
 * An association's request, from the station the access point takes,
 * answered as hostapd answers one: the station added with the rates it
 * asked for (NL80211_CMD_NEW_STATION), the response sent -- success, its
 * association id, the access point's rates -- and, on an open network, the
 * station let through the port, there being no 802.1X to wait for. On a
 * protected one the request must ask for the network's own security (its
 * RSN element, lx_wifi_rsn), which message 2 must then repeat, and the
 * port stays closed until the handshake opens it. A request asked again --
 * the station heard no answer in time and resent it -- has the response
 * sent again; the station is added and let through once.
 */
static int lx_wifi_ap_assoc(struct lx_wifi_ap *ap, const struct ieee80211_mgmt *req, size_t len)
{
	const size_t fixed = offsetof(struct ieee80211_mgmt, u.assoc_req.variable);
	u8 buf[offsetof(struct ieee80211_mgmt, u.assoc_resp.variable) + sizeof(lx_wifi_rates)] = {};
	struct ieee80211_mgmt *resp = (struct ieee80211_mgmt *)buf;
	const struct lx_wifi_tx tx = { buf, sizeof(buf) };
	const size_t ssid_len = strlen(LX_WIFI_SSID);
	u8 all_rates[NL80211_MAX_SUPP_RATES];
	struct lx_wifi_new_sta sta;
	const u8 *ssid, *rates, *ext, *rsn;
	size_t n_rates;
	int err;

	if (!ap->authenticated) {
		netdev_err(ap->dev, "an association asked by %pM before any authentication\n", req->sa);
		return -EPROTO;
	}
	if (len < fixed) {
		netdev_err(ap->dev, "an association request of %zu bytes from %pM: short\n", len, req->sa);
		return -EPROTO;
	}
	ssid = cfg80211_find_ie(WLAN_EID_SSID, req->u.assoc_req.variable, len - fixed);
	rates = cfg80211_find_ie(WLAN_EID_SUPP_RATES, req->u.assoc_req.variable, len - fixed);
	if (!ssid || ssid[1] != ssid_len || memcmp(ssid + 2, LX_WIFI_SSID, ssid_len) || !rates ||
	    !rates[1]) {
		netdev_err(ap->dev, "an association from %pM without this network's name, or without rates\n",
			   req->sa);
		return -EPROTO;
	}
	rsn = cfg80211_find_ie(WLAN_EID_RSN, req->u.assoc_req.variable, len - fixed);
	if (ap->wpa2) {
		if (!rsn || rsn[1] + 2 != sizeof(lx_wifi_rsn) ||
		    memcmp(rsn, lx_wifi_rsn, sizeof(lx_wifi_rsn))) {
			netdev_err(ap->dev, "an association from %pM not asking for WPA2-PSK with CCMP\n",
				   req->sa);
			return -EPROTO;
		}
	} else if (rsn) {
		netdev_err(ap->dev, "an association from %pM asking for security an open network has not\n",
			   req->sa);
		return -EPROTO;
	}
	/*
	 * Its rates whole, as hostapd copies them: the Supported Rates element's
	 * eight at most, and the Extended Supported Rates element's -- OFDM's
	 * 24 Mb/s and up, which a station puts there -- after them.
	 */
	ext = cfg80211_find_ie(WLAN_EID_EXT_SUPP_RATES, req->u.assoc_req.variable, len - fixed);
	n_rates = min_t(size_t, rates[1], sizeof(all_rates));
	memcpy(all_rates, rates + 2, n_rates);
	if (ext) {
		size_t more = min_t(size_t, ext[1], sizeof(all_rates) - n_rates);

		memcpy(all_rates + n_rates, ext + 2, more);
		n_rates += more;
	}
	sta = (struct lx_wifi_new_sta){
		.addr = ap->sta,
		.rates = all_rates,
		.n_rates = n_rates,
		.listen_interval = le16_to_cpu(req->u.assoc_req.listen_interval),
		.capability = le16_to_cpu(req->u.assoc_req.capab_info),
	};
	if (!ap->added) {
		err = lx_wifi_ask(ap->nl80211, NL80211_CMD_NEW_STATION, ap->dev, lx_wifi_new_sta,
				  &sta, "add the station associating");
		if (err)
			return err;
		ap->added = true;
	}
	lx_wifi_mgmt_header(resp, IEEE80211_STYPE_ASSOC_RESP, ap->sta, ap->dev);
	resp->u.assoc_resp.capab_info =
		cpu_to_le16(WLAN_CAPABILITY_ESS | (ap->wpa2 ? WLAN_CAPABILITY_PRIVACY : 0));
	resp->u.assoc_resp.status_code = cpu_to_le16(WLAN_STATUS_SUCCESS);
	/* The two top bits set, as every association id is sent. */
	resp->u.assoc_resp.aid = cpu_to_le16(LX_WIFI_AID | BIT(14) | BIT(15));
	memcpy(resp->u.assoc_resp.variable, lx_wifi_rates, sizeof(lx_wifi_rates));
	err = lx_wifi_ask(ap->nl80211, NL80211_CMD_FRAME, ap->dev, lx_wifi_frame, &tx,
			  "answer an association");
	if (!err && !ap->associated && !ap->wpa2)
		err = lx_wifi_ask(ap->nl80211, NL80211_CMD_SET_STATION, ap->dev, lx_wifi_authorize,
				  ap->sta, "let the station through");
	if (!err)
		ap->associated = true;
	return err;
}

/*
 * A message sent to the layer's port, as the access point's hostapd: a
 * registered frame for its interface answered, anything else passed over.
 * Once a station has authenticated, it is the one the access point takes,
 * and frames from any other are passed over too. The first error is kept.
 */
static void lx_wifi_ap_message(struct lx_wifi_ap *ap, struct sk_buff *msg)
{
	const struct nlattr *ifindex = lx_wifi_attr(msg, NL80211_ATTR_IFINDEX);
	const struct nlattr *frame = lx_wifi_attr(msg, NL80211_ATTR_FRAME);
	const struct ieee80211_mgmt *m;
	size_t len;
	int err;

	if (lx_wifi_cmd(msg) != NL80211_CMD_FRAME || !ifindex || !frame ||
	    nla_get_u32(ifindex) != (u32)ap->dev->ifindex || ap->err)
		return;
	m = nla_data(frame);
	len = nla_len(frame);
	/* Each answer checks its own fixed fields: here, the header they all have. */
	if (len < offsetof(struct ieee80211_mgmt, u)) {
		netdev_err(ap->dev, "a management frame of %zu bytes: short\n", len);
		err = -EPROTO;
	} else if (ap->authenticated && !ether_addr_equal(m->sa, ap->sta))
		err = 0;
	else if (ieee80211_is_auth(m->frame_control))
		err = lx_wifi_ap_auth(ap, m, len);
	else if (ieee80211_is_assoc_req(m->frame_control))
		err = lx_wifi_ap_assoc(ap, m, len);
	else
		err = 0;
	ap->err = err;
}

/*
 * The network a station connects to: its name, the access point's address
 * and frequency, and whether it is protected.
 */
struct lx_wifi_connect {
	const u8 *ssid;
	size_t ssid_len;
	const u8 *bssid;
	u32 freq;
	bool wpa2;
};

/*
 * What the station connecting asks (NL80211_CMD_CONNECT): the network the
 * scan found -- protected, with WPA2-PSK's security asked for in its
 * association (the RSN element) and its port closed until the handshake.
 */
static int lx_wifi_connect_params(struct sk_buff *req, struct net_device *dev, const void *arg)
{
	const struct lx_wifi_connect *c = arg;

	return nla_put(req, NL80211_ATTR_SSID, c->ssid_len, c->ssid) ||
	       nla_put(req, NL80211_ATTR_MAC, ETH_ALEN, c->bssid) ||
	       nla_put_u32(req, NL80211_ATTR_WIPHY_FREQ, c->freq) ||
	       nla_put_u32(req, NL80211_ATTR_AUTH_TYPE, NL80211_AUTHTYPE_OPEN_SYSTEM) ||
	       (c->wpa2 && (lx_wifi_put_wpa2(req) ||
			    nla_put(req, NL80211_ATTR_IE, sizeof(lx_wifi_rsn), lx_wifi_rsn)));
}

/*
 * How a station's connection ended, as nl80211 says it: its status code --
 * WLAN_STATUS_SUCCESS for joined -- and, for one that timed out, which step
 * did (enum nl80211_timeout_reason).
 */
struct lx_wifi_connected {
	int status;
	bool timed_out;
	u32 timeout_reason;
};

/*
 * A message nl80211's "mlme" group sent, read into `c` when it says how a
 * connection on `dev` ended (NL80211_CMD_CONNECT, which always carries a
 * status code): whether it did.
 */
static bool lx_wifi_connect_result(struct sk_buff *msg, struct net_device *dev,
				   struct lx_wifi_connected *c)
{
	const struct nlattr *ifindex = lx_wifi_attr(msg, NL80211_ATTR_IFINDEX);
	const struct nlattr *status = lx_wifi_attr(msg, NL80211_ATTR_STATUS_CODE);
	const struct nlattr *reason = lx_wifi_attr(msg, NL80211_ATTR_TIMEOUT_REASON);

	if (lx_wifi_cmd(msg) != NL80211_CMD_CONNECT || !ifindex ||
	    nla_get_u32(ifindex) != (u32)dev->ifindex)
		return false;
	c->status = status ? nla_get_u16(status) : WLAN_STATUS_UNSPECIFIED_FAILURE;
	c->timed_out = lx_wifi_attr(msg, NL80211_ATTR_TIMED_OUT);
	c->timeout_reason = reason ? nla_get_u32(reason) : NL80211_TIMEOUT_UNSPECIFIED;
	return true;
}

/* A timed-out connection's step, as nl80211 names it. */
static const char *lx_wifi_timeout_step(u32 reason)
{
	switch (reason) {
	case NL80211_TIMEOUT_SCAN:
		return "its scan";
	case NL80211_TIMEOUT_AUTH:
		return "its authentication";
	case NL80211_TIMEOUT_ASSOC:
		return "its association";
	default:
		return "a step nl80211 did not name";
	}
}

/*
 * The bounded wait for `dev`'s link to be up as the network core sees it
 * (netif_oper_up): its carrier's coming on is acted on later, by linkwatch,
 * which brings its queueing discipline up in the same step -- before which
 * a frame sent would be dropped. Whether it came up within LX_WIFI_FRAME_MS.
 */
static bool lx_wifi_oper_up(struct net_device *dev)
{
	int waited;

	for (waited = 0; waited < LX_WIFI_FRAME_MS && !netif_oper_up(dev); waited += 10)
		msleep(10);
	return netif_oper_up(dev);
}

/* Both ends' links up (lx_wifi_oper_up), for frames to cross between them: 0, or -ENOLINK, said. */
static int lx_wifi_links_up(struct net_device *sta_dev, struct net_device *ap_dev)
{
	if (lx_wifi_oper_up(sta_dev) && lx_wifi_oper_up(ap_dev))
		return 0;
	netdev_err(sta_dev, "joined \"%s\", but the network core did not bring %s up in %d ms\n",
		   LX_WIFI_SSID, netif_oper_up(sta_dev) ? "the access point's link" : "its link",
		   LX_WIFI_FRAME_MS);
	return -ENOLINK;
}

/*
 * The access point `ap_dev` taking its stations' authentications and
 * associations (NL80211_CMD_REGISTER_FRAME) -- once for the process: nl80211
 * frees a registration only with the socket that made it, and refuses a
 * second of the same frame type (-EALREADY), so a later join on the same
 * interface relies on these.
 */
static int lx_wifi_ap_take_frames(const struct genl_family *nl80211, struct net_device *ap_dev)
{
	static const u16 auth = IEEE80211_FTYPE_MGMT | IEEE80211_STYPE_AUTH;
	static const u16 assoc = IEEE80211_FTYPE_MGMT | IEEE80211_STYPE_ASSOC_REQ;
	int err;

	err = lx_wifi_ask(nl80211, NL80211_CMD_REGISTER_FRAME, ap_dev, lx_wifi_frame_type, &auth,
			  "take authentications");
	if (!err)
		err = lx_wifi_ask(nl80211, NL80211_CMD_REGISTER_FRAME, ap_dev, lx_wifi_frame_type,
				  &assoc, "take associations");
	return err;
}

/*
 * The station `sta_dev` joining the network the access point `ap` runs --
 * which takes its frames already (lx_wifi_ap_take_frames), its state fresh
 * but for whether the network is protected: `sta_dev` told to connect, and
 * the frames `ap` is sent answered -- as hostapd answers them -- until
 * nl80211 says how the connection ended, within LX_WIFI_JOIN_S. 0 once
 * `sta_dev` is connected, its carrier on, and `ap` has it as a station --
 * on a protected network, with its port still closed to all but the
 * handshake (lx_wifi_handshake); else why not, said.
 */
static int lx_wifi_join(const struct genl_family *nl80211, struct lx_wifi_ap *ap,
			struct net_device *sta_dev)
{
	struct net_device *ap_dev = ap->dev;
	const struct lx_wifi_connect connect = {
		.ssid = (const u8 *)LX_WIFI_SSID,
		.ssid_len = strlen(LX_WIFI_SSID),
		.bssid = ap_dev->dev_addr,
		.freq = LX_WIFI_FREQ,
		.wpa2 = ap->wpa2,
	};
	unsigned long until = jiffies + LX_WIFI_JOIN_S * HZ;
	struct lx_wifi_connected c = {};
	bool ended = false;
	int err;

	err = lx_genl_listen(nl80211, NL80211_MULTICAST_GROUP_MLME);
	if (err) {
		pr_err("lx-wifi: nl80211's \"%s\" group could not be listened to: %d\n",
		       NL80211_MULTICAST_GROUP_MLME, err);
		return err;
	}
	err = lx_wifi_ask(nl80211, NL80211_CMD_CONNECT, sta_dev, lx_wifi_connect_params, &connect,
			  "connect");
	while (!err && !ended) {
		struct sk_buff *msg;

		if (!lx_genl_wait(nl80211, NL80211_MULTICAST_GROUP_MLME, lx_wifi_left(until))) {
			netdev_err(sta_dev, "did not join \"%s\" in %d s (the access point %s it)\n",
				   LX_WIFI_SSID, LX_WIFI_JOIN_S,
				   ap->associated ? "associated" : ap->authenticated ? "authenticated" :
										    "never heard from");
			return -ETIMEDOUT;
		}
		while ((msg = lx_genl_unicast())) {
			lx_wifi_ap_message(ap, msg);
			nlmsg_free(msg);
		}
		err = ap->err;
		while ((msg = lx_genl_event(nl80211, NL80211_MULTICAST_GROUP_MLME, 0))) {
			if (!ended)
				ended = lx_wifi_connect_result(msg, sta_dev, &c);
			nlmsg_free(msg);
		}
	}
	if (err)
		return err;
	if (c.status != WLAN_STATUS_SUCCESS || !ap->associated) {
		netdev_err(sta_dev, "joining \"%s\" ended with status %d%s%s, the access point %s\n",
			   LX_WIFI_SSID, c.status, c.timed_out ? ", timed out in " : "",
			   c.timed_out ? lx_wifi_timeout_step(c.timeout_reason) : "",
			   ap->associated ? "having associated it" : "not having associated it");
		return -ECONNREFUSED;
	}
	if (!netif_carrier_ok(sta_dev)) {
		netdev_err(sta_dev, "joined \"%s\", but its carrier is off\n", LX_WIFI_SSID);
		return -ENOLINK;
	}
	/* The access point has it as a station: nl80211 answers for it, -ENOENT were it not. */
	err = lx_wifi_ask(nl80211, NL80211_CMD_GET_STATION, ap_dev, lx_wifi_mac, sta_dev->dev_addr,
			  "say what it has of the station");
	if (!err)
		netdev_info(sta_dev, "joined \"%s\"%s, %pM, as station %pM\n", LX_WIFI_SSID,
			    ap->wpa2 ? " (WPA2, its port closed until the handshake)" : "",
			    ap_dev->dev_addr, sta_dev->dev_addr);
	return err;
}

/*
 * A frame crossing the network: `from` sending LX_WIFI_PAYLOAD to `to`,
 * under the local experimental ethertype, and `to` receiving it -- from
 * `from`'s address, the payload whole -- within LX_WIFI_FRAME_MS.
 */
struct lx_wifi_crossing {
	struct packet_type type;
	struct net_device *from;
	struct completion arrived;
};

static int lx_wifi_rx(struct sk_buff *skb, struct net_device *dev, struct packet_type *pt,
		      struct net_device *orig_dev)
{
	struct lx_wifi_crossing *c = container_of(pt, struct lx_wifi_crossing, type);
	const size_t len = strlen(LX_WIFI_PAYLOAD);

	/* The payload in the linear head to compare: a driver may hand it over in pages. */
	if (pskb_may_pull(skb, len) && !memcmp(skb->data, LX_WIFI_PAYLOAD, len) &&
	    ether_addr_equal(eth_hdr(skb)->h_source, c->from->dev_addr))
		complete(&c->arrived);
	kfree_skb(skb);
	return NET_RX_SUCCESS;
}

/*
 * The `len` bytes at `data` sent from `from` to the address `to`, under the
 * ethertype `proto` (host order): 0 once handed to the driver, else why
 * not, said.
 */
static int lx_wifi_send(struct net_device *from, const u8 *to, u16 proto, const void *data,
			size_t len)
{
	struct sk_buff *skb;
	int err;

	skb = alloc_skb(LL_RESERVED_SPACE(from) + len + from->needed_tailroom, GFP_KERNEL);
	if (!skb) {
		netdev_err(from, "no memory for a frame to %pM\n", to);
		return -ENOMEM;
	}
	skb_reserve(skb, LL_RESERVED_SPACE(from));
	skb_reset_network_header(skb);
	skb_put_data(skb, data, len);
	skb->dev = from;
	skb->protocol = htons(proto);
	if (dev_hard_header(skb, from, proto, to, from->dev_addr, len) < 0) {
		netdev_err(from, "a frame to %pM could not be given its header\n", to);
		kfree_skb(skb);
		return -EINVAL;
	}
	/*
	 * A drop is a positive code (NET_XMIT_DROP); congestion is no failure.
	 * Sent once, then judged: net_xmit_eval() is a macro that reads its
	 * argument twice, so the send in it would be made twice -- the second
	 * time of a buffer the first had already consumed.
	 */
	err = dev_queue_xmit(skb);
	err = net_xmit_eval(err);
	if (err > 0)
		err = -ENOBUFS;
	if (err)
		netdev_err(from, "a frame to %pM was not sent: %d\n", to, err);
	return err;
}

static int lx_wifi_cross(struct net_device *from, struct net_device *to)
{
	struct lx_wifi_crossing c = {
		.type = { .type = htons(ETH_P_802_EX1), .dev = to, .func = lx_wifi_rx },
		.from = from,
	};
	int err;

	init_completion(&c.arrived);
	dev_add_pack(&c.type);
	err = lx_wifi_send(from, to->dev_addr, ETH_P_802_EX1, LX_WIFI_PAYLOAD,
			   strlen(LX_WIFI_PAYLOAD));
	if (!err && !wait_for_completion_timeout(&c.arrived, msecs_to_jiffies(LX_WIFI_FRAME_MS))) {
		netdev_err(from, "a frame to %pM did not cross the network in %d ms\n", to->dev_addr,
			   LX_WIFI_FRAME_MS);
		err = -ETIMEDOUT;
	}
	dev_remove_pack(&c.type);
	return err;
}

/* How long the four-way handshake has, both sides' PMKs worked out first. */
#define LX_WIFI_HANDSHAKE_S 10
/* The group key's index: the access point's first, as hostapd numbers it. */
#define LX_WIFI_GTK_IDX 1

/*
 * A key installed (NL80211_CMD_NEW_KEY): CCMP's, pairwise with the peer at
 * `mac`, or the group's -- with, when `rsc` is not NULL, the packet number
 * its frames have reached, below which none is taken (a CCMP PN, 6 bytes).
 */
struct lx_wifi_key {
	const u8 *key;
	u8 idx;
	const u8 *mac;
	const u8 *rsc;
};

/* A CCMP packet number's length, as a key's RSC carries it. */
#define LX_WIFI_CCMP_PN_LEN 6

static int lx_wifi_key_params(struct sk_buff *req, struct net_device *dev, const void *arg)
{
	const struct lx_wifi_key *k = arg;

	return nla_put(req, NL80211_ATTR_KEY_DATA, LX_WPA_TK_LEN, k->key) ||
	       nla_put_u8(req, NL80211_ATTR_KEY_IDX, k->idx) ||
	       nla_put_u32(req, NL80211_ATTR_KEY_CIPHER, WLAN_CIPHER_SUITE_CCMP) ||
	       nla_put_u32(req, NL80211_ATTR_KEY_TYPE,
			   k->mac ? NL80211_KEYTYPE_PAIRWISE : NL80211_KEYTYPE_GROUP) ||
	       (k->mac && nla_put(req, NL80211_ATTR_MAC, ETH_ALEN, k->mac)) ||
	       (k->rsc && nla_put(req, NL80211_ATTR_KEY_SEQ, LX_WIFI_CCMP_PN_LEN, k->rsc));
}

/* The group key at index `idx` (a u8) made the one multicast frames go under (NL80211_CMD_SET_KEY). */
static int lx_wifi_default_key(struct sk_buff *req, struct net_device *dev, const void *idx)
{
	struct nlattr *types;

	if (nla_put_u8(req, NL80211_ATTR_KEY_IDX, *(const u8 *)idx) ||
	    nla_put_flag(req, NL80211_ATTR_KEY_DEFAULT))
		return -EMSGSIZE;
	types = nla_nest_start(req, NL80211_ATTR_KEY_DEFAULT_TYPES);
	if (!types || nla_put_flag(req, NL80211_KEY_DEFAULT_TYPE_MULTICAST))
		return -EMSGSIZE;
	nla_nest_end(req, types);
	return 0;
}

/* A reason a station leaves (NL80211_CMD_DISCONNECT), as a u16. */
static int lx_wifi_reason(struct sk_buff *req, struct net_device *dev, const void *reason)
{
	return nla_put_u16(req, NL80211_ATTR_REASON_CODE, *(const u16 *)reason);
}

/*
 * The next message nl80211's "mlme" group sends of command `cmd` for
 * `dev`, within `timeout` jiffies, others passed over: whether one came.
 */
static bool lx_wifi_mlme_event(const struct genl_family *nl80211, struct net_device *dev, u8 cmd,
			       unsigned long timeout)
{
	unsigned long until = jiffies + timeout;
	struct sk_buff *msg;

	while ((msg = lx_genl_event(nl80211, NL80211_MULTICAST_GROUP_MLME, lx_wifi_left(until)))) {
		const struct nlattr *ifindex = lx_wifi_attr(msg, NL80211_ATTR_IFINDEX);
		bool it = lx_wifi_cmd(msg) == cmd && ifindex &&
			  nla_get_u32(ifindex) == (u32)dev->ifindex;

		nlmsg_free(msg);
		if (it)
			return true;
	}
	return false;
}

/*
 * The access point's network made a protected one, its station gone: the
 * station told to leave (NL80211_CMD_DISCONNECT) and nl80211 saying it
 * has; the access point stopped -- which drops its stations -- and started
 * again with WPA2-PSK (lx_wifi_ap_params); a group key made and installed,
 * the one its multicast frames go under. `ap` is then fresh for the next
 * join, but for its network's protection and group key. 0, or why not,
 * said.
 */
static int lx_wifi_protect(const struct genl_family *nl80211, struct lx_wifi_ap *ap,
			   struct net_device *sta_dev)
{
	static const u16 leaving = WLAN_REASON_DEAUTH_LEAVING;
	static const bool wpa2 = true;
	static const u8 gtk_idx = LX_WIFI_GTK_IDX;
	struct lx_wifi_key gtk = { .key = ap->gtk, .idx = LX_WIFI_GTK_IDX };
	int err;

	err = lx_wifi_ask(nl80211, NL80211_CMD_DISCONNECT, sta_dev, lx_wifi_reason, &leaving,
			  "leave the open network");
	if (!err && !lx_wifi_mlme_event(nl80211, sta_dev, NL80211_CMD_DISCONNECT,
					LX_WIFI_JOIN_S * HZ)) {
		netdev_err(sta_dev, "nl80211 never said it had left the open network\n");
		err = -ETIMEDOUT;
	}
	if (!err)
		err = lx_wifi_ask(nl80211, NL80211_CMD_STOP_AP, ap->dev, NULL, NULL,
				  "stop the open network");
	if (!err)
		err = lx_wifi_ask(nl80211, NL80211_CMD_START_AP, ap->dev, lx_wifi_ap_params, &wpa2,
				  "start the protected network");
	*ap = (struct lx_wifi_ap){ .nl80211 = nl80211, .dev = ap->dev, .wpa2 = true };
	get_random_bytes(ap->gtk, sizeof(ap->gtk));
	if (!err)
		err = lx_wifi_ask(nl80211, NL80211_CMD_NEW_KEY, ap->dev, lx_wifi_key_params, &gtk,
				  "install its group key");
	if (!err)
		err = lx_wifi_ask(nl80211, NL80211_CMD_SET_KEY, ap->dev, lx_wifi_default_key, &gtk_idx,
				  "send multicast under its group key");
	return err;
}

/*
 * EAPOL frames as one side of the handshake receives them: every frame of
 * 802.1X's ethertype to its interface kept, in order, for the handshake to
 * take (lx_wifi_handshake), its waiter woken -- and, once a station's own
 * handshake is done, its keeper run for them (lx_wifi_keep).
 */
struct lx_wifi_eapol {
	struct packet_type type;
	struct sk_buff_head frames;
	wait_queue_head_t *wq;
	struct work_struct *keeper;
};

static int lx_wifi_eapol_rx(struct sk_buff *skb, struct net_device *dev, struct packet_type *pt,
			    struct net_device *orig_dev)
{
	struct lx_wifi_eapol *e = container_of(pt, struct lx_wifi_eapol, type);

	skb_queue_tail(&e->frames, skb);
	wake_up(e->wq);
	if (e->keeper)
		schedule_work(e->keeper);
	return NET_RX_SUCCESS;
}

/*
 * One side's part of the handshake: its PMK, the PTK, both nonces, the
 * replay counter -- the authenticator's, of the frame it last sent, and of
 * its first message 1 (`first_replay`); the supplicant's, of the last
 * whose MIC held (`replayed` once there is one: a message 1, unsigned,
 * sets none) -- the last message it handled, and whether it is done. The
 * supplicant's also: whether it has made its nonce (once for each
 * handshake), the network it joined as its scan heard it -- the access
 * point's address and the RSN element it advertised, which message 3 must
 * repeat -- and, after its first handshake, how many times it sent message
 * 4 again and renewed the group key. The authenticator's: whether its
 * station answered the group key it last sent.
 */
struct lx_wifi_side {
	u8 pmk[LX_WPA_PMK_LEN];
	struct lx_wpa_ptk ptk;
	u8 anonce[LX_WPA_NONCE_LEN];
	u8 snonce[LX_WPA_NONCE_LEN];
	bool snonce_made;
	u64 replay;
	u64 first_replay;
	bool replayed;
	int frames;
	bool done;
	u8 bssid[ETH_ALEN];
	u8 rsn[2 + 255];
	size_t rsn_len;
	unsigned int resent;
	unsigned int rekeys;
	bool group_answered;
	bool installed;
};

/* The handshake under way: the access point's join, the station, and the two sides. */
struct lx_wifi_handshaking {
	const struct genl_family *nl80211;
	struct lx_wifi_ap *ap;
	struct net_device *sta_dev;
	struct lx_wifi_side authenticator;
	struct lx_wifi_side supplicant;
};

/*
 * An EAPOL-Key frame built (lx_wpa_key_frame), 802.1X version `version`,
 * and sent from `from` to `to`: 0, or why not, said.
 */
static int lx_wifi_eapol_send(struct net_device *from, const u8 *to, u8 version, u16 info,
			      u64 replay, const u8 *nonce, const u8 *data, size_t data_len,
			      const u8 *kck)
{
	u8 buf[sizeof(struct lx_wpa_eapol_key) + LX_WPA_KEY_DATA_MAX + 8];
	int len = lx_wpa_key_frame(buf, sizeof(buf), version, info, replay, nonce, data, data_len,
				   kck);

	if (len < 0) {
		netdev_err(from, "a handshake frame to %pM could not be built: %d\n", to, len);
		return len;
	}
	return lx_wifi_send(from, to, ETH_P_PAE, buf, len);
}

/*
 * The network the station `dev` has joined, as its scan heard it, into the
 * supplicant's side `s`: the access point's address, and the RSN element
 * it advertised. 0, or -ENOENT, said, when cfg80211 has no such network or
 * it advertised none.
 */
static int lx_wifi_joined(struct net_device *dev, struct lx_wifi_side *s)
{
	struct wireless_dev *wdev = dev->ieee80211_ptr;
	struct cfg80211_bss *bss;
	const u8 *ie;

	ether_addr_copy(s->bssid, wdev->u.client.connected_addr);
	bss = cfg80211_get_bss(wdev->wiphy, NULL, s->bssid, NULL, 0, IEEE80211_BSS_TYPE_ESS,
			       IEEE80211_PRIVACY_ANY);
	if (bss) {
		rcu_read_lock();
		ie = ieee80211_bss_get_ie(bss, WLAN_EID_RSN);
		s->rsn_len = ie ? ie[1] + 2 : 0;
		if (ie)
			memcpy(s->rsn, ie, s->rsn_len);
		rcu_read_unlock();
		cfg80211_put_bss(wdev->wiphy, bss);
	}
	if (!bss || !s->rsn_len) {
		netdev_err(dev, "cfg80211 has no RSN element for %pM, the network joined\n", s->bssid);
		return -ENOENT;
	}
	return 0;
}

/*
 * The group key handshake's first message (802.11-2020 12.7.7.2), from the
 * access point the station holds a PTK with: signed by it, a replay counter
 * past any it has checked (the caller has seen to that), the new group key
 * wrapped inside -- installed from the packet number the frame gives, and
 * answered with the second message. 0, or what was wrong, said.
 */
static int lx_wifi_group_key(struct lx_wifi_handshaking *h, struct sk_buff *skb,
			     const struct lx_wpa_eapol_key *k, u16 info, u64 replay)
{
	struct lx_wifi_side *s = &h->supplicant;
	struct net_device *dev = h->sta_dev;
	size_t data_len = be16_to_cpu(k->data_length);
	u8 plain[LX_WPA_KEY_DATA_MAX];
	const u8 *gtk;
	size_t gtk_len;
	u8 keyid;
	int err;

	if (!s->done || !(info & LX_WPA_INFO_MIC) || !(info & LX_WPA_INFO_SECURE) ||
	    !(info & LX_WPA_INFO_ENCRYPTED)) {
		netdev_err(dev, "a group key frame before the handshake, or not signed, secure and encrypted (key information %#x)\n",
			   info);
		return -EPROTO;
	}
	if (!lx_wpa_key_parse(skb->data, skb->len, s->ptk.kck)) {
		netdev_err(dev, "the group key frame's MIC does not hold\n");
		return -EBADMSG;
	}
	s->replay = replay;
	s->replayed = true;
	if (data_len < 24 || data_len > sizeof(plain) + 8 ||
	    lx_wpa_unwrap(s->ptk.kek, k->data, data_len, plain)) {
		netdev_err(dev, "the group key frame's key data does not unwrap\n");
		return -EBADMSG;
	}
	gtk = lx_wpa_find_gtk(plain, data_len - 8, &keyid, &gtk_len);
	if (!gtk || gtk_len != LX_WPA_TK_LEN) {
		netdev_err(dev, "the group key frame holds no group key\n");
		memzero_explicit(plain, sizeof(plain));
		return -EPROTO;
	}
	{
		const struct lx_wifi_key group = { .key = gtk, .idx = keyid, .rsc = k->rsc };

		err = lx_wifi_ask(h->nl80211, NL80211_CMD_NEW_KEY, dev, lx_wifi_key_params, &group,
				  "install the renewed group key");
	}
	memzero_explicit(plain, sizeof(plain));
	if (!err)
		err = lx_wifi_eapol_send(dev, s->bssid, LX_WPA_EAPOL_STATION,
					 LX_WPA_INFO_VERSION | LX_WPA_INFO_MIC | LX_WPA_INFO_SECURE,
					 replay, NULL, NULL, 0, s->ptk.kck);
	if (!err) {
		s->rekeys++;
		netdev_info(dev, "the group key renewed (key %u)\n", keyid);
	}
	return err;
}

/*
 * The supplicant -- the station -- given a frame of the handshake from the
 * access point it joined (lx_wifi_joined), as wpa_supplicant takes it:
 * message 1 answered with message 2 (its nonce, made once for the
 * handshake; the PTK derived; its RSN element; signed) -- message 1 carries
 * no MIC, so nothing in it is trusted further, its replay counter not
 * kept, and one sent again is answered again; message 3 checked (signed by
 * that PTK, a replay counter past any it has checked, the access point's
 * nonce unchanged, the RSN element it advertised, the group key wrapped
 * inside), answered with message 4, and its keys installed -- the group
 * key from the packet number message 3 gives -- and its port opened.
 * After that, for the life of the association (lx_wifi_keep): a message 3
 * sent again -- its message 4 lost on the air -- answered with message 4
 * again, its keys not installed twice; a message 1 starting a new
 * handshake, which renews the pairwise key, made as the first was, with a
 * nonce of its own; and the group key handshake answered
 * (lx_wifi_group_key). 0, or what was wrong, said.
 */
static int lx_wifi_supplicant(struct lx_wifi_handshaking *h, struct sk_buff *skb)
{
	struct lx_wifi_side *s = &h->supplicant;
	struct net_device *dev = h->sta_dev;
	const u8 *bssid = s->bssid;
	const struct lx_wpa_eapol_key *k;
	u8 plain[LX_WPA_KEY_DATA_MAX];
	const u8 *gtk, *rsn;
	size_t gtk_len, data_len;
	u16 info;
	u64 replay;
	u8 keyid;
	int err;

	if (skb_linearize(skb) || !(k = lx_wpa_key_parse(skb->data, skb->len, NULL)) ||
	    !ether_addr_equal(eth_hdr(skb)->h_source, bssid)) {
		netdev_err(dev, "a handshake frame that is not one, or not the access point's\n");
		return -EPROTO;
	}
	info = be16_to_cpu(k->info);
	replay = get_unaligned_be64(k->replay);
	data_len = be16_to_cpu(k->data_length);
	if (!(info & LX_WPA_INFO_ACK) || (s->replayed && replay <= s->replay)) {
		netdev_err(dev, "a handshake frame (key information %#x, replay counter %llu) the access point would not send\n",
			   info, replay);
		return -EPROTO;
	}
	if (!(info & LX_WPA_INFO_PAIRWISE))
		return lx_wifi_group_key(h, skb, k, info, replay);
	if (!(info & LX_WPA_INFO_MIC)) {
		/*
		 * Message 1, perhaps sent again: the PTK from its nonce and the
		 * station's one -- a new nonce for a handshake begun after the
		 * last one ended, which renews the pairwise key.
		 */
		if (s->done) {
			s->snonce_made = false;
			s->done = false;
		}
		if (!s->snonce_made) {
			get_random_bytes(s->snonce, LX_WPA_NONCE_LEN);
			s->snonce_made = true;
		}
		memcpy(s->anonce, k->nonce, LX_WPA_NONCE_LEN);
		lx_wpa_ptk(s->pmk, bssid, dev->dev_addr, s->anonce, s->snonce, &s->ptk);
		s->frames = 1;
		return lx_wifi_eapol_send(dev, bssid, LX_WPA_EAPOL_STATION,
					  LX_WPA_INFO_VERSION | LX_WPA_INFO_PAIRWISE |
					  LX_WPA_INFO_MIC, replay, s->snonce, lx_wifi_rsn,
					  sizeof(lx_wifi_rsn), s->ptk.kck);
	}
	/* Message 3. */
	if ((s->frames != 1 && s->frames != 3) || !(info & LX_WPA_INFO_INSTALL) ||
	    !(info & LX_WPA_INFO_SECURE) || !(info & LX_WPA_INFO_ENCRYPTED)) {
		netdev_err(dev, "a third handshake frame out of turn, or not installing a key (key information %#x)\n",
			   info);
		return -EPROTO;
	}
	if (!lx_wpa_key_parse(skb->data, skb->len, s->ptk.kck)) {
		netdev_err(dev, "the third handshake frame's MIC does not hold\n");
		return -EBADMSG;
	}
	s->replay = replay;
	s->replayed = true;
	if (memcmp(k->nonce, s->anonce, LX_WPA_NONCE_LEN)) {
		netdev_err(dev, "the third handshake frame's nonce is not the first's\n");
		return -EPROTO;
	}
	if (s->frames == 3) {
		/* Sent again: message 4 was lost. Answered again; the keys are in. */
		err = lx_wifi_eapol_send(dev, bssid, LX_WPA_EAPOL_STATION,
					 LX_WPA_INFO_VERSION | LX_WPA_INFO_PAIRWISE |
					 LX_WPA_INFO_MIC | LX_WPA_INFO_SECURE, replay, NULL, NULL, 0,
					 s->ptk.kck);
		if (!err)
			s->resent++;
		return err;
	}
	if (data_len < 24 || data_len > sizeof(plain) + 8 ||
	    lx_wpa_unwrap(s->ptk.kek, k->data, data_len, plain)) {
		netdev_err(dev, "the third handshake frame's key data does not unwrap\n");
		return -EBADMSG;
	}
	rsn = cfg80211_find_ie(WLAN_EID_RSN, plain, data_len - 8);
	gtk = lx_wpa_find_gtk(plain, data_len - 8, &keyid, &gtk_len);
	if (!rsn || rsn[1] + 2 != s->rsn_len || memcmp(rsn, s->rsn, s->rsn_len) || !gtk ||
	    gtk_len != LX_WPA_TK_LEN) {
		netdev_err(dev, "the third handshake frame's key data has not the RSN element the access point advertised, and a group key\n");
		memzero_explicit(plain, sizeof(plain));
		return -EPROTO;
	}
	s->frames = 3;
	err = lx_wifi_eapol_send(dev, bssid, LX_WPA_EAPOL_STATION,
				 LX_WPA_INFO_VERSION | LX_WPA_INFO_PAIRWISE | LX_WPA_INFO_MIC |
				 LX_WPA_INFO_SECURE, replay, NULL, NULL, 0, s->ptk.kck);
	/* Message 4 is sent before the keys go in: it must leave unencrypted. */
	if (!err) {
		const struct lx_wifi_key ptk = { .key = s->ptk.tk, .idx = 0, .mac = bssid };
		const struct lx_wifi_key group = { .key = gtk, .idx = keyid, .rsc = k->rsc };

		err = lx_wifi_ask(h->nl80211, NL80211_CMD_NEW_KEY, dev, lx_wifi_key_params, &ptk,
				  "install its pairwise key");
		if (!err)
			err = lx_wifi_ask(h->nl80211, NL80211_CMD_NEW_KEY, dev, lx_wifi_key_params,
					  &group, "install the group key");
		if (!err)
			err = lx_wifi_ask(h->nl80211, NL80211_CMD_SET_STATION, dev, lx_wifi_authorize,
					  bssid, "open its port");
	}
	memzero_explicit(plain, sizeof(plain));
	s->done = !err;
	return err;
}

/*
 * The authenticator's message 3 to its station, sent anew: the replay
 * counter past the last, its nonce, the network's RSN element and the
 * group key wrapped by the KEK, signed. 0, or why not, said.
 */
static int lx_wifi_ap_message_3(struct lx_wifi_handshaking *h)
{
	struct lx_wifi_side *a = &h->authenticator;
	struct lx_wifi_ap *ap = h->ap;
	u8 plain[sizeof(lx_wifi_ap_rsn) + 8 + LX_WPA_TK_LEN];
	u8 wrapped[LX_WPA_KEY_DATA_MAX + 8];
	int len;

	/* The RSN element the access point advertises, then the group key's KDE. */
	memcpy(plain, lx_wifi_ap_rsn, sizeof(lx_wifi_ap_rsn));
	lx_wpa_gtk_kde(plain + sizeof(lx_wifi_ap_rsn), LX_WIFI_GTK_IDX, ap->gtk, LX_WPA_TK_LEN);
	len = lx_wpa_key_data_wrap(a->ptk.kek, plain, sizeof(plain), wrapped, sizeof(wrapped));
	memzero_explicit(plain, sizeof(plain));
	if (len < 0) {
		netdev_err(ap->dev, "the group key could not be wrapped: %d\n", len);
		return len;
	}
	a->replay++;
	a->frames = 3;
	return lx_wifi_eapol_send(ap->dev, ap->sta, LX_WPA_EAPOL_AP,
				  LX_WPA_INFO_VERSION | LX_WPA_INFO_PAIRWISE | LX_WPA_INFO_INSTALL |
				  LX_WPA_INFO_ACK | LX_WPA_INFO_MIC | LX_WPA_INFO_SECURE |
				  LX_WPA_INFO_ENCRYPTED, a->replay, a->anonce, wrapped, len,
				  a->ptk.kck);
}

/*
 * The group key handshake's first message from the authenticator to its
 * station (802.11-2020 12.7.7.2): the access point's group key -- already
 * renewed -- wrapped by the KEK, the replay counter past the last, signed.
 * 0, or why not, said.
 */
static int lx_wifi_ap_group_key(struct lx_wifi_handshaking *h)
{
	struct lx_wifi_side *a = &h->authenticator;
	struct lx_wifi_ap *ap = h->ap;
	u8 plain[8 + LX_WPA_TK_LEN];
	u8 wrapped[LX_WPA_KEY_DATA_MAX + 8];
	int len;

	lx_wpa_gtk_kde(plain, LX_WIFI_GTK_IDX, ap->gtk, LX_WPA_TK_LEN);
	len = lx_wpa_key_data_wrap(a->ptk.kek, plain, sizeof(plain), wrapped, sizeof(wrapped));
	memzero_explicit(plain, sizeof(plain));
	if (len < 0) {
		netdev_err(ap->dev, "the renewed group key could not be wrapped: %d\n", len);
		return len;
	}
	a->replay++;
	a->group_answered = false;
	return lx_wifi_eapol_send(ap->dev, ap->sta, LX_WPA_EAPOL_AP,
				  LX_WPA_INFO_VERSION | LX_WPA_INFO_ACK | LX_WPA_INFO_MIC |
				  LX_WPA_INFO_SECURE | LX_WPA_INFO_ENCRYPTED, a->replay, NULL,
				  wrapped, len, a->ptk.kck);
}

/*
 * The authenticator -- the access point -- given a frame of the handshake
 * from its station, as hostapd takes it: message 2 checked (the replay
 * counter of message 1, signed by the PTK its nonce gives, the RSN
 * element the station associated with) and answered with message 3
 * (lx_wifi_ap_message_3); message 4 checked, the station's key installed
 * and its port opened; and the group key handshake's second message
 * checked, answering the last group key sent (lx_wifi_ap_group_key). 0, or
 * what was wrong, said.
 */
static int lx_wifi_authenticator(struct lx_wifi_handshaking *h, struct sk_buff *skb)
{
	struct lx_wifi_side *a = &h->authenticator;
	struct lx_wifi_ap *ap = h->ap;
	const struct lx_wpa_eapol_key *k;
	u64 replay;
	u16 info;
	int err;

	if (skb_linearize(skb) || !(k = lx_wpa_key_parse(skb->data, skb->len, NULL)) ||
	    !ether_addr_equal(eth_hdr(skb)->h_source, ap->sta)) {
		netdev_err(ap->dev, "a handshake frame that is not one, or not the station's\n");
		return -EPROTO;
	}
	info = be16_to_cpu(k->info);
	replay = get_unaligned_be64(k->replay);
	if ((info & LX_WPA_INFO_ACK) || !(info & LX_WPA_INFO_MIC)) {
		netdev_err(ap->dev, "a handshake frame (key information %#x) the station would not send\n",
			   info);
		return -EPROTO;
	}
	if (!(info & LX_WPA_INFO_PAIRWISE)) {
		/* The group key handshake's message 2, answering the last group key sent. */
		if (!a->done || !(info & LX_WPA_INFO_SECURE) || replay != a->replay ||
		    !lx_wpa_key_parse(skb->data, skb->len, a->ptk.kck)) {
			netdev_err(ap->dev, "a group key answer out of turn, or its MIC not holding (replay counter %llu)\n",
				   replay);
			return -EPROTO;
		}
		a->group_answered = true;
		return 0;
	}
	if (!(info & LX_WPA_INFO_SECURE)) {
		/* Message 2, answering one of the message 1s sent; once message 3 is out, a later answer is passed over. */
		if (a->frames == 3 && replay >= a->first_replay && replay < a->replay)
			return 0;
		if (a->frames != 1 || replay < a->first_replay || replay > a->replay) {
			netdev_err(ap->dev, "a second handshake frame out of turn, or answering no first one (replay counter %llu)\n",
				   replay);
			return -EPROTO;
		}
		memcpy(a->snonce, k->nonce, LX_WPA_NONCE_LEN);
		lx_wpa_ptk(a->pmk, ap->dev->dev_addr, ap->sta, a->anonce, a->snonce, &a->ptk);
		if (!lx_wpa_key_parse(skb->data, skb->len, a->ptk.kck)) {
			netdev_err(ap->dev, "the second handshake frame's MIC does not hold: the passphrases differ\n");
			return -EBADMSG;
		}
		/* What the station associated with: the association was refused unless it was this. */
		if (be16_to_cpu(k->data_length) != sizeof(lx_wifi_rsn) ||
		    memcmp(k->data, lx_wifi_rsn, sizeof(lx_wifi_rsn))) {
			netdev_err(ap->dev, "the second handshake frame's RSN element is not the one the station associated with\n");
			return -EPROTO;
		}
		return lx_wifi_ap_message_3(h);
	}
	/* Message 4, answering message 3. */
	if (a->frames != 3 || replay != a->replay) {
		netdev_err(ap->dev, "a fourth handshake frame out of turn, or not answering the third (replay counter %llu)\n",
			   replay);
		return -EPROTO;
	}
	if (!lx_wpa_key_parse(skb->data, skb->len, a->ptk.kck)) {
		netdev_err(ap->dev, "the fourth handshake frame's MIC does not hold\n");
		return -EBADMSG;
	}
	/*
	 * Answering message 3 sent again, the keys are in already: installed
	 * again they would start their packet numbers over, and the station
	 * would take every frame after as a replay -- hostapd's guard too.
	 */
	if (a->installed) {
		a->done = true;
		return 0;
	}
	{
		const struct lx_wifi_key ptk = { .key = a->ptk.tk, .idx = 0, .mac = ap->sta };

		err = lx_wifi_ask(h->nl80211, NL80211_CMD_NEW_KEY, ap->dev, lx_wifi_key_params, &ptk,
				  "install the station's pairwise key");
	}
	if (!err)
		err = lx_wifi_ask(h->nl80211, NL80211_CMD_SET_STATION, ap->dev, lx_wifi_authorize,
				  ap->sta, "open the station's port");
	a->done = !err;
	a->installed = !err;
	return err;
}

/*
 * A station's side of the handshake, and where its frames come, kept past
 * its first handshake for the life of its association: its keeper
 * (lx_wifi_keep) then answers what the access point sends -- message 3
 * again, a new handshake, the group key's (lx_wifi_supplicant).
 */
struct lx_wifi_station {
	wait_queue_head_t wq;
	struct lx_wifi_eapol eapol;
	struct lx_wifi_handshaking h;
	struct work_struct keep;
};

static void lx_wifi_keep(struct work_struct *work)
{
	struct lx_wifi_station *st = container_of(work, struct lx_wifi_station, keep);
	struct sk_buff *skb;
	int err;

	while ((skb = skb_dequeue(&st->eapol.frames))) {
		err = lx_wifi_supplicant(&st->h, skb);
		if (err)
			netdev_err(st->h.sta_dev, "a key frame after the join went unanswered: %d\n",
				   err);
		kfree_skb(skb);
	}
}

/* A station on `dev`, its handshake's frames taken from now on; NULL for no memory. */
static struct lx_wifi_station *lx_wifi_station_new(const struct genl_family *nl80211,
						   struct net_device *dev)
{
	struct lx_wifi_station *st = kzalloc(sizeof(*st), GFP_KERNEL);

	if (!st)
		return NULL;
	init_waitqueue_head(&st->wq);
	st->eapol.type.type = htons(ETH_P_PAE);
	st->eapol.type.dev = dev;
	st->eapol.type.func = lx_wifi_eapol_rx;
	st->eapol.wq = &st->wq;
	skb_queue_head_init(&st->eapol.frames);
	INIT_WORK(&st->keep, lx_wifi_keep);
	st->h.nl80211 = nl80211;
	st->h.sta_dev = dev;
	dev_add_pack(&st->eapol.type);
	return st;
}

/* Its first handshake made: its keeper answers the frames that come from now on, and any come already. */
static void lx_wifi_station_keep(struct lx_wifi_station *st)
{
	st->eapol.keeper = &st->keep;
	if (!skb_queue_empty(&st->eapol.frames))
		schedule_work(&st->keep);
}

/* The station gone: its frames no longer taken, its keeper's last run waited for, its keys wiped. */
static void lx_wifi_station_free(struct lx_wifi_station *st)
{
	dev_remove_pack(&st->eapol.type);
	st->eapol.keeper = NULL;
	cancel_work_sync(&st->keep);
	skb_queue_purge(&st->eapol.frames);
	kfree_sensitive(st);
}

/*
 * The access point's frames taken (lx_wifi_authenticator) until `*done`,
 * within LX_WIFI_HANDSHAKE_S: 0, or why not, said -- `what` what was
 * awaited.
 */
static int lx_wifi_ap_take(struct lx_wifi_handshaking *h, struct lx_wifi_eapol *at_ap,
			   wait_queue_head_t *wq, const bool *done, const char *what)
{
	unsigned long until = jiffies + LX_WIFI_HANDSHAKE_S * HZ;
	struct sk_buff *skb;
	int err = 0;

	while (!err && !*done) {
		if (!wait_event_timeout(*wq, !skb_queue_empty(&at_ap->frames), lx_wifi_left(until))) {
			netdev_err(h->ap->dev, "%s did not come in %d s\n", what, LX_WIFI_HANDSHAKE_S);
			return -ETIMEDOUT;
		}
		while (!err && (skb = skb_dequeue(&at_ap->frames))) {
			err = lx_wifi_authenticator(h, skb);
			kfree_skb(skb);
		}
	}
	return err;
}

/*
 * The station's keeper at work (lx_wifi_keep), the access point driving
 * it once their handshake is made: message 3 sent again -- as when message
 * 4 is lost on the air -- which the station must answer again, its keys
 * not installed twice; then the access point's group key renewed and sent
 * (802.11-2020 12.7.7), which the station must install and answer. 0, or
 * what was wrong, said.
 */
static int lx_wifi_kept_check(struct lx_wifi_station *st, struct lx_wifi_eapol *at_ap,
			      wait_queue_head_t *wq)
{
	static const u8 gtk_idx = LX_WIFI_GTK_IDX;
	struct lx_wifi_handshaking *h = &st->h;
	struct lx_wifi_side *a = &h->authenticator;
	struct lx_wifi_ap *ap = h->ap;
	struct lx_wifi_key gtk = { .key = ap->gtk, .idx = LX_WIFI_GTK_IDX };
	int err;

	lx_wifi_station_keep(st);
	a->done = false;
	err = lx_wifi_ap_message_3(h);
	if (!err)
		err = lx_wifi_ap_take(h, at_ap, wq, &a->done, "message 4, answering message 3 sent again,");
	if (!err && h->supplicant.resent != 1) {
		netdev_err(h->sta_dev, "message 3 sent again was answered %u time(s) again, not once\n",
			   h->supplicant.resent);
		err = -EPROTO;
	}
	if (!err) {
		get_random_bytes(ap->gtk, sizeof(ap->gtk));
		err = lx_wifi_ask(h->nl80211, NL80211_CMD_NEW_KEY, ap->dev, lx_wifi_key_params, &gtk,
				  "install its renewed group key");
	}
	if (!err)
		err = lx_wifi_ask(h->nl80211, NL80211_CMD_SET_KEY, ap->dev, lx_wifi_default_key, &gtk_idx,
				  "send multicast under its renewed group key");
	if (!err)
		err = lx_wifi_ap_group_key(h);
	if (!err)
		err = lx_wifi_ap_take(h, at_ap, wq, &a->group_answered, "the renewed group key's answer");
	if (!err && h->supplicant.rekeys != 1) {
		netdev_err(h->sta_dev, "the group key renewed was taken %u time(s), not once\n",
			   h->supplicant.rekeys);
		err = -EPROTO;
	}
	if (!err)
		netdev_info(h->sta_dev, "after the handshake: message 3 sent again answered again, the keys not installed twice; the group key renewed, installed and answered\n");
	return err;
}

/*
 * The four-way handshake between the access point `ap`, whose station
 * `sta_dev` has just joined its protected network, and that station --
 * the layer each side, as hostapd and wpa_supplicant are: the network the
 * station joined learnt from its own join (lx_wifi_joined), the PMK worked
 * out from the passphrase, message 1 sent, and each side's frames
 * taken as they come (lx_wifi_supplicant, lx_wifi_authenticator) until
 * both have installed their keys and opened their ports, within
 * LX_WIFI_HANDSHAKE_S; then the station kept for its association, as a
 * real join keeps it, and its keeper checked (lx_wifi_kept_check). 0, or
 * why not, said. Every key is wiped after.
 */
static int lx_wifi_handshake(const struct genl_family *nl80211, struct lx_wifi_ap *ap,
			     struct net_device *sta_dev)
{
	DECLARE_WAIT_QUEUE_HEAD_ONSTACK(wq);
	struct lx_wifi_eapol at_ap = {
		.type = { .type = htons(ETH_P_PAE), .dev = ap->dev, .func = lx_wifi_eapol_rx },
		.wq = &wq,
	};
	struct lx_wifi_station *st = lx_wifi_station_new(nl80211, sta_dev);
	struct lx_wifi_handshaking *h;
	unsigned long until;
	int err;

	if (!st)
		return -ENOMEM;
	h = &st->h;
	h->ap = ap;
	/* One waiter for both sides' frames. */
	st->eapol.wq = &wq;
	/* The supplicant knows the network from its own join and scan, not from the access point. */
	err = lx_wifi_joined(sta_dev, &h->supplicant);
	if (err) {
		lx_wifi_station_free(st);
		return err;
	}
	/* The PMK worked out once: both sides hold the same one, from the one passphrase. */
	lx_wpa_pmk(LX_WIFI_PASSPHRASE, (const u8 *)LX_WIFI_SSID, strlen(LX_WIFI_SSID),
		   h->authenticator.pmk);
	memcpy(h->supplicant.pmk, h->authenticator.pmk, LX_WPA_PMK_LEN);
	skb_queue_head_init(&at_ap.frames);
	dev_add_pack(&at_ap.type);
	/*
	 * Message 1 -- the access point's nonce, no MIC -- and again, its
	 * replay counter the next, as hostapd sends it again when message 2 is
	 * slow: the supplicant must answer both without losing the handshake.
	 */
	get_random_bytes(h->authenticator.anonce, LX_WPA_NONCE_LEN);
	h->authenticator.first_replay = 1;
	h->authenticator.frames = 1;
	for (h->authenticator.replay = 1; !err && h->authenticator.replay <= 2;
	     h->authenticator.replay++)
		err = lx_wifi_eapol_send(ap->dev, ap->sta, LX_WPA_EAPOL_AP,
					 LX_WPA_INFO_VERSION | LX_WPA_INFO_PAIRWISE | LX_WPA_INFO_ACK,
					 h->authenticator.replay, h->authenticator.anonce, NULL, 0,
					 NULL);
	h->authenticator.replay--;
	until = jiffies + LX_WIFI_HANDSHAKE_S * HZ;
	while (!err && !(h->authenticator.done && h->supplicant.done)) {
		struct sk_buff *skb;

		if (!wait_event_timeout(wq, !skb_queue_empty(&at_ap.frames) ||
					    !skb_queue_empty(&st->eapol.frames),
					lx_wifi_left(until))) {
			netdev_err(sta_dev, "the handshake did not end in %d s: the station last handled message %d, the access point last sent message %d\n",
				   LX_WIFI_HANDSHAKE_S, h->supplicant.frames,
				   h->authenticator.frames);
			err = -ETIMEDOUT;
			break;
		}
		while (!err && (skb = skb_dequeue(&st->eapol.frames))) {
			err = lx_wifi_supplicant(h, skb);
			kfree_skb(skb);
		}
		while (!err && (skb = skb_dequeue(&at_ap.frames))) {
			err = lx_wifi_authenticator(h, skb);
			kfree_skb(skb);
		}
	}
	if (!err)
		netdev_info(sta_dev, "the four-way handshake made: its keys installed, its port open\n");
	if (!err)
		err = lx_wifi_kept_check(st, &at_ap, &wq);
	dev_remove_pack(&at_ap.type);
	skb_queue_purge(&at_ap.frames);
	lx_wifi_station_free(st);
	return err;
}

/*
 * The air between the radios, as hwsim's monitor interface shows every
 * frame sent on it: of the data frames between the addresses `a` and
 * `b`, how many, how many protected, and how many with LX_WIFI_PAYLOAD in
 * clear.
 */
struct lx_wifi_air {
	struct packet_type type;
	const u8 *a;
	const u8 *b;
	unsigned int frames;
	unsigned int protected;
	unsigned int clear;
};

/* Whether the `len` bytes at `buf` hold the string `s`. */
static bool lx_wifi_holds(const u8 *buf, size_t len, const char *s)
{
	size_t n = strlen(s), at;

	for (at = 0; at + n <= len; at++)
		if (!memcmp(buf + at, s, n))
			return true;
	return false;
}

static int lx_wifi_air_rx(struct sk_buff *skb, struct net_device *dev, struct packet_type *pt,
			  struct net_device *orig_dev)
{
	struct lx_wifi_air *air = container_of(pt, struct lx_wifi_air, type);
	const struct ieee80211_hdr *hdr;
	unsigned int rt;

	/*
	 * A radiotap header first, its length at its third byte; then the
	 * frame. hwsim's copies are whole in their head; one that is not goes
	 * uncounted, which the verdict then says.
	 */
	if (skb_is_nonlinear(skb) || skb->len < 4)
		goto out;
	rt = get_unaligned_le16(skb->data + 2);
	if (skb->len < rt + 24)
		goto out;
	hdr = (const struct ieee80211_hdr *)(skb->data + rt);
	if (!ieee80211_is_data_present(hdr->frame_control) ||
	    !((ether_addr_equal_unaligned(hdr->addr1, air->a) &&
	       ether_addr_equal_unaligned(hdr->addr2, air->b)) ||
	      (ether_addr_equal_unaligned(hdr->addr1, air->b) &&
	       ether_addr_equal_unaligned(hdr->addr2, air->a))))
		goto out;
	air->frames++;
	if (ieee80211_has_protected(hdr->frame_control))
		air->protected++;
	if (lx_wifi_holds(skb->data + rt, skb->len - rt, LX_WIFI_PAYLOAD))
		air->clear++;
out:
	kfree_skb(skb);
	return NET_RX_SUCCESS;
}

/*
 * A frame crossing the network each way between `a` and `b` (lx_wifi_cross),
 * watched on the air (hwsim's monitor interface, "hwsim0"): both seen
 * there, and -- `protect` -- both protected, their payload never in clear;
 * or -- open -- both in clear, which shows the watch sees a payload when
 * there is one to see. 0, or what was wrong, said.
 */
static int lx_wifi_cross_watched(struct net_device *a, struct net_device *b, bool protect)
{
	struct net_device *mon = dev_get_by_name(&init_net, "hwsim0");
	struct lx_wifi_air air = { .a = a->dev_addr, .b = b->dev_addr };
	int waited, err;

	if (!mon) {
		pr_err("lx-wifi: hwsim's monitor interface (hwsim0) is not there to watch the air\n");
		return -ENODEV;
	}
	air.type = (struct packet_type){ .type = htons(ETH_P_ALL), .dev = mon, .func = lx_wifi_air_rx };
	err = lx_wifi_open(mon);
	if (!err) {
		dev_add_pack(&air.type);
		err = lx_wifi_cross(a, b);
		if (!err)
			err = lx_wifi_cross(b, a);
		/* The monitor's copies come by the receive softirq, after the frames themselves. */
		for (waited = 0; !err && air.frames < 2 && waited < LX_WIFI_FRAME_MS; waited += 10)
			msleep(10);
		dev_remove_pack(&air.type);
		/* Down again: up, hwsim copies every frame on the medium to it -- each beacon too. */
		rtnl_lock();
		dev_close(mon);
		rtnl_unlock();
	}
	dev_put(mon);
	if (err)
		return err;
	if (air.frames < 2 || (protect ? air.protected != air.frames || air.clear :
					 air.protected || air.clear != air.frames)) {
		pr_err("lx-wifi: on the air, %u data frame(s) between the radios, %u protected, %u with the payload in clear: not as a%s network's\n",
		       air.frames, air.protected, air.clear, protect ? " protected" : "n open");
		return -EPROTO;
	}
	pr_info("lx-wifi: on the air, %u data frame(s) between the radios, %s\n", air.frames,
		protect ? "every one protected, its payload never in clear" :
			  "every one in clear, as an open network's are");
	return 0;
}

/* RFC 3610's packet vector #1: AES-CCM, an 8-byte tag, a 2-byte length field. */
static const u8 lx_ccm_key[16] = {
	0xc0, 0xc1, 0xc2, 0xc3, 0xc4, 0xc5, 0xc6, 0xc7,
	0xc8, 0xc9, 0xca, 0xcb, 0xcc, 0xcd, 0xce, 0xcf,
};
static const u8 lx_ccm_nonce[13] = {
	0x00, 0x00, 0x00, 0x03, 0x02, 0x01, 0x00, 0xa0, 0xa1, 0xa2, 0xa3, 0xa4, 0xa5,
};
/* The header the tag covers, and the payload it encrypts, back to back. */
static const u8 lx_ccm_plain[8 + 23] = {
	0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07,
	0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15,
	0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e,
};
/* The header, then the payload encrypted and the tag. */
static const u8 lx_ccm_sealed[8 + 23 + 8] = {
	0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07,
	0x58, 0x8c, 0x97, 0x9a, 0x61, 0xc6, 0x63, 0xd2, 0xf0, 0x66, 0xd0, 0xc2, 0xc0, 0xf9,
	0x89, 0x80, 0x6d, 0x5f, 0x6b, 0x61, 0xda, 0xc3, 0x84, 0x17, 0xe8, 0xd1, 0x2c, 0xfd,
	0xf9, 0x26, 0xe0,
};
#define LX_CCM_AAD 8
#define LX_CCM_TAG 8

/*
 * One CCM operation of `req` on the `len` bytes at `buf` (the header
 * first), in place: encrypting `len` of header and payload, or decrypting
 * them with the tag. The counter block made afresh each time, since CCM
 * writes into it: its first byte the length field's size less one (2 - 1),
 * then the nonce.
 */
static int lx_ccm_op(struct aead_request *req, u8 *buf, unsigned int len, bool encrypt)
{
	struct scatterlist sg;
	u8 iv[16] = { 1 };

	memcpy(iv + 1, lx_ccm_nonce, sizeof(lx_ccm_nonce));
	sg_init_one(&sg, buf, encrypt ? len + LX_CCM_TAG : len);
	aead_request_set_callback(req, 0, NULL, NULL);
	aead_request_set_ad(req, LX_CCM_AAD);
	aead_request_set_crypt(req, &sg, &sg, len - LX_CCM_AAD, iv);
	return encrypt ? crypto_aead_encrypt(req) : crypto_aead_decrypt(req);
}

/*
 * The crypto API's CCM -- what mac80211's CCMP encrypts and authenticates
 * every frame with where the card does not -- against RFC 3610's first
 * vector, as mac80211 asks for it (a synchronous "ccm(aes)"): the vector
 * sealed as the RFC seals it, opened back, and refused with one bit of it
 * changed. Both ends of a simulated network use the same code, so a cipher
 * wrong in the same way at each would pass a join unnoticed. 0, or why
 * not, said.
 */
static int lx_wifi_ccm_check(void)
{
	struct aead_request *req = NULL;
	struct crypto_aead *tfm;
	const char *wrong = NULL;
	u8 *buf = NULL;
	int err;

	tfm = crypto_alloc_aead("ccm(aes)", 0, CRYPTO_ALG_ASYNC);
	if (IS_ERR(tfm)) {
		pr_err("lx-wifi: the crypto API has no ccm(aes): %ld\n", PTR_ERR(tfm));
		return PTR_ERR(tfm);
	}
	err = crypto_aead_setauthsize(tfm, LX_CCM_TAG);
	if (!err)
		err = crypto_aead_setkey(tfm, lx_ccm_key, sizeof(lx_ccm_key));
	buf = kmalloc(sizeof(lx_ccm_sealed), GFP_KERNEL);
	req = aead_request_alloc(tfm, GFP_KERNEL);
	if (!err && (!buf || !req))
		err = -ENOMEM;
	if (!err) {
		memcpy(buf, lx_ccm_plain, sizeof(lx_ccm_plain));
		err = lx_ccm_op(req, buf, sizeof(lx_ccm_plain), true);
		if (!err && memcmp(buf, lx_ccm_sealed, sizeof(lx_ccm_sealed)))
			wrong = "sealed RFC 3610's first vector otherwise than the RFC";
	}
	if (!err && !wrong) {
		err = lx_ccm_op(req, buf, sizeof(lx_ccm_sealed), false);
		if (!err && memcmp(buf, lx_ccm_plain, sizeof(lx_ccm_plain)))
			wrong = "opened RFC 3610's first vector otherwise than the RFC";
	}
	if (!err && !wrong) {
		memcpy(buf, lx_ccm_sealed, sizeof(lx_ccm_sealed));
		buf[LX_CCM_AAD] ^= 1;
		if (lx_ccm_op(req, buf, sizeof(lx_ccm_sealed), false) != -EBADMSG)
			wrong = "opened RFC 3610's first vector with a bit of it changed";
	}
	if (err)
		pr_err("lx-wifi: ccm(aes) could not be set up or run on RFC 3610's first vector: %d\n",
		       err);
	else if (wrong)
		pr_err("lx-wifi: ccm(aes) %s\n", wrong);
	else
		pr_info("lx-wifi: ccm(aes) sealed, opened and refused RFC 3610's first vector as the RFC does\n");
	aead_request_free(req);
	kfree(buf);
	crypto_free_aead(tfm);
	return err ?: wrong ? -EBADMSG : 0;
}

/*
 * "join=<path>": a real network joined, as wpa_supplicant joins one. The
 * file at <path> -- on the boot medium, which only this process may read
 * (the kernel's linuxdrv::wifi_reading) -- names it: an "ssid=" line, its
 * name, and a "psk=" line, its WPA2 passphrase, which nothing here ever
 * says. The card scans; of the access points that answer to the name, the
 * strongest whose RSN element SlateOS can join (lx_wifi_rsn_why) is joined
 * (NL80211_CMD_CONNECT) -- the next, should one refuse -- and the four-way
 * handshake made as the station (lx_wifi_supplicant), its keys installed
 * and its port opened. With "serve=" as well, the joined interface is then
 * served to the network stack (lx_nic.c, which waits for this:
 * lx_nic_await_link_layer); without, the process ends 0 once joined.
 */

/* The most a network file may hold, and the access points tried in turn. */
#define LX_WIFI_NETWORK_MAX 512
#define LX_WIFI_CANDIDATES 4

/* An RSN element's capability: management frame protection required (802.11-2020 9.4.2.24.4). */
#define LX_WIFI_RSN_CAP_MFPR BIT(6)

/* A network file read: the network's name, and its passphrase, NUL-terminated. */
struct lx_wifi_network {
	u8 ssid[IEEE80211_MAX_SSID_LEN];
	size_t ssid_len;
	char psk[64];
};

/*
 * The `len` bytes at `buf`, a network file, read into `n`: lines ending in
 * "\n" or "\r\n", blank ones passed over, each else "ssid=<name>" -- 1 to
 * 32 bytes, any -- or "psk=<passphrase>" -- 8 to 63 printable ASCII
 * characters, as WPA2 has a passphrase -- each once. NULL, or what is wrong
 * with it, which never quotes it.
 */
static const char *lx_wifi_network_parse(const char *buf, size_t len, struct lx_wifi_network *n)
{
	const char *p = buf, *end = buf + len;
	bool named = false, keyed = false;

	while (p < end) {
		const char *nl = memchr(p, '\n', end - p);
		size_t l = (nl ? nl : end) - p;
		size_t i;

		if (l && p[l - 1] == '\r')
			l--;
		if (l > 5 && !memcmp(p, "ssid=", 5)) {
			if (named)
				return "it has two ssid= lines";
			if (l - 5 > IEEE80211_MAX_SSID_LEN)
				return "its ssid= is longer than 32 bytes";
			memcpy(n->ssid, p + 5, l - 5);
			n->ssid_len = l - 5;
			named = true;
		} else if (l > 4 && !memcmp(p, "psk=", 4)) {
			if (keyed)
				return "it has two psk= lines";
			if (l - 4 < 8 || l - 4 > 63)
				return "its psk= is not 8 to 63 characters long";
			for (i = 4; i < l; i++)
				if (p[i] < 0x20 || p[i] > 0x7e)
					return "its psk= holds a character that is not printable ASCII";
			memcpy(n->psk, p + 4, l - 4);
			n->psk[l - 4] = '\0';
			keyed = true;
		} else if (l) {
			return "it has a line that is neither ssid=<name> nor psk=<passphrase>";
		}
		p = nl ? nl + 1 : end;
	}
	if (!named || !keyed)
		return named ? "it has no psk= line" : "it has no ssid= line";
	return NULL;
}

/*
 * Why an access point whose RSN element is `ie` (NULL for none) cannot be
 * joined as this layer joins -- WPA2-PSK, CCMP for both keys, management
 * frames unprotected, which is the station's own element (lx_wifi_rsn) --
 * or NULL when it can: version 1, a CCMP group cipher, CCMP among its
 * pairwise ciphers, PSK among its key managements (a WPA2/WPA3 network
 * offers SAE beside it), and management frame protection not required.
 */
static const char *lx_wifi_rsn_why(const u8 *ie)
{
	static const char *const malformed = "its RSN element is malformed";
	const u8 *p;
	size_t len, i;
	bool ccmp = false, psk = false, sae = false;
	u16 count;

	if (!ie)
		return "it is not protected by WPA2 (it advertises no RSN element)";
	p = ie + 2;
	len = ie[1];
	if (len < 2 || get_unaligned_le16(p) != 1)
		return "its RSN element is not version 1";
	p += 2;
	len -= 2;
	if (len < 4)
		return malformed;
	if (get_unaligned_be32(p) != WLAN_CIPHER_SUITE_CCMP)
		return "its group cipher is not CCMP (AES) -- TKIP, perhaps, which SlateOS does not join";
	p += 4;
	len -= 4;
	if (len < 2)
		return malformed;
	count = get_unaligned_le16(p);
	p += 2;
	len -= 2;
	if (len < 4 * (size_t)count)
		return malformed;
	for (i = 0; i < count; i++, p += 4)
		ccmp |= get_unaligned_be32(p) == WLAN_CIPHER_SUITE_CCMP;
	len -= 4 * (size_t)count;
	if (!ccmp)
		return "it offers no CCMP (AES) pairwise cipher";
	if (len < 2)
		return malformed;
	count = get_unaligned_le16(p);
	p += 2;
	len -= 2;
	if (len < 4 * (size_t)count)
		return malformed;
	for (i = 0; i < count; i++, p += 4) {
		psk |= get_unaligned_be32(p) == WLAN_AKM_SUITE_PSK;
		sae |= get_unaligned_be32(p) == WLAN_AKM_SUITE_SAE;
	}
	len -= 4 * (size_t)count;
	if (!psk)
		return sae ? "it is WPA3-only (SAE), which SlateOS cannot join yet" :
			     "it offers no pre-shared key (PSK) authentication";
	if (len >= 2 && (get_unaligned_le16(p) & LX_WIFI_RSN_CAP_MFPR))
		return "it requires management frame protection, which SlateOS cannot give yet";
	return NULL;
}

/* An access point that answers to the network's name, as the scan heard it. */
struct lx_wifi_candidate {
	u8 bssid[ETH_ALEN];
	u32 freq;
	int dbm;
};

/*
 * What the scan heard of the network: the access points that answer to its
 * name, how many, the joinable ones strongest first -- the first
 * LX_WIFI_CANDIDATES -- and of the others, the strongest's reason.
 */
struct lx_wifi_choice {
	const struct lx_wifi_network *n;
	struct lx_wifi_candidate c[LX_WIFI_CANDIDATES];
	unsigned int heard;
	unsigned int joinable;
	const char *why;
	int why_dbm;
};

/*
 * One access point answering to the network's name told into `ch`: at
 * `bssid` on `freq` MHz, heard at `dbm`, joinable when `why` is NULL --
 * kept among the strongest LX_WIFI_CANDIDATES, strongest first, the earlier
 * heard first of two as strong -- else its reason kept if it is the
 * strongest of those that are not.
 */
static void lx_wifi_rank(struct lx_wifi_choice *ch, const u8 *bssid, u32 freq, int dbm,
			 const char *why)
{
	unsigned int at, kept;

	ch->heard++;
	if (why) {
		if (!ch->why || dbm > ch->why_dbm) {
			ch->why = why;
			ch->why_dbm = dbm;
		}
		return;
	}
	kept = min_t(unsigned int, ch->joinable, LX_WIFI_CANDIDATES);
	ch->joinable++;
	for (at = 0; at < kept && ch->c[at].dbm >= dbm; at++)
		;
	if (at == LX_WIFI_CANDIDATES)
		return;
	memmove(&ch->c[at + 1], &ch->c[at],
		(min_t(unsigned int, kept, LX_WIFI_CANDIDATES - 1) - at) * sizeof(ch->c[0]));
	ether_addr_copy(ch->c[at].bssid, bssid);
	ch->c[at].freq = freq;
	ch->c[at].dbm = dbm;
}

/*
 * lx_wifi_network_parse() and lx_wifi_rsn_why() at their edges, for the
 * "hwsim" run to check before anything relies on them: 0, or -EINVAL, every
 * wrong answer said.
 */
static int lx_wifi_join_check(void)
{
	static const struct {
		const char *text;
		bool good;
	} files[] = {
		{ "ssid=home\npsk=12345678\n", true },
		{ "\r\nssid=home\r\npsk=12345678\r\n\r\n", true },
		{ "psk=a b%c+d!\nssid=x", true },
		{ "ssid=home\n", false },
		{ "psk=12345678\n", false },
		{ "ssid=home\npsk=1234567\n", false },
		{ "ssid=home\npsk=1234567890123456789012345678901234567890123456789012345678901234\n", false },
		{ "ssid=home\npsk=1234567\t8\n", false },
		{ "ssid=123456789012345678901234567890123\npsk=12345678\n", false },
		{ "ssid=a\nssid=b\npsk=12345678\n", false },
		{ "ssid=home\npsk=12345678\nhidden=1\n", false },
		{ "ssid=\npsk=12345678\n", false },
	};
	/* WPA2-PSK, CCMP: the station's own element. */
	static const u8 wpa2[] = { WLAN_EID_RSN, 20, 0x01, 0x00, 0x00, 0x0f, 0xac, 0x04,
				   0x01, 0x00, 0x00, 0x0f, 0xac, 0x04, 0x01, 0x00,
				   0x00, 0x0f, 0xac, 0x02, 0x00, 0x00 };
	/* WPA2/WPA3 transition: PSK and SAE, management frame protection capable. */
	static const u8 mixed[] = { WLAN_EID_RSN, 24, 0x01, 0x00, 0x00, 0x0f, 0xac, 0x04,
				    0x01, 0x00, 0x00, 0x0f, 0xac, 0x04, 0x02, 0x00,
				    0x00, 0x0f, 0xac, 0x02, 0x00, 0x0f, 0xac, 0x08,
				    0x80, 0x00 };
	/* WPA3-only: SAE, protection required. */
	static const u8 wpa3[] = { WLAN_EID_RSN, 20, 0x01, 0x00, 0x00, 0x0f, 0xac, 0x04,
				   0x01, 0x00, 0x00, 0x0f, 0xac, 0x04, 0x01, 0x00,
				   0x00, 0x0f, 0xac, 0x08, 0xc0, 0x00 };
	/* PSK, protection required. */
	static const u8 mfpr[] = { WLAN_EID_RSN, 20, 0x01, 0x00, 0x00, 0x0f, 0xac, 0x04,
				   0x01, 0x00, 0x00, 0x0f, 0xac, 0x04, 0x01, 0x00,
				   0x00, 0x0f, 0xac, 0x02, 0xc0, 0x00 };
	/* WPA/WPA2 mixed: a TKIP group cipher. */
	static const u8 tkip[] = { WLAN_EID_RSN, 20, 0x01, 0x00, 0x00, 0x0f, 0xac, 0x02,
				   0x01, 0x00, 0x00, 0x0f, 0xac, 0x04, 0x01, 0x00,
				   0x00, 0x0f, 0xac, 0x02, 0x00, 0x00 };
	/* Its pairwise count past its end. */
	static const u8 short_ie[] = { WLAN_EID_RSN, 8, 0x01, 0x00, 0x00, 0x0f, 0xac, 0x04,
				       0x02, 0x00 };
	static const struct {
		const u8 *ie;
		bool good;
	} rsns[] = {
		{ wpa2, true }, { mixed, true }, { wpa3, false }, { mfpr, false },
		{ tkip, false }, { short_ie, false }, { NULL, false },
	};
	struct lx_wifi_network n;
	int wrong = 0;
	size_t i;

	for (i = 0; i < ARRAY_SIZE(files); i++) {
		const char *why;

		memset(&n, 0, sizeof(n));
		why = lx_wifi_network_parse(files[i].text, strlen(files[i].text), &n);
		if (!why != files[i].good) {
			pr_err("lx-wifi: network file %zu read %s\n", i,
			       why ? "as refused" : "as good");
			wrong++;
		}
	}
	memset(&n, 0, sizeof(n));
	if (lx_wifi_network_parse(files[2].text, strlen(files[2].text), &n) || n.ssid_len != 1 ||
	    n.ssid[0] != 'x' || strcmp(n.psk, "a b%c+d!")) {
		pr_err("lx-wifi: a network file's name or passphrase read wrong\n");
		wrong++;
	}
	for (i = 0; i < ARRAY_SIZE(rsns); i++) {
		if (!lx_wifi_rsn_why(rsns[i].ie) != rsns[i].good) {
			pr_err("lx-wifi: RSN element %zu judged %s\n", i,
			       rsns[i].good ? "unjoinable" : "joinable");
			wrong++;
		}
	}
	/*
	 * Each element cut short at every length -- its length byte lowered,
	 * nothing of it past that in its buffer -- judged: never joinable
	 * shorter than the shortest that can be (a version, a group cipher, one
	 * pairwise cipher and one key management, 18 bytes), whatever the cut
	 * left. (The layer's C parsers have no fuzz harness yet: known-issues
	 * A-THE-DRIVER-LAYERS-C-PARSERS-ARE-NOT-FUZZED.)
	 */
	for (i = 0; i < ARRAY_SIZE(rsns); i++) {
		const u8 *ie = rsns[i].ie;
		unsigned int len;

		for (len = 0; ie && len < ie[1]; len++) {
			u8 *cut = kmalloc(2 + len, GFP_KERNEL);

			if (!cut)
				return -ENOMEM;
			memcpy(cut, ie, 2 + len);
			cut[1] = len;
			if (!lx_wifi_rsn_why(cut) && len < 18) {
				pr_err("lx-wifi: RSN element %zu cut to %u bytes judged joinable\n", i,
				       len);
				wrong++;
			}
			kfree(cut);
		}
	}
	{
		/*
		 * Seven joinable access points -- two as strong -- and two not,
		 * the strongest of all among those: the four strongest joinable
		 * kept, strongest first, the earlier of two as strong first;
		 * every one counted, and the reason the strongest of the others
		 * gave kept.
		 */
		static const struct {
			int dbm;
			const char *why;
		} heard[] = {
			{ -70, NULL }, { -40, NULL }, { -30, "x" }, { -60, NULL }, { -80, NULL },
			{ -50, NULL }, { -75, "y" }, { -90, NULL }, { -50, NULL },
		};
		static const u8 order[LX_WIFI_CANDIDATES] = { 1, 5, 8, 3 };
		struct lx_wifi_choice *ch = kzalloc(sizeof(*ch), GFP_KERNEL);
		u8 bssid[ETH_ALEN] = { 0x02 };

		if (!ch)
			return -ENOMEM;
		for (i = 0; i < ARRAY_SIZE(heard); i++) {
			bssid[ETH_ALEN - 1] = i;
			lx_wifi_rank(ch, bssid, 2412, heard[i].dbm, heard[i].why);
		}
		if (ch->heard != ARRAY_SIZE(heard) || ch->joinable != 7 || !ch->why ||
		    strcmp(ch->why, "x")) {
			pr_err("lx-wifi: access points counted %u heard, %u joinable, not 9 and 7, or the strongest refused's reason lost\n",
			       ch->heard, ch->joinable);
			wrong++;
		}
		for (i = 0; i < LX_WIFI_CANDIDATES; i++) {
			if (ch->c[i].bssid[ETH_ALEN - 1] != order[i] ||
			    ch->c[i].dbm != heard[order[i]].dbm) {
				pr_err("lx-wifi: the joinable access point kept %zu-th is the %u-th heard, not the %u-th\n",
				       i, ch->c[i].bssid[ETH_ALEN - 1], order[i]);
				wrong++;
			}
		}
		kfree(ch);
	}
	if (wrong)
		return -EINVAL;
	pr_info("lx-wifi: network files read, access points' RSN elements judged, and those heard ranked, as they should be\n");
	return 0;
}

static void lx_wifi_choose(struct wiphy *wiphy, struct cfg80211_bss *bss, void *data)
{
	struct lx_wifi_choice *ch = data;
	const u8 *ssid;
	const char *why;

	rcu_read_lock();
	ssid = ieee80211_bss_get_ie(bss, WLAN_EID_SSID);
	if (!ssid || ssid[1] != ch->n->ssid_len || memcmp(ssid + 2, ch->n->ssid, ssid[1])) {
		rcu_read_unlock();
		return;
	}
	why = lx_wifi_rsn_why(ieee80211_bss_get_ie(bss, WLAN_EID_RSN));
	rcu_read_unlock();
	lx_wifi_rank(ch, bss->bssid, bss->channel->center_freq, bss->signal / 100, why);
}

/*
 * The next message nl80211's "mlme" group sends saying how `dev`'s
 * connection ended, read into `c`, within LX_WIFI_JOIN_S: 0, or -ETIMEDOUT
 * for none.
 */
static int lx_wifi_connect_end(const struct genl_family *nl80211, struct net_device *dev,
			       struct lx_wifi_connected *c)
{
	unsigned long until = jiffies + LX_WIFI_JOIN_S * HZ;
	struct sk_buff *msg;
	bool ended = false;

	while (!ended &&
	       (msg = lx_genl_event(nl80211, NL80211_MULTICAST_GROUP_MLME, lx_wifi_left(until)))) {
		ended = lx_wifi_connect_result(msg, dev, c);
		nlmsg_free(msg);
	}
	return ended ? 0 : -ETIMEDOUT;
}

/*
 * The four-way handshake with the access point `st`'s interface has just
 * joined, as its station, from `pmk`: its frames, which `st` has been
 * taking since before the join, handled (lx_wifi_supplicant) until its
 * keys are in and its port is open, within LX_WIFI_HANDSHAKE_S. 0, or why
 * not, said.
 */
static int lx_wifi_station_handshake(struct lx_wifi_station *st, const u8 pmk[LX_WPA_PMK_LEN])
{
	struct lx_wifi_handshaking *h = &st->h;
	struct net_device *dev = h->sta_dev;
	unsigned long until = jiffies + LX_WIFI_HANDSHAKE_S * HZ;
	struct sk_buff *skb;
	int err;

	memcpy(h->supplicant.pmk, pmk, LX_WPA_PMK_LEN);
	err = lx_wifi_joined(dev, &h->supplicant);
	while (!err && !h->supplicant.done) {
		if (!wait_event_timeout(st->wq, !skb_queue_empty(&st->eapol.frames),
					lx_wifi_left(until))) {
			netdev_err(dev, "the handshake did not end in %d s: %s\n",
				   LX_WIFI_HANDSHAKE_S,
				   h->supplicant.frames ?
					   "the access point never sent its third message -- a passphrase that is not the network's is the usual reason" :
					   "the access point never sent its first message");
			err = -ETIMEDOUT;
			break;
		}
		while (!err && (skb = skb_dequeue(&st->eapol.frames))) {
			err = lx_wifi_supplicant(h, skb);
			kfree_skb(skb);
		}
	}
	return err;
}

/*
 * The network `n` joined on `dev`, which is up and has scanned, at the
 * access point `c` -- protected from the first frame: the handshake's
 * frames taken from before the connection is asked for, since the access
 * point sends its first message as soon as it has associated the station.
 * The station, its keys in, kept for the association (lx_wifi_station_keep)
 * into `*kept`. 0, or why not, said: a refused connection -EAGAIN, for the
 * next access point to be tried.
 */
static int lx_wifi_join_at(const struct genl_family *nl80211, struct net_device *dev,
			   const struct lx_wifi_network *n, const struct lx_wifi_candidate *c,
			   const u8 pmk[LX_WPA_PMK_LEN], struct lx_wifi_station **kept)
{
	const struct lx_wifi_connect connect = {
		.ssid = n->ssid,
		.ssid_len = n->ssid_len,
		.bssid = c->bssid,
		.freq = c->freq,
		.wpa2 = true,
	};
	struct lx_wifi_connected done = {};
	struct lx_wifi_station *st = lx_wifi_station_new(nl80211, dev);
	int err;

	if (!st)
		return -ENOMEM;
	netdev_info(dev, "joining %pM on %u MHz, %d dBm\n", c->bssid, c->freq, c->dbm);
	/* Until the station is associated, a failure leaves the next access point to try. */
	err = lx_wifi_ask(nl80211, NL80211_CMD_CONNECT, dev, lx_wifi_connect_params, &connect,
			  "connect") ? -EAGAIN : 0;
	if (!err && lx_wifi_connect_end(nl80211, dev, &done)) {
		static const u16 leaving = WLAN_REASON_DEAUTH_LEAVING;

		netdev_err(dev, "nl80211 never said how joining %pM ended, in %d s\n", c->bssid,
			   LX_WIFI_JOIN_S);
		/* Whatever it reached, left, for the next to be asked. */
		lx_wifi_ask(nl80211, NL80211_CMD_DISCONNECT, dev, lx_wifi_reason, &leaving,
			    "give the join up");
		err = -EAGAIN;
	}
	if (!err && done.status != WLAN_STATUS_SUCCESS) {
		netdev_err(dev, "%pM refused it: status %d%s%s\n", c->bssid, done.status,
			   done.timed_out ? ", timed out in " : "",
			   done.timed_out ? lx_wifi_timeout_step(done.timeout_reason) : "");
		err = -EAGAIN;
	}
	if (!err)
		err = lx_wifi_station_handshake(st, pmk);
	if (!err && !lx_wifi_oper_up(dev)) {
		netdev_err(dev, "joined, but the network core did not bring its link up in %d ms\n",
			   LX_WIFI_FRAME_MS);
		err = -ENOLINK;
	}
	if (err) {
		lx_wifi_station_free(st);
		return err;
	}
	lx_wifi_station_keep(st);
	*kept = st;
	netdev_info(dev, "joined \"%*pE\" (WPA2-PSK, CCMP) at %pM on %u MHz, %d dBm: its keys installed, its port open\n",
		    (int)n->ssid_len, n->ssid, c->bssid, c->freq, c->dbm);
	return 0;
}

/*
 * The station a join made, kept for the life of the process: its keeper
 * answers the access point's key management for as long as the card is
 * served (lx_wifi_keep).
 */
static struct lx_wifi_station *lx_wifi_joined_station;

/*
 * The network the file at `path` names (lx_wifi_network_parse) joined on
 * the card's interface: scanned for by name, and its joinable access
 * points tried, strongest first, until one takes the station -- which is
 * then kept (lx_wifi_joined_station). The join's nl80211 groups are left
 * once it has ended, so that nothing they send later is kept for a reader
 * gone. 0, or why not, said -- never the passphrase.
 */
static int lx_wifi_join_network(const char *path)
{
	const struct genl_family *nl80211 = lx_wifi_nl80211();
	struct lx_wifi_network *n = kzalloc(sizeof(*n), GFP_KERNEL);
	struct lx_wifi_choice *ch = kzalloc(sizeof(*ch), GFP_KERNEL);
	u8 pmk[LX_WPA_PMK_LEN];
	struct net_device *dev = NULL;
	struct lx_wifi_ssid wanted;
	const char *why;
	void *buf = NULL;
	size_t size = 0;
	ssize_t got;
	unsigned int i;
	int err;

	if (!nl80211 || !n || !ch) {
		kfree(n);
		kfree(ch);
		return nl80211 ? -ENOMEM : -ENODEV;
	}
	got = kernel_read_file_from_path_initns(path, 0, &buf, LX_WIFI_NETWORK_MAX, &size,
						READING_UNKNOWN);
	if (got < 0) {
		pr_err("lx-wifi: join: the network file %s could not be read: %zd\n", path, got);
		err = got;
		goto out;
	}
	/* `size` is the whole file's: one read no further than the most is refused, not cut short. */
	why = size > LX_WIFI_NETWORK_MAX ? "it is longer than 512 bytes" :
					   lx_wifi_network_parse(buf, got, n);
	memzero_explicit(buf, got);
	vfree(buf);
	if (why) {
		pr_err("lx-wifi: join: the network file %s is not one: %s\n", path, why);
		err = -EINVAL;
		goto out;
	}
	/* The PMK first: the handshake's first message comes the moment the station is associated. */
	lx_wpa_pmk(n->psk, n->ssid, n->ssid_len, pmk);
	memzero_explicit(n->psk, sizeof(n->psk));
	wanted = (struct lx_wifi_ssid){ .ssid = n->ssid, .len = n->ssid_len };
	err = lx_wifi_devs(&dev, 1) ? 0 : -ENODEV;
	if (!err)
		err = lx_wifi_open(dev);
	if (!err)
		err = lx_genl_listen(nl80211, NL80211_MULTICAST_GROUP_MLME);
	if (!err)
		err = lx_wifi_scan_on(nl80211, dev, &wanted);
	if (err)
		goto out;
	ch->n = n;
	cfg80211_bss_iter(dev->ieee80211_ptr->wiphy, NULL, lx_wifi_choose, ch);
	if (!ch->joinable) {
		if (ch->heard)
			pr_err("lx-wifi: join: \"%*pE\" was heard from %u access point(s), none of which SlateOS can join: the strongest's %s\n",
			       (int)n->ssid_len, n->ssid, ch->heard, ch->why);
		else
			pr_err("lx-wifi: join: no access point answered to \"%*pE\", asked for by name\n",
			       (int)n->ssid_len, n->ssid);
		err = -ENOENT;
		goto out;
	}
	pr_info("lx-wifi: join: \"%*pE\" heard from %u access point(s), %u of them joinable\n",
		(int)n->ssid_len, n->ssid, ch->heard, ch->joinable);
	err = -EAGAIN;
	for (i = 0; err == -EAGAIN && i < min_t(unsigned int, ch->joinable, LX_WIFI_CANDIDATES);
	     i++)
		err = lx_wifi_join_at(nl80211, dev, n, &ch->c[i], pmk, &lx_wifi_joined_station);
out:
	lx_genl_unlisten(nl80211, NL80211_MULTICAST_GROUP_MLME);
	lx_genl_unlisten(nl80211, NL80211_MULTICAST_GROUP_SCAN);
	memzero_explicit(pmk, sizeof(pmk));
	if (dev)
		lx_wifi_put(&dev, 1);
	kfree_sensitive(n);
	kfree(ch);
	return err;
}

/* How the join ended, for the network task to wait on (lx_nic_await_link_layer). */
static DECLARE_COMPLETION(lx_wifi_join_done);
static int lx_wifi_join_err = -ENODEV;

/*
 * The network task's wait before it serves the card (lx_nic.c): with
 * "join=", until the network is joined -- its link-layer frames, the
 * handshake's, this task's until then -- and how that ended; else at once.
 */
int lx_nic_await_link_layer(void)
{
	if (!lx_arg("join="))
		return 0;
	wait_for_completion(&lx_wifi_join_done);
	return lx_wifi_join_err;
}

static int lx_wifi_hwsim(void)
{
	const struct genl_family *nl80211 = lx_wifi_nl80211();
	struct net_device *devs[2];
	struct lx_wifi_ap *ap;
	int err;

	if (!nl80211 || !lx_wifi_devs(devs, 2))
		return -ENODEV;
	/* The access point's state outlives a join, and holds its group key: kept off the stack. */
	ap = kzalloc(sizeof(*ap), GFP_KERNEL);
	if (!ap) {
		lx_wifi_put(devs, 2);
		return -ENOMEM;
	}
	ap->nl80211 = nl80211;
	ap->dev = devs[0];
	/* What the join relies on, at its edges, first -- and the cipher it will encrypt with. */
	err = lx_genl_check(nl80211, NL80211_MULTICAST_GROUP_MLME, NL80211_MULTICAST_GROUP_SCAN);
	if (!err)
		err = lx_wifi_ccm_check();
	if (!err)
		err = lx_wpa_check();
	if (!err)
		err = lx_wifi_join_check();
	if (!err)
		err = lx_wifi_ask(nl80211, NL80211_CMD_SET_INTERFACE, devs[0], lx_wifi_ap_type, NULL,
				  "make the first radio an access point");
	if (!err)
		err = lx_wifi_open(devs[0]);
	if (!err)
		err = lx_wifi_ask(nl80211, NL80211_CMD_START_AP, devs[0], lx_wifi_ap_params, NULL,
				  "start the access point");
	if (!err)
		err = lx_wifi_ap_take_frames(nl80211, devs[0]);
	if (!err)
		err = lx_wifi_open(devs[1]);
	if (!err)
		err = lx_wifi_scan_on(nl80211, devs[1], NULL);
	if (!err)
		err = lx_wifi_heard(devs[1], devs[0], false);
	/* The open network: joined, and a frame across it each way, in clear on the air. */
	if (!err)
		err = lx_wifi_join(nl80211, ap, devs[1]);
	if (!err)
		err = lx_wifi_links_up(devs[1], devs[0]);
	if (!err)
		err = lx_wifi_cross_watched(devs[1], devs[0], false);
	if (!err)
		pr_info("lx-wifi: hwsim: a frame crossed \"%s\" each way, open\n", LX_WIFI_SSID);
	/*
	 * The same network protected (WPA2-PSK): joined again, the four-way
	 * handshake made, and a frame across it each way, protected on the air.
	 */
	if (!err)
		err = lx_wifi_protect(nl80211, ap, devs[1]);
	/*
	 * Scanned again first, as wpa_supplicant scans before it connects:
	 * cfg80211 matches a protected connection only to a network it has
	 * heard advertise privacy, and it last heard this one open.
	 */
	if (!err)
		err = lx_wifi_scan_on(nl80211, devs[1], NULL);
	if (!err)
		err = lx_wifi_heard(devs[1], devs[0], true);
	if (!err)
		err = lx_wifi_join(nl80211, ap, devs[1]);
	if (!err)
		err = lx_wifi_handshake(nl80211, ap, devs[1]);
	if (!err)
		err = lx_wifi_links_up(devs[1], devs[0]);
	if (!err)
		err = lx_wifi_cross_watched(devs[1], devs[0], true);
	if (!err)
		pr_info("lx-wifi: hwsim: a frame crossed \"%s\" each way, protected by WPA2\n",
			LX_WIFI_SSID);
	kfree_sensitive(ap);
	lx_wifi_put(devs, 2);
	return err;
}

static int lx_wifi_task(void *unused)
{
	/* A warning anywhere -- the driver, the 802.11 stack, the layer -- is a bug. */
	if (lx_arg("scan"))
		lx_end_run("lx-wifi", lx_wifi_scan());
	if (lx_arg("hwsim"))
		lx_end_run("lx-wifi", lx_wifi_hwsim());
	if (lx_arg("join=")) {
		lx_wifi_join_err = lx_wifi_join_network(lx_arg("join="));
		complete_all(&lx_wifi_join_done);
		/* Joined and served: the network task ends the run, when its stacks have gone. */
		if (lx_wifi_join_err || !lx_arg("serve="))
			lx_end_run("lx-wifi", lx_wifi_join_err);
		return 0;
	}
	if (lx_arg("pci="))
		return 0;
	/*
	 * Given no card there is nothing to drive: the process ends once its
	 * initcalls are done (lx_emul_main() says so) -- not when the timers
	 * the 802.11 stack set have all run, the last of them a minute out
	 * (cfg80211 checks the channels against a new regulatory domain after
	 * REG_ENFORCE_GRACE_MS).
	 */
	while (system_state != SYSTEM_RUNNING)
		msleep(10);
	lx_end_run("lx-wifi", 0);
}

/* After the driver's own initcalls, which register its radios as they probe. */
static int __init lx_wifi_start(void)
{
	struct task_struct *task = kthread_run(lx_wifi_task, NULL, "lx-wifi");

	return PTR_ERR_OR_ZERO(task);
}
late_initcall(lx_wifi_start);
