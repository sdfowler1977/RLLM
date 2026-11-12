#if !defined(_WIN32)
#define _DEFAULT_SOURCE
#endif
#include <stdint.h>
#include <string.h>
#if defined(_WIN32)
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#else
#include <sys/mman.h>
#endif

struct Sha256 {
    uint32_t state[8];
    uint64_t bits;
    unsigned char block[64];
    unsigned used;
};

struct LoginAccount {
    int enrolled;
    unsigned rounds;
    unsigned char salt[16];
    unsigned char hash[32];
    int factor_live;
    unsigned char factor_key[32];
    unsigned long long factor_last;
    int factor_has_last;
    char factor_code[8];
    int factor_code_len;
    int failures;
    long long locked_until_ms;
    int reset_live;
    unsigned char reset_token[32];
    long long reset_expires_ms;
    int reset_attempts;
};

struct SecretPage {
    unsigned char *page;
    int used;
};

static struct SecretPage g_secret_page[8];
int secret_store_puts = 0;
int secret_store_drops = 0;

static uint32_t rotr(uint32_t x, unsigned n) {
    return (x >> n) | (x << (32u - n));
}

static void sha256_block(struct Sha256 *s, const unsigned char *p) {
    static const uint32_t k[64] = {
        0x428a2f98u, 0x71374491u, 0xb5c0fbcfu, 0xe9b5dba5u, 0x3956c25bu, 0x59f111f1u, 0x923f82a4u, 0xab1c5ed5u,
        0xd807aa98u, 0x12835b01u, 0x243185beu, 0x550c7dc3u, 0x72be5d74u, 0x80deb1feu, 0x9bdc06a7u, 0xc19bf174u,
        0xe49b69c1u, 0xefbe4786u, 0x0fc19dc6u, 0x240ca1ccu, 0x2de92c6fu, 0x4a7484aau, 0x5cb0a9dcu, 0x76f988dau,
        0x983e5152u, 0xa831c66du, 0xb00327c8u, 0xbf597fc7u, 0xc6e00bf3u, 0xd5a79147u, 0x06ca6351u, 0x14292967u,
        0x27b70a85u, 0x2e1b2138u, 0x4d2c6dfcu, 0x53380d13u, 0x650a7354u, 0x766a0abbu, 0x81c2c92eu, 0x92722c85u,
        0xa2bfe8a1u, 0xa81a664bu, 0xc24b8b70u, 0xc76c51a3u, 0xd192e819u, 0xd6990624u, 0xf40e3585u, 0x106aa070u,
        0x19a4c116u, 0x1e376c08u, 0x2748774cu, 0x34b0bcb5u, 0x391c0cb3u, 0x4ed8aa4au, 0x5b9cca4fu, 0x682e6ff3u,
        0x748f82eeu, 0x78a5636fu, 0x84c87814u, 0x8cc70208u, 0x90befffau, 0xa4506cebu, 0xbef9a3f7u, 0xc67178f2u};
    uint32_t w[64];
    for (int i = 0; i < 16; ++i) {
        w[i] = ((uint32_t)p[i * 4] << 24) | ((uint32_t)p[i * 4 + 1] << 16) | ((uint32_t)p[i * 4 + 2] << 8) |
               (uint32_t)p[i * 4 + 3];
    }
    for (int i = 16; i < 64; ++i) {
        uint32_t s0 = rotr(w[i - 15], 7) ^ rotr(w[i - 15], 18) ^ (w[i - 15] >> 3);
        uint32_t s1 = rotr(w[i - 2], 17) ^ rotr(w[i - 2], 19) ^ (w[i - 2] >> 10);
        w[i] = w[i - 16] + s0 + w[i - 7] + s1;
    }
    uint32_t a = s->state[0];
    uint32_t b = s->state[1];
    uint32_t c = s->state[2];
    uint32_t d = s->state[3];
    uint32_t e = s->state[4];
    uint32_t f = s->state[5];
    uint32_t g = s->state[6];
    uint32_t h = s->state[7];
    for (int i = 0; i < 64; ++i) {
        uint32_t s1 = rotr(e, 6) ^ rotr(e, 11) ^ rotr(e, 25);
        uint32_t ch = (e & f) ^ ((~e) & g);
        uint32_t t1 = h + s1 + ch + k[i] + w[i];
        uint32_t s0 = rotr(a, 2) ^ rotr(a, 13) ^ rotr(a, 22);
        uint32_t maj = (a & b) ^ (a & c) ^ (b & c);
        uint32_t t2 = s0 + maj;
        h = g;
        g = f;
        f = e;
        e = d + t1;
        d = c;
        c = b;
        b = a;
        a = t1 + t2;
    }
    s->state[0] += a;
    s->state[1] += b;
    s->state[2] += c;
    s->state[3] += d;
    s->state[4] += e;
    s->state[5] += f;
    s->state[6] += g;
    s->state[7] += h;
}

