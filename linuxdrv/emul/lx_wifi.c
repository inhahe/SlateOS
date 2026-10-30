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
 * - "hwsim": first the layer's netlink at its edges (lx_genl_check), and
 *   the crypto API's CCM against RFC 3610 (lx_wifi_ccm_check); then
 *   mac80211_hwsim's two simulated radios on one medium -- the
 *   first made an open access point (NL80211_CMD_SET_INTERFACE to AP, then
 *   NL80211_CMD_START_AP, beaconing LX_WIFI_SSID on LX_WIFI_FREQ), the
 *   second scanning as above, which must find that network, from the first
 *   radio's address, on that frequency. Then the second joins it
 *   (NL80211_CMD_CONNECT), the layer answering its authentication and
 *   association on the first as hostapd would (lx_wifi_join), and a frame
 *   crosses the network each way (lx_wifi_cross); the process ends 0 only
 *   if every step did. The whole of the stack and the layer's use of it,
 *   where there is no card (the boot test).
 * - Neither, and given no card: the process ends once its initcalls are
 *   done. Given a card, the task does nothing.
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
#include <linux/key.h>
#include <linux/kthread.h>
#include <linux/netdevice.h>
#include <linux/random.h>
#include <linux/rtnetlink.h>
#include <linux/unaligned.h>
#include <linux/skbuff.h>
#include <linux/slab.h>
#include <linux/verification.h>
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

/* A passive scan on `dev`, which is up, to its end: 0, or why not. */
static int lx_wifi_scan_on(const struct genl_family *nl80211, struct net_device *dev)
{
	int err = lx_genl_listen(nl80211, NL80211_MULTICAST_GROUP_SCAN);
	u8 end;

	if (!err)
		err = lx_wifi_ask(nl80211, NL80211_CMD_TRIGGER_SCAN, dev, NULL, NULL, "scan");
	if (err)
		return err;
	end = lx_wifi_scan_end(nl80211);
	if (end != NL80211_CMD_NEW_SCAN_RESULTS) {
		netdev_err(dev, "the scan %s\n", end ? "was aborted" : "did not finish in time");
		return -ETIMEDOUT;
	}
	return 0;
}

/* What the networks a scan found are told against: the one wanted, if any. */
struct lx_wifi_found {
	unsigned int networks;
	const u8 *bssid;
	bool wanted;
};

/*
 * One network cfg80211 has: said, its name escaped as Linux prints one;
 * and whether it is the wanted one -- LX_WIFI_SSID from `bssid`, on
 * LX_WIFI_FREQ.
 */
static void lx_wifi_bss(struct wiphy *wiphy, struct cfg80211_bss *bss, void *data)
{
	struct lx_wifi_found *found = data;
	const u8 *ssid;

	rcu_read_lock();
	ssid = ieee80211_bss_get_ie(bss, WLAN_EID_SSID);
	pr_info("lx-wifi: \"%*pE\" %pM %u MHz %d dBm\n", ssid ? ssid[1] : 0, ssid ? ssid + 2 : NULL,
		bss->bssid, bss->channel->center_freq, bss->signal / 100);
	if (found->bssid && ssid && ssid[1] == strlen(LX_WIFI_SSID) &&
	    !memcmp(ssid + 2, LX_WIFI_SSID, ssid[1]) && ether_addr_equal(bss->bssid, found->bssid) &&
	    bss->channel->center_freq == LX_WIFI_FREQ)
		found->wanted = true;
	rcu_read_unlock();
	found->networks++;
}