static void sha256_init(struct Sha256 *s) {
    s->state[0] = 0x6a09e667u;
    s->state[1] = 0xbb67ae85u;
    s->state[2] = 0x3c6ef372u;
    s->state[3] = 0xa54ff53au;
    s->state[4] = 0x510e527fu;
    s->state[5] = 0x9b05688cu;
    s->state[6] = 0x1f83d9abu;
    s->state[7] = 0x5be0cd19u;
    s->bits = 0;
    s->used = 0;
}

static void sha256_update(struct Sha256 *s, const unsigned char *data, int n) {
    if (s == 0 || n <= 0) return;
    if (data == 0) return;
    s->bits += (uint64_t)n * 8ull;
    while (n > 0) {
        unsigned room = 64u - s->used;
        unsigned take = (unsigned)n < room ? (unsigned)n : room;
        memcpy(s->block + s->used, data, take);
        s->used += take;
        data += take;
        n -= (int)take;
        if (s->used == 64u) {
            sha256_block(s, s->block);
            s->used = 0;
        }
    }
}

static void sha256_final(struct Sha256 *s, unsigned char out[32]) {
    unsigned used = s->used;
    s->block[used++] = 0x80u;
    if (used > 56u) {
        while (used < 64u) s->block[used++] = 0;
        sha256_block(s, s->block);
        used = 0;
    }
    while (used < 56u) s->block[used++] = 0;
    uint64_t bits = s->bits;
    for (int i = 0; i < 8; ++i) s->block[63 - i] = (unsigned char)(bits >> (8 * i));
    sha256_block(s, s->block);
    for (int i = 0; i < 8; ++i) {
        out[i * 4] = (unsigned char)(s->state[i] >> 24);
        out[i * 4 + 1] = (unsigned char)(s->state[i] >> 16);
        out[i * 4 + 2] = (unsigned char)(s->state[i] >> 8);
        out[i * 4 + 3] = (unsigned char)s->state[i];
    }
}

static void hmac_sha256(const unsigned char *key, int key_len, const unsigned char *msg, int msg_len,
                        unsigned char out[32]) {
    unsigned char kpad[64];
    memset(kpad, 0, sizeof(kpad));
    if (key_len > 64) {
        struct Sha256 hashed;
        sha256_init(&hashed);
        sha256_update(&hashed, key, key_len);
        sha256_final(&hashed, out);
        memcpy(kpad, out, 32);
    } else if (key_len > 0 && key != 0) {
        memcpy(kpad, key, (size_t)key_len);
    }
    unsigned char ipad[64];
    unsigned char opad[64];
    for (int i = 0; i < 64; ++i) {
        ipad[i] = (unsigned char)(kpad[i] ^ 0x36u);
        opad[i] = (unsigned char)(kpad[i] ^ 0x5cu);
    }
    struct Sha256 inner;
    unsigned char inner_hash[32];
    sha256_init(&inner);
    sha256_update(&inner, ipad, 64);
    if (msg_len > 0 && msg != 0) sha256_update(&inner, msg, msg_len);
    sha256_final(&inner, inner_hash);
    struct Sha256 outer;
    sha256_init(&outer);
    sha256_update(&outer, opad, 64);
    sha256_update(&outer, inner_hash, 32);
    sha256_final(&outer, out);
}

/* The keyed pads are absorbed once; each round then costs one inner and one outer block instead of four. */
static void hmac_sha256_keyed(const struct Sha256 *inner_pad, const struct Sha256 *outer_pad,
                              const unsigned char *msg, int msg_len, unsigned char out[32]) {
    struct Sha256 inner = *inner_pad;
    struct Sha256 outer = *outer_pad;
    unsigned char inner_hash[32];
    if (msg_len > 0 && msg != 0) sha256_update(&inner, msg, msg_len);
    sha256_final(&inner, inner_hash);
    sha256_update(&outer, inner_hash, 32);
    sha256_final(&outer, out);
}

int login_stretch(const unsigned char *secret, int secret_len, const unsigned char *salt, int salt_len,
                  unsigned rounds, unsigned char out[32]) {
    unsigned char msg[260];
    unsigned char u[32];
    unsigned char t[32];
    unsigned char kpad[64];
    unsigned char pad[64];
    struct Sha256 inner_pad;
    struct Sha256 outer_pad;
    if (out == 0 || rounds < 1u) return 0;
    if (secret_len < 0 || salt_len < 0 || secret_len > 256 || salt_len > 256) return 0;
    if (secret_len > 0 && secret == 0) return 0;
    if (salt_len > 0 && salt == 0) return 0;
    memset(kpad, 0, sizeof(kpad));
    if (secret_len > 64) {
        struct Sha256 hashed;
        sha256_init(&hashed);
        sha256_update(&hashed, secret, secret_len);
        sha256_final(&hashed, kpad);
    } else if (secret_len > 0) {
        memcpy(kpad, secret, (size_t)secret_len);
    }
    for (int i = 0; i < 64; ++i) pad[i] = (unsigned char)(kpad[i] ^ 0x36u);
    sha256_init(&inner_pad);
    sha256_update(&inner_pad, pad, 64);
    for (int i = 0; i < 64; ++i) pad[i] = (unsigned char)(kpad[i] ^ 0x5cu);
    sha256_init(&outer_pad);
    sha256_update(&outer_pad, pad, 64);
    memset(kpad, 0, sizeof(kpad));
    memset(pad, 0, sizeof(pad));
    if (salt_len > 0) memcpy(msg, salt, (size_t)salt_len);
    msg[salt_len] = 0;
    msg[salt_len + 1] = 0;
    msg[salt_len + 2] = 0;
    msg[salt_len + 3] = 1;
    hmac_sha256_keyed(&inner_pad, &outer_pad, msg, salt_len + 4, u);
    memcpy(t, u, 32);
    for (unsigned i = 1; i < rounds; ++i) {
        hmac_sha256_keyed(&inner_pad, &outer_pad, u, 32, u);
        for (int b = 0; b < 32; ++b) t[b] = (unsigned char)(t[b] ^ u[b]);
    }
    memcpy(out, t, 32);
    memset(&inner_pad, 0, sizeof(inner_pad));
    memset(&outer_pad, 0, sizeof(outer_pad));
    return 1;
}

static const unsigned k_login_rounds = 600000u;
static const long long k_login_lifetime_ms = 3600000LL;
static const int k_login_lock_after = 5;
static const long long k_login_lock_seconds = 900LL;
static const int k_login_reset_budget = 5;
static const long long k_login_reset_max_ms = 900000LL;

static int same32(const unsigned char *a, const unsigned char *b) {
    unsigned diff = 0u;
    int i;
    if (a == 0 || b == 0) return 0;
    for (i = 0; i < 32; ++i) diff |= (unsigned)(a[i] ^ b[i]);
    return diff == 0u ? 1 : 0;
}

static void wipe_page(unsigned char *page) {
    volatile unsigned char *v = page;
    int i;
    for (i = 0; i < 4096; ++i) v[i] = 0;
}