/* The networks `dev`'s wiphy has, said and told against `found`. */
static void lx_wifi_results(struct net_device *dev, struct lx_wifi_found *found)
{
	cfg80211_bss_iter(dev->ieee80211_ptr->wiphy, NULL, lx_wifi_bss, found);
	pr_info("lx-wifi: scan: %u network(s)\n", found->networks);
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
		err = lx_wifi_scan_on(nl80211, dev);
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
 * A WPA2-PSK network's RSN element, as the access point beacons it and the
 * station asks for it: version 1, CCMP its group and its one pairwise
 * cipher, PSK its one key management, no capabilities.
 */
static const u8 lx_wifi_rsn[] = {
	WLAN_EID_RSN, 20,
	0x01, 0x00,
	0x00, 0x0f, 0xac, 0x04,
	0x01, 0x00, 0x00, 0x0f, 0xac, 0x04,
	0x01, 0x00, 0x00, 0x0f, 0xac, 0x02,
	0x00, 0x00,
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
	u8 tail[3 + sizeof(lx_wifi_rsn)] = { WLAN_EID_DS_PARAMS, 1, LX_WIFI_CHANNEL };
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
		memcpy(tail + 3, lx_wifi_rsn, sizeof(lx_wifi_rsn));
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
 * the port), the RSN element a protected network's station associated
 * with, its group key, and the first error met answering it.
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
	u8 sta_rsn[sizeof(lx_wifi_rsn)];
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
 * RSN element), kept for the handshake to hold the station to, and the
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
		memcpy(ap->sta_rsn, rsn, sizeof(ap->sta_rsn));
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

/* The network a station connects to: the access point's address, and whether it is protected. */
struct lx_wifi_connect {
	const u8 *bssid;
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

	return nla_put(req, NL80211_ATTR_SSID, strlen(LX_WIFI_SSID), LX_WIFI_SSID) ||
	       nla_put(req, NL80211_ATTR_MAC, ETH_ALEN, c->bssid) ||
	       nla_put_u32(req, NL80211_ATTR_WIPHY_FREQ, LX_WIFI_FREQ) ||
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
static int lx_wifi_join(const struct genl_family *nl80211, struct lx_wifi_ap *ap_state,
			struct net_device *sta_dev)
{
	struct lx_wifi_ap ap = *ap_state;
	struct net_device *ap_dev = ap.dev;
	const struct lx_wifi_connect connect = { .bssid = ap_dev->dev_addr, .wpa2 = ap.wpa2 };
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
				   ap.associated ? "associated" : ap.authenticated ? "authenticated" :
										    "never heard from");
			return -ETIMEDOUT;
		}
		while ((msg = lx_genl_unicast())) {
			lx_wifi_ap_message(&ap, msg);
			nlmsg_free(msg);
		}
		err = ap.err;
		while ((msg = lx_genl_event(nl80211, NL80211_MULTICAST_GROUP_MLME, 0))) {
			if (!ended)
				ended = lx_wifi_connect_result(msg, sta_dev, &c);
			nlmsg_free(msg);
		}
	}
	if (err)
		return err;
	if (c.status != WLAN_STATUS_SUCCESS || !ap.associated) {
		netdev_err(sta_dev, "joining \"%s\" ended with status %d%s%s, the access point %s\n",
			   LX_WIFI_SSID, c.status, c.timed_out ? ", timed out in " : "",
			   c.timed_out ? lx_wifi_timeout_step(c.timeout_reason) : "",
			   ap.associated ? "having associated it" : "not having associated it");
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
			    ap.wpa2 ? " (WPA2, its port closed until the handshake)" : "",
			    ap_dev->dev_addr, sta_dev->dev_addr);
	*ap_state = ap;
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

/* A key installed (NL80211_CMD_NEW_KEY): CCMP's, pairwise with the peer at `mac`, or the group's. */
struct lx_wifi_key {
	const u8 *key;
	u8 idx;
	const u8 *mac;
};