static unsigned char *lock_page(void) {
#if defined(_WIN32)
    unsigned char *page = (unsigned char *)VirtualAlloc(0, 4096, 0x3000, 0x04);
    if (page == 0) return 0;
    if (VirtualLock(page, 4096) == 0) {
        VirtualFree(page, 0, 0x8000);
        return 0;
    }
    return page;
#else
    unsigned char *page = (unsigned char *)mmap(0, 4096, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    if (page == (unsigned char *)MAP_FAILED) return 0;
    if (mlock(page, 4096) != 0) {
        munmap(page, 4096);
        return 0;
    }
#if defined(MADV_DONTDUMP)
    if (madvise(page, 4096, MADV_DONTDUMP) != 0) {
        munlock(page, 4096);
        munmap(page, 4096);
        return 0;
    }
#endif
    return page;
#endif
}

static void release_page(struct SecretPage *slot) {
    if (slot->page == 0) {
        slot->used = 0;
        return;
    }
    wipe_page(slot->page);
#if defined(_WIN32)
    VirtualUnlock(slot->page, 4096);
    VirtualFree(slot->page, 0, 0x8000);
#else
    munlock(slot->page, 4096);
    munmap(slot->page, 4096);
#endif
    slot->page = 0;
    slot->used = 0;
}

static unsigned session_seal(const unsigned char *token, long long expiry_ms) {
    unsigned seal = 0xa5a5c3c3u;
    int i;
    unsigned long long rest;
    if (token == 0) return 0u;
    for (i = 0; i < 32; ++i) {
        unsigned b = token[i];
        seal ^= b + (seal << 5) + (seal >> 2);
    }
    rest = (unsigned long long)expiry_ms;
    for (i = 0; i < 8; ++i) {
        unsigned b = (unsigned)(rest & 0xffu);
        seal ^= b + (seal << 5) + (seal >> 2);
        rest >>= 8;
    }
    return seal;
}

static void store_record(unsigned char *page, const unsigned char *token, long long expiry_ms) {
    unsigned seal = session_seal(token, expiry_ms);
    memcpy(page, token, 32);
    memcpy(page + 32, &expiry_ms, sizeof(expiry_ms));
    memcpy(page + 40, &seal, sizeof(seal));
}

int secret_store_put(const unsigned char *token, int token_len, long long expiry_ms) {
    int i;
    unsigned char *page;
    if (token == 0 || token_len != 32) return 0;
    for (i = 0; i < 8; ++i) {
        if (!g_secret_page[i].used || g_secret_page[i].page == 0) continue;
        if (!same32(g_secret_page[i].page, token)) continue;
        store_record(g_secret_page[i].page, token, expiry_ms);
        secret_store_puts += 1;
        return 1;
    }
    for (i = 0; i < 8; ++i) {
        if (g_secret_page[i].used) continue;
        page = lock_page();
        if (page == 0) return 0;
        store_record(page, token, expiry_ms);
        g_secret_page[i].page = page;
        g_secret_page[i].used = 1;
        secret_store_puts += 1;
        return 1;
    }
    return 0;
}

int secret_store_find(const unsigned char *token, int token_len, long long *expiry_ms) {
    int i;
    if (token == 0 || token_len != 32 || expiry_ms == 0) return 0;
    for (i = 0; i < 8; ++i) {
        unsigned char *page;
        long long expiry = 0;
        unsigned seal = 0;
        if (!g_secret_page[i].used || g_secret_page[i].page == 0) continue;
        if (!same32(g_secret_page[i].page, token)) continue;
        page = g_secret_page[i].page;
        memcpy(&expiry, page + 32, sizeof(expiry));
        memcpy(&seal, page + 40, sizeof(seal));
        if (session_seal(page, expiry) != seal) return 0;
        *expiry_ms = expiry;
        return 1;
    }
    return 0;
}

int secret_store_load(unsigned char *token, int token_cap, long long *expiry_ms) {
    int i;
    int saw = 0;
    if (token == 0 || token_cap < 32 || expiry_ms == 0) return -1;
    for (i = 0; i < 8; ++i) {
        unsigned char *page;
        long long expiry = 0;
        unsigned seal = 0;
        if (!g_secret_page[i].used || g_secret_page[i].page == 0) continue;
        saw = 1;
        page = g_secret_page[i].page;
        memcpy(&expiry, page + 32, sizeof(expiry));
        memcpy(&seal, page + 40, sizeof(seal));
        if (session_seal(page, expiry) != seal) {
            memset(token, 0, 32);
            return -1;
        }
        memcpy(token, page, 32);
        *expiry_ms = expiry;
        return 1;
    }
    if (saw) {
        memset(token, 0, 32);
        return -1;
    }
    return 0;
}

static void secret_store_clear(void) {
    int i;
    for (i = 0; i < 8; ++i) {
        if (!g_secret_page[i].used || g_secret_page[i].page == 0) continue;
        release_page(&g_secret_page[i]);
        secret_store_drops += 1;
    }
}

int secret_store_drop(const unsigned char *token, int token_len) {
    int i;
    if (token == 0 || token_len != 32) return 0;
    for (i = 0; i < 8; ++i) {
        if (!g_secret_page[i].used || g_secret_page[i].page == 0) continue;
        if (!same32(g_secret_page[i].page, token)) continue;
        release_page(&g_secret_page[i]);
        secret_store_drops += 1;
        return 1;
    }
    return 0;
}

static int write_u(char *out, int cap, int at, unsigned v) {
    char tmp[16];
    int n = 0;
    if (at < 0) return -1;
    if (v == 0u) {
        tmp[n++] = '0';
    } else {
        while (v > 0u && n < 16) {
            tmp[n++] = (char)('0' + (v % 10u));
            v /= 10u;
        }
    }
    if (n <= 0 || at > cap - n - 1) return -1;
    for (int i = 0; i < n; ++i) out[at + i] = tmp[n - 1 - i];
    return at + n;
}

static int write_hex(char *out, int cap, int at, const unsigned char *p, int n) {
    static const char digits[] = "0123456789abcdef";
    if (at < 0 || p == 0 || n < 0) return -1;
    if (at > cap - (n * 2) - 1) return -1;
    for (int i = 0; i < n; ++i) {
        out[at++] = digits[p[i] >> 4];
        out[at++] = digits[p[i] & 0x0f];
    }
    return at;
}

int login_step_code(const unsigned char key[32], unsigned long long unix_seconds, char out[7]) {
    unsigned char msg[8];
    unsigned char mac[32];
    unsigned long long counter;
    unsigned offset;
    unsigned long bin;
    unsigned long code;
    int i;
    if (key == 0 || out == 0) return 0;
    counter = unix_seconds / 30ull;
    for (i = 7; i >= 0; --i) {
        msg[i] = (unsigned char)(counter & 0xffu);
        counter >>= 8;
    }
    hmac_sha256(key, 32, msg, 8, mac);
    offset = mac[31] & 0x0fu;
    bin = ((unsigned long)(mac[offset] & 0x7fu) << 24) | ((unsigned long)mac[offset + 1] << 16) |
          ((unsigned long)mac[offset + 2] << 8) | (unsigned long)mac[offset + 3];
    code = bin % 1000000ul;
    for (i = 5; i >= 0; --i) {
        out[i] = (char)('0' + (code % 10ul));
        code /= 10ul;
    }
    out[6] = 0;
    memset(msg, 0, sizeof(msg));
    memset(mac, 0, sizeof(mac));
    return 1;
}

static int factor_codes_differ(const char *code, int code_len, const char expect[7]) {
    unsigned diff = 0u;
    int i;
    for (i = 0; i < 6; ++i) {
        unsigned char got = 0;
        if (code != 0 && code_len == 6) got = (unsigned char)code[i];
        diff |= (unsigned)got ^ (unsigned)(unsigned char)expect[i];
        if (code == 0 || code_len != 6 || code[i] < '0' || code[i] > '9') diff |= 1u;
    }
    return diff == 0u ? 0 : 1;
}

int login_step_accept(struct LoginAccount *account, unsigned long long unix_seconds) {
    unsigned long long base;
    int window;
    int matched = 0;
    unsigned long long matched_step = 0;
    if (account == 0 || account->factor_live == 0) return 0;
    base = unix_seconds / 30ull;
    for (window = 0; window < 2; ++window) {
        int usable = 1;
        unsigned long long step = base;
        char expect[7];
        if (window == 1) {
            if (base == 0ull) usable = 0;
            else step = base - 1ull;
        }
        if (login_step_code(account->factor_key, step * 30ull, expect) != 1) return 0;
        if (factor_codes_differ(account->factor_code, account->factor_code_len, expect) == 0 && usable != 0) {
            int fresh = !(account->factor_has_last && step <= account->factor_last);
            if (fresh && (matched == 0 || step > matched_step)) {
                matched = 1;
                matched_step = step;
            }
        }
        memset(expect, 0, sizeof(expect));
    }
    if (matched == 0) return 0;
    account->factor_has_last = 1;
    account->factor_last = matched_step;
    return 1;
}

int login_factor_arm(struct LoginAccount *account, const unsigned char key[32], const char *code, int code_len) {
    if (account == 0 || account->enrolled != 1 || key == 0 || code == 0 || code_len != 6) return 0;
    memcpy(account->factor_key, key, 32);
    memcpy(account->factor_code, code, 6);
    account->factor_code[6] = 0;
    account->factor_code_len = 6;
    account->factor_live = 1;
    account->factor_has_last = 0;
    account->factor_last = 0;
    return 1;
}

int login_enroll(struct LoginAccount *account, const unsigned char *secret, int secret_len,
                 const unsigned char salt[16]) {
    if (account == 0 || salt == 0 || secret == 0 || secret_len < 12 || secret_len > 256) return 0;
    memset(account, 0, sizeof(*account));
    if (login_stretch(secret, secret_len, salt, 16, k_login_rounds, account->hash) != 1) return 0;
    memcpy(account->salt, salt, 16);
    account->rounds = k_login_rounds;
    account->enrolled = 1;
    return 1;
}

int login_record(const struct LoginAccount *account, char *out, int cap) {
    const char *prefix = "pbkdf2-sha256$";
    int at = 0;
    if (account == 0 || account->enrolled != 1 || out == 0 || cap < 128) return 0;
    while (*prefix != 0) {
        if (at > cap - 2) return 0;
        out[at++] = *prefix++;
    }
    at = write_u(out, cap, at, account->rounds);
    if (at < 0 || at > cap - 2) return 0;
    out[at++] = '$';
    at = write_hex(out, cap, at, account->salt, 16);
    if (at < 0 || at > cap - 2) return 0;
    out[at++] = '$';
    at = write_hex(out, cap, at, account->hash, 32);
    if (at < 0 || at >= cap) return 0;
    out[at] = 0;
    return at;
}

static int note_login_failure(struct LoginAccount *account, long long now_ms) {
    if (account->failures < 1000000) account->failures += 1;
    if (account->failures >= k_login_lock_after) {
        if (now_ms > 9223372036854775807LL - k_login_lock_seconds * 1000LL) account->locked_until_ms = 9223372036854775807LL;
        else account->locked_until_ms = now_ms + k_login_lock_seconds * 1000LL;
        return 429;
    }
    return 401;
}

int login_submit(struct LoginAccount *account, long long now_ms, const unsigned char *secret, int secret_len,
                 const unsigned char random32[32], unsigned char token_out[32]) {
    unsigned char got[32];
    long long expiry_ms;
    if (account == 0 || account->enrolled != 1 || random32 == 0 || token_out == 0) return 401;
    if (secret == 0 || secret_len < 12 || secret_len > 256) return 401;
    if (account->locked_until_ms != 0 && now_ms < account->locked_until_ms) return 429;
    if (account->locked_until_ms != 0) {
        account->locked_until_ms = 0;
        account->failures = 0;
    }
    if (login_stretch(secret, secret_len, account->salt, 16, account->rounds, got) != 1) return 401;
    if (same32(got, account->hash) != 1) return note_login_failure(account, now_ms);
    if (account->factor_live != 0) {
        unsigned long long unix_seconds = 0ull;
        if (now_ms > 0) unix_seconds = (unsigned long long)(now_ms / 1000LL);
        if (login_step_accept(account, unix_seconds) != 1) return note_login_failure(account, now_ms);
    }
    account->failures = 0;
    account->locked_until_ms = 0;
    if (now_ms > 9223372036854775807LL - k_login_lifetime_ms) return 401;
    expiry_ms = now_ms + k_login_lifetime_ms;
    memcpy(token_out, random32, 32);
    if (secret_store_put(token_out, 32, expiry_ms) != 1) return 500;
    return 200;
}

int login_accept(const unsigned char token[32], long long now_ms) {
    long long expiry_ms = 0;
    if (token == 0) return 401;
    if (secret_store_find(token, 32, &expiry_ms) != 1) return 401;
    if (now_ms >= expiry_ms) return 401;
    return 200;
}

int login_logout(const unsigned char token[32]) {
    if (token == 0) return 401;
    if (secret_store_drop(token, 32) != 1) return 401;
    return 200;
}

static void clear_reset(struct LoginAccount *account) {
    account->reset_live = 0;
    memset(account->reset_token, 0, 32);
    account->reset_expires_ms = 0;
    account->reset_attempts = 0;
}

int login_password_change(struct LoginAccount *account, long long now_ms, const unsigned char *old_secret,
                          int old_len, const unsigned char *new_secret, int new_len, const unsigned char new_salt[16]) {
    unsigned char got[32];
    unsigned char fresh[32];
    unsigned salt_diff;
    int i;
    if (account == 0 || account->enrolled != 1 || new_salt == 0) return 401;
    if (old_secret == 0 || old_len < 12 || old_len > 256) return 401;
    if (account->locked_until_ms != 0 && now_ms < account->locked_until_ms) return 429;
    if (login_stretch(old_secret, old_len, account->salt, 16, account->rounds, got) != 1) return 500;
    if (same32(got, account->hash) != 1) {
        memset(got, 0, sizeof(got));
        return note_login_failure(account, now_ms);
    }
    memset(got, 0, sizeof(got));
    if (new_secret == 0 || new_len < 12 || new_len > 256) return 400;
    if (old_len == new_len) {
        unsigned same = 0u;
        for (i = 0; i < old_len; ++i) same |= (unsigned)(old_secret[i] ^ new_secret[i]);
        if (same == 0u) return 400;
    }
    salt_diff = 0u;
    for (i = 0; i < 16; ++i) salt_diff |= (unsigned)(account->salt[i] ^ new_salt[i]);
    if (salt_diff == 0u) return 400;
    if (login_stretch(new_secret, new_len, new_salt, 16, k_login_rounds, fresh) != 1) return 500;
    memcpy(account->salt, new_salt, 16);
    memcpy(account->hash, fresh, 32);
    account->rounds = k_login_rounds;
    account->failures = 0;
    account->locked_until_ms = 0;
    clear_reset(account);
    memset(fresh, 0, sizeof(fresh));
    secret_store_clear();
    return 200;
}

int login_password_reset_open(struct LoginAccount *account, long long now_ms, const unsigned char token[32],
                              long long ttl_ms) {
    if (account == 0 || account->enrolled != 1 || token == 0) return 401;
    if (ttl_ms <= 0 || ttl_ms > k_login_reset_max_ms) return 400;
    if (now_ms > 9223372036854775807LL - ttl_ms) return 400;
    memcpy(account->reset_token, token, 32);
    account->reset_expires_ms = now_ms + ttl_ms;
    account->reset_attempts = 0;
    account->reset_live = 1;
    return 200;
}

int login_password_reset_finish(struct LoginAccount *account, long long now_ms, const unsigned char token[32],
                                const unsigned char *new_secret, int new_len, const unsigned char new_salt[16]) {
    unsigned diff;
    int i;
    unsigned char fresh[32];
    if (account == 0 || account->enrolled != 1 || token == 0 || account->reset_live != 1) return 401;
    if (now_ms >= account->reset_expires_ms) {
        clear_reset(account);
        return 401;
    }
    diff = 0u;
    for (i = 0; i < 32; ++i) diff |= (unsigned)(account->reset_token[i] ^ token[i]);
    if (diff != 0u) {
        if (account->reset_attempts < 1000000) account->reset_attempts += 1;
        if (account->reset_attempts >= k_login_reset_budget) {
            clear_reset(account);
            return 429;
        }
        return 401;
    }
    if (new_secret == 0 || new_len < 12 || new_len > 256 || new_salt == 0) return 400;
    if (login_stretch(new_secret, new_len, new_salt, 16, k_login_rounds, fresh) != 1) return 500;
    memcpy(account->salt, new_salt, 16);
    memcpy(account->hash, fresh, 32);
    account->rounds = k_login_rounds;
    account->failures = 0;
    account->locked_until_ms = 0;
    clear_reset(account);
    memset(fresh, 0, sizeof(fresh));
    secret_store_clear();
    return 200;
}

int projection_on_sign_in(struct LoginAccount *account, long long now_ms, const unsigned char *secret, int secret_len,
                          const unsigned char salt[16], const unsigned char random32[32], unsigned char token[32]) {
    int status;
    (void)salt;
    status = login_submit(account, now_ms, secret, secret_len, random32, token);
    if (status != 200) return status;
    return login_accept(token, now_ms);
}

int projection_on_sign_out(const unsigned char token[32]) {
    return login_logout(token);
}