static int lx_wifi_key_params(struct sk_buff *req, struct net_device *dev, const void *arg)
{
	const struct lx_wifi_key *k = arg;

	return nla_put(req, NL80211_ATTR_KEY_DATA, LX_WPA_TK_LEN, k->key) ||
	       nla_put_u8(req, NL80211_ATTR_KEY_IDX, k->idx) ||
	       nla_put_u32(req, NL80211_ATTR_KEY_CIPHER, WLAN_CIPHER_SUITE_CCMP) ||
	       nla_put_u32(req, NL80211_ATTR_KEY_TYPE,
			   k->mac ? NL80211_KEYTYPE_PAIRWISE : NL80211_KEYTYPE_GROUP) ||
	       (k->mac && nla_put(req, NL80211_ATTR_MAC, ETH_ALEN, k->mac));
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
 * take (lx_wifi_handshake), its waiter woken.
 */
struct lx_wifi_eapol {
	struct packet_type type;
	struct sk_buff_head frames;
	wait_queue_head_t *wq;
};

static int lx_wifi_eapol_rx(struct sk_buff *skb, struct net_device *dev, struct packet_type *pt,
			    struct net_device *orig_dev)
{
	struct lx_wifi_eapol *e = container_of(pt, struct lx_wifi_eapol, type);

	skb_queue_tail(&e->frames, skb);
	wake_up(e->wq);
	return NET_RX_SUCCESS;
}

/* One side's part of the handshake: its PMK, the PTK, both nonces, the replay counter, and whether it is done. */
struct lx_wifi_side {
	u8 pmk[LX_WPA_PMK_LEN];
	struct lx_wpa_ptk ptk;
	u8 anonce[LX_WPA_NONCE_LEN];
	u8 snonce[LX_WPA_NONCE_LEN];
	u64 replay;
	bool replayed;
	int frames;
	bool done;
};

/* The handshake under way: the access point's join, the station, and the two sides. */
struct lx_wifi_handshaking {
	const struct genl_family *nl80211;
	struct lx_wifi_ap *ap;
	struct net_device *sta_dev;
	struct lx_wifi_side authenticator;
	struct lx_wifi_side supplicant;
};

/* An EAPOL-Key frame built (lx_wpa_key_frame) and sent from `from` to `to`: 0, or why not, said. */
static int lx_wifi_eapol_send(struct net_device *from, const u8 *to, u16 info, u64 replay,
			      const u8 *nonce, const u8 *data, size_t data_len, const u8 *kck)
{
	u8 buf[sizeof(struct lx_wpa_eapol_key) + LX_WPA_KEY_DATA_MAX + 8];
	int len = lx_wpa_key_frame(buf, sizeof(buf), info, replay, nonce, data, data_len, kck);

	if (len < 0) {
		netdev_err(from, "a handshake frame to %pM could not be built: %d\n", to, len);
		return len;
	}
	return lx_wifi_send(from, to, ETH_P_PAE, buf, len);
}

/*
 * The supplicant -- the station -- given a frame of the handshake from the
 * access point, as wpa_supplicant takes it: message 1 answered with
 * message 2 (its nonce, the PTK derived, its RSN element, signed);
 * message 3 checked (signed by that PTK, the access point's nonce
 * unchanged, a later replay counter, the network's RSN element and the
 * group key wrapped inside), answered with message 4, and its keys
 * installed and its port opened. 0, or what was wrong, said.
 */
static int lx_wifi_supplicant(struct lx_wifi_handshaking *h, struct sk_buff *skb)
{
	struct lx_wifi_side *s = &h->supplicant;
	struct net_device *dev = h->sta_dev;
	const u8 *bssid = h->ap->dev->dev_addr;
	const struct lx_wpa_eapol_key *k;
	u8 plain[LX_WPA_KEY_DATA_MAX];
	const u8 *gtk;
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
	if (!(info & LX_WPA_INFO_ACK) || !(info & LX_WPA_INFO_PAIRWISE) ||
	    (s->replayed && replay <= s->replay)) {
		netdev_err(dev, "a handshake frame (key information %#x, replay counter %llu) the access point would not send\n",
			   info, replay);
		return -EPROTO;
	}
	if (!(info & LX_WPA_INFO_MIC)) {
		/* Message 1. */
		memcpy(s->anonce, k->nonce, LX_WPA_NONCE_LEN);
		get_random_bytes(s->snonce, LX_WPA_NONCE_LEN);
		lx_wpa_ptk(s->pmk, bssid, dev->dev_addr, s->anonce, s->snonce, &s->ptk);
		s->replay = replay;
		s->replayed = true;
		s->frames = 1;
		return lx_wifi_eapol_send(dev, bssid, LX_WPA_INFO_VERSION | LX_WPA_INFO_PAIRWISE |
					  LX_WPA_INFO_MIC, replay, s->snonce, lx_wifi_rsn,
					  sizeof(lx_wifi_rsn), s->ptk.kck);
	}
	/* Message 3. */
	if (s->frames != 1 || !(info & LX_WPA_INFO_INSTALL) || !(info & LX_WPA_INFO_SECURE) ||
	    !(info & LX_WPA_INFO_ENCRYPTED)) {
		netdev_err(dev, "a third handshake frame out of turn, or not installing a key (key information %#x)\n",
			   info);
		return -EPROTO;
	}
	if (!lx_wpa_key_parse(skb->data, skb->len, s->ptk.kck)) {
		netdev_err(dev, "the third handshake frame's MIC does not hold\n");
		return -EBADMSG;
	}
	if (memcmp(k->nonce, s->anonce, LX_WPA_NONCE_LEN)) {
		netdev_err(dev, "the third handshake frame's nonce is not the first's\n");
		return -EPROTO;
	}
	if (data_len < 24 || data_len > sizeof(plain) + 8 ||
	    lx_wpa_unwrap(s->ptk.kek, k->data, data_len, plain)) {
		netdev_err(dev, "the third handshake frame's key data does not unwrap\n");
		return -EBADMSG;
	}
	gtk = lx_wpa_find_gtk(plain, data_len - 8, &keyid, &gtk_len);
	if (memcmp(plain, lx_wifi_rsn, sizeof(lx_wifi_rsn)) || !gtk || gtk_len != LX_WPA_TK_LEN) {
		netdev_err(dev, "the third handshake frame's key data has not the network's RSN element and a group key\n");
		memzero_explicit(plain, sizeof(plain));
		return -EPROTO;
	}
	s->replay = replay;
	err = lx_wifi_eapol_send(dev, bssid, LX_WPA_INFO_VERSION | LX_WPA_INFO_PAIRWISE |
				 LX_WPA_INFO_MIC | LX_WPA_INFO_SECURE, replay, NULL, NULL, 0,
				 s->ptk.kck);
	/* Message 4 is sent before the keys go in: it must leave unencrypted. */
	if (!err) {
		const struct lx_wifi_key ptk = { .key = s->ptk.tk, .idx = 0, .mac = bssid };
		const struct lx_wifi_key group = { .key = gtk, .idx = keyid };

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
 * The authenticator -- the access point -- given a frame of the handshake
 * from its station, as hostapd takes it: message 2 checked (the replay
 * counter of message 1, signed by the PTK its nonce gives, the RSN
 * element the station associated with) and answered with message 3 (the
 * network's RSN element and the group key, wrapped by the KEK, signed);
 * message 4 checked, the station's key installed and its port opened. 0,
 * or what was wrong, said.
 */
static int lx_wifi_authenticator(struct lx_wifi_handshaking *h, struct sk_buff *skb)
{
	struct lx_wifi_side *a = &h->authenticator;
	struct lx_wifi_ap *ap = h->ap;
	const struct lx_wpa_eapol_key *k;
	u8 plain[sizeof(lx_wifi_rsn) + 8 + LX_WPA_TK_LEN];
	u8 wrapped[LX_WPA_KEY_DATA_MAX + 8];
	u16 info;
	int len, err;

	if (skb_linearize(skb) || !(k = lx_wpa_key_parse(skb->data, skb->len, NULL)) ||
	    !ether_addr_equal(eth_hdr(skb)->h_source, ap->sta)) {
		netdev_err(ap->dev, "a handshake frame that is not one, or not the station's\n");
		return -EPROTO;
	}
	info = be16_to_cpu(k->info);
	if ((info & LX_WPA_INFO_ACK) || !(info & LX_WPA_INFO_MIC) ||
	    get_unaligned_be64(k->replay) != a->replay) {
		netdev_err(ap->dev, "a handshake frame (key information %#x) the station would not send, or not of the last replay counter\n",
			   info);
		return -EPROTO;
	}
	if (!(info & LX_WPA_INFO_SECURE)) {
		/* Message 2. */
		if (a->frames != 1) {
			netdev_err(ap->dev, "a second handshake frame out of turn\n");
			return -EPROTO;
		}
		memcpy(a->snonce, k->nonce, LX_WPA_NONCE_LEN);
		lx_wpa_ptk(a->pmk, ap->dev->dev_addr, ap->sta, a->anonce, a->snonce, &a->ptk);
		if (!lx_wpa_key_parse(skb->data, skb->len, a->ptk.kck)) {
			netdev_err(ap->dev, "the second handshake frame's MIC does not hold: the passphrases differ\n");
			return -EBADMSG;
		}
		if (be16_to_cpu(k->data_length) != sizeof(ap->sta_rsn) ||
		    memcmp(k->data, ap->sta_rsn, sizeof(ap->sta_rsn))) {
			netdev_err(ap->dev, "the second handshake frame's RSN element is not the one the station associated with\n");
			return -EPROTO;
		}
		/* The network's RSN element, then the group key's KDE: its id, and the key. */
		memcpy(plain, lx_wifi_rsn, sizeof(lx_wifi_rsn));
		plain[sizeof(lx_wifi_rsn)] = 0xdd;
		plain[sizeof(lx_wifi_rsn) + 1] = 6 + LX_WPA_TK_LEN;
		memcpy(plain + sizeof(lx_wifi_rsn) + 2, "\x00\x0f\xac\x01", 4);
		plain[sizeof(lx_wifi_rsn) + 6] = LX_WIFI_GTK_IDX;
		plain[sizeof(lx_wifi_rsn) + 7] = 0;
		memcpy(plain + sizeof(lx_wifi_rsn) + 8, ap->gtk, LX_WPA_TK_LEN);
		len = lx_wpa_key_data_wrap(a->ptk.kek, plain, sizeof(plain), wrapped, sizeof(wrapped));
		memzero_explicit(plain, sizeof(plain));
		if (len < 0) {
			netdev_err(ap->dev, "the group key could not be wrapped: %d\n", len);
			return len;
		}
		a->replay++;
		a->frames = 3;
		return lx_wifi_eapol_send(ap->dev, ap->sta, LX_WPA_INFO_VERSION | LX_WPA_INFO_PAIRWISE |
					  LX_WPA_INFO_INSTALL | LX_WPA_INFO_ACK | LX_WPA_INFO_MIC |
					  LX_WPA_INFO_SECURE | LX_WPA_INFO_ENCRYPTED, a->replay,
					  a->anonce, wrapped, len, a->ptk.kck);
	}
	/* Message 4. */
	if (a->frames != 3) {
		netdev_err(ap->dev, "a fourth handshake frame out of turn\n");
		return -EPROTO;
	}
	if (!lx_wpa_key_parse(skb->data, skb->len, a->ptk.kck)) {
		netdev_err(ap->dev, "the fourth handshake frame's MIC does not hold\n");
		return -EBADMSG;
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
	return err;
}

/*
 * The four-way handshake between the access point `ap`, whose station
 * `sta_dev` has just joined its protected network, and that station --
 * the layer each side, as hostapd and wpa_supplicant are: each side's PMK
 * worked out from the passphrase, message 1 sent, and each side's frames
 * taken as they come (lx_wifi_supplicant, lx_wifi_authenticator) until
 * both have installed their keys and opened their ports, within
 * LX_WIFI_HANDSHAKE_S. 0, or why not, said. Every key is wiped after.
 */
static int lx_wifi_handshake(const struct genl_family *nl80211, struct lx_wifi_ap *ap,
			     struct net_device *sta_dev)
{
	DECLARE_WAIT_QUEUE_HEAD_ONSTACK(wq);
	struct lx_wifi_eapol at_ap = {
		.type = { .type = htons(ETH_P_PAE), .dev = ap->dev, .func = lx_wifi_eapol_rx },
		.wq = &wq,
	};
	struct lx_wifi_eapol at_sta = {
		.type = { .type = htons(ETH_P_PAE), .dev = sta_dev, .func = lx_wifi_eapol_rx },
		.wq = &wq,
	};
	struct lx_wifi_handshaking *h = kzalloc(sizeof(*h), GFP_KERNEL);
	unsigned long until;
	int err;

	if (!h)
		return -ENOMEM;
	h->nl80211 = nl80211;
	h->ap = ap;
	h->sta_dev = sta_dev;
	lx_wpa_pmk(LX_WIFI_PASSPHRASE, (const u8 *)LX_WIFI_SSID, strlen(LX_WIFI_SSID),
		   h->authenticator.pmk);
	lx_wpa_pmk(LX_WIFI_PASSPHRASE, (const u8 *)LX_WIFI_SSID, strlen(LX_WIFI_SSID),
		   h->supplicant.pmk);
	skb_queue_head_init(&at_ap.frames);
	skb_queue_head_init(&at_sta.frames);
	dev_add_pack(&at_ap.type);
	dev_add_pack(&at_sta.type);
	/* Message 1: the access point's nonce, no MIC, the first replay counter. */
	get_random_bytes(h->authenticator.anonce, LX_WPA_NONCE_LEN);
	h->authenticator.replay = 1;
	h->authenticator.frames = 1;
	err = lx_wifi_eapol_send(ap->dev, ap->sta, LX_WPA_INFO_VERSION | LX_WPA_INFO_PAIRWISE |
				 LX_WPA_INFO_ACK, h->authenticator.replay, h->authenticator.anonce,
				 NULL, 0, NULL);
	until = jiffies + LX_WIFI_HANDSHAKE_S * HZ;
	while (!err && !(h->authenticator.done && h->supplicant.done)) {
		struct sk_buff *skb;

		if (!wait_event_timeout(wq, !skb_queue_empty(&at_ap.frames) ||
					    !skb_queue_empty(&at_sta.frames),
					lx_wifi_left(until))) {
			netdev_err(sta_dev, "the handshake did not end in %d s: the station had message %d, the access point sent message %d\n",
				   LX_WIFI_HANDSHAKE_S, h->supplicant.frames,
				   h->authenticator.frames);
			err = -ETIMEDOUT;
			break;
		}
		while (!err && (skb = skb_dequeue(&at_sta.frames))) {
			err = lx_wifi_supplicant(h, skb);
			kfree_skb(skb);
		}
		while (!err && (skb = skb_dequeue(&at_ap.frames))) {
			err = lx_wifi_authenticator(h, skb);
			kfree_skb(skb);
		}
	}
	dev_remove_pack(&at_ap.type);
	dev_remove_pack(&at_sta.type);
	skb_queue_purge(&at_ap.frames);
	skb_queue_purge(&at_sta.frames);
	if (!err)
		netdev_info(sta_dev, "the four-way handshake made: its keys installed, its port open\n");
	kfree_sensitive(h);
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

static int lx_wifi_hwsim(void)
{
	const struct genl_family *nl80211 = lx_wifi_nl80211();
	struct lx_wifi_found found = {};
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
		err = lx_wifi_scan_on(nl80211, devs[1]);
	if (!err) {
		found.bssid = devs[0]->dev_addr;
		lx_wifi_results(devs[1], &found);
		if (found.wanted)
			pr_info("lx-wifi: hwsim: \"%s\" found, beaconed by the first radio and heard by the second\n",
				LX_WIFI_SSID);
		else
			pr_err("lx-wifi: the scan did not find \"%s\" from %pM on %u MHz\n",
			       LX_WIFI_SSID, devs[0]->dev_addr, LX_WIFI_FREQ);
		err = found.wanted ? 0 : -ENOENT;
	}
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
		err = lx_wifi_scan_on(nl80211, devs[1]);
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
