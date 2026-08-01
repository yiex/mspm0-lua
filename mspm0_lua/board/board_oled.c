#include "board_oled.h"

#include <string.h>

#include "board_i2c1.h"
#include "board_lfs.h"
#include "ti_msp_dl_config.h"

/* Built-in 6x8 ASCII 32..90 (space..Z). No _run.fnt needed for Latin text. */
static const uint8_t FONT6[][6] = {
    {0, 0, 0, 0, 0, 0},       /* ' ' */
    {0, 0, 0, 47, 0, 0},      /* '!' */
    {0, 0, 7, 0, 7, 0},       /* '"' */
    {0, 20, 127, 20, 127, 20},/* '#' */
    {0, 36, 42, 127, 42, 18}, /* '$' */
    {0, 98, 100, 8, 19, 35},  /* '%' */
    {0, 54, 73, 85, 34, 80},  /* '&' */
    {0, 0, 5, 3, 0, 0},       /* '\'' */
    {0, 0, 28, 34, 65, 0},    /* '(' */
    {0, 0, 65, 34, 28, 0},    /* ')' */
    {0, 20, 8, 62, 8, 20},    /* '*' */
    {0, 8, 8, 62, 8, 8},      /* '+' */
    {0, 0, 0, 160, 96, 0},    /* ',' */
    {0, 8, 8, 8, 8, 8},       /* '-' */
    {0, 0, 96, 96, 0, 0},     /* '.' */
    {0, 32, 16, 8, 4, 2},     /* '/' */
    {0, 62, 81, 73, 69, 62},  /* '0' */
    {0, 0, 66, 127, 64, 0},   /* '1' */
    {0, 66, 97, 81, 73, 70},  /* '2' */
    {0, 33, 65, 69, 75, 49},  /* '3' */
    {0, 24, 20, 18, 127, 16}, /* '4' */
    {0, 39, 69, 69, 69, 57},  /* '5' */
    {0, 60, 74, 73, 73, 48},  /* '6' */
    {0, 1, 113, 9, 5, 3},     /* '7' */
    {0, 54, 73, 73, 73, 54},  /* '8' */
    {0, 6, 73, 73, 41, 30},   /* '9' */
    {0, 0, 54, 54, 0, 0},     /* ':' */
    {0, 0, 86, 54, 0, 0},     /* ';' */
    {0, 8, 20, 34, 65, 0},    /* '<' */
    {0, 20, 20, 20, 20, 20},  /* '=' */
    {0, 0, 65, 34, 20, 8},    /* '>' */
    {0, 2, 1, 81, 9, 6},      /* '?' */
    {0, 50, 73, 89, 81, 62},  /* '@' */
    {0, 124, 18, 17, 18, 124},/* 'A' */
    {0, 127, 73, 73, 73, 54}, /* 'B' */
    {0, 62, 65, 65, 65, 34},  /* 'C' */
    {0, 127, 65, 65, 34, 28}, /* 'D' */
    {0, 127, 73, 73, 73, 65}, /* 'E' */
    {0, 127, 9, 9, 9, 1},     /* 'F' */
    {0, 62, 65, 73, 73, 122}, /* 'G' */
    {0, 127, 8, 8, 8, 127},   /* 'H' */
    {0, 0, 65, 127, 65, 0},   /* 'I' */
    {0, 32, 64, 65, 63, 1},   /* 'J' */
    {0, 127, 8, 20, 34, 65},  /* 'K' */
    {0, 127, 64, 64, 64, 64}, /* 'L' */
    {0, 127, 2, 12, 2, 127},  /* 'M' */
    {0, 127, 4, 8, 16, 127},  /* 'N' */
    {0, 62, 65, 65, 65, 62},  /* 'O' */
    {0, 127, 9, 9, 9, 6},     /* 'P' */
    {0, 62, 65, 81, 33, 94},  /* 'Q' */
    {0, 127, 9, 25, 41, 70},  /* 'R' */
    {0, 70, 73, 73, 73, 49},  /* 'S' */
    {0, 1, 1, 127, 1, 1},     /* 'T' */
    {0, 63, 64, 64, 64, 63},  /* 'U' */
    {0, 31, 32, 64, 32, 31},  /* 'V' */
    {0, 63, 64, 56, 64, 63},  /* 'W' */
    {0, 99, 20, 8, 20, 99},   /* 'X' */
    {0, 7, 8, 112, 8, 7},     /* 'Y' */
    {0, 97, 81, 73, 69, 67},  /* 'Z' */
};

#define FONT_BANK_MAX 24

static struct {
    uint8_t code;
    uint8_t g[6];
} s_bank[FONT_BANK_MAX];
static uint8_t s_bank_n;

/* 16x16 CJK/any: codepoint + 32B row-major (16 rows × 2 bytes, MSB left). */
static struct {
    uint16_t code;
    uint8_t bmp[BOARD_OLED_CJK_BYTES];
} s_cjk[BOARD_OLED_CJK_MAX];
static uint8_t s_cjk_n;

static board_i2c1_t s_bus;
static uint8_t s_addr = 0x3c;
static uint8_t s_open;
static uint8_t s_x;
static uint8_t s_page;

static int wr(const uint8_t *d, size_t n)
{
    int st = board_i2c1_write(&s_bus, s_addr, d, n);
    if (st != 0 && s_open) {
        /* A failed retry means the device is no longer connected.  Latch the
         * state so oled.ready() can drive a full open/init retry. */
        board_i2c1_close(&s_bus);
        s_open = 0;
    }
    return st;
}

static int cmd1(uint8_t c)
{
    uint8_t b[2] = {0x00, c};
    return wr(b, 2);
}

static int data6(const uint8_t g[6])
{
    uint8_t b[7];
    b[0] = 0x40;
    memcpy(b + 1, g, 6);
    return wr(b, 7);
}

static const uint8_t *glyph_builtin(char c)
{
    unsigned u = (unsigned char)c;
    if (u >= 32u && u <= 90u) {
        return FONT6[u - 32u];
    }
    if (u >= 'a' && u <= 'z') {
        return FONT6[u - (unsigned)'a' + ((unsigned)'A' - 32u)];
    }
    return NULL;
}

static const uint8_t *glyph_bank(uint8_t code)
{
    uint8_t i;
    for (i = 0; i < s_bank_n; i++) {
        if (s_bank[i].code == code) {
            return s_bank[i].g;
        }
    }
    return NULL;
}

static const uint8_t *glyph(char c)
{
    const uint8_t *g = glyph_builtin(c);
    if (g) {
        return g;
    }
    g = glyph_bank((uint8_t)c);
    if (g) {
        return g;
    }
    /* lowercase → uppercase bank / still miss → blank */
    if (c >= 'a' && c <= 'z') {
        g = glyph_bank((uint8_t)(c - 'a' + 'A'));
        if (g) {
            return g;
        }
    }
    return FONT6[0];
}

void board_oled_glyph_clear(void)
{
    s_bank_n = 0;
}

int board_oled_glyph_set(uint8_t code, const uint8_t data6_in[6])
{
    uint8_t i;
    if (!data6_in) {
        return -1;
    }
    /* overwrite built-in? allow for customization */
    for (i = 0; i < s_bank_n; i++) {
        if (s_bank[i].code == code) {
            memcpy(s_bank[i].g, data6_in, 6);
            return 0;
        }
    }
    if (s_bank_n >= FONT_BANK_MAX) {
        return -2;
    }
    s_bank[s_bank_n].code = code;
    memcpy(s_bank[s_bank_n].g, data6_in, 6);
    s_bank_n++;
    return 0;
}

int board_oled_has_glyph(uint8_t code)
{
    if (glyph_builtin((char)code)) {
        return 1;
    }
    if (glyph_bank(code)) {
        return 1;
    }
    if (code >= 'a' && code <= 'z' && glyph_bank((uint8_t)(code - 'a' + 'A'))) {
        return 1;
    }
    return 0;
}

int board_oled_font_load(const char *path)
{
    uint8_t buf[512];
    int n;
    int i;
    int loaded = 0;
    if (!path || !path[0]) {
        return -1;
    }
    n = board_lfs_read_file(path, (char *)buf, sizeof(buf));
    if (n < 4) {
        return -1;
    }
    if (buf[0] != 'F' || buf[1] != '6' || buf[2] != 1) {
        return -2;
    }
    {
        uint8_t count = buf[3];
        size_t need = 4u + (size_t)count * 7u;
        if ((size_t)n < need) {
            return -3;
        }
        for (i = 0; i < (int)count; i++) {
            size_t off = 4u + (size_t)i * 7u;
            if (board_oled_glyph_set(buf[off], &buf[off + 1]) == 0) {
                loaded++;
            }
        }
    }
    return loaded;
}

int board_oled_open(const char *scl, const char *sda, uint8_t addr7, uint32_t hz)
{
    static const uint8_t init[] = {
        0xae, 0xd5, 0x80, 0xa8, 0x3f, 0xd3, 0x00, 0x40, 0x8d, 0x14, 0x20, 0x02,
        0xa1, 0xc8, 0xda, 0x12, 0x81, 0xcf, 0xd9, 0xf1, 0xdb, 0x40, 0xa4, 0xa6,
        0xaf,
    };
    uint8_t addrs[2];
    int naddr;
    int ai;
    size_t i;
    int try;
    int ok;
    if (!scl) {
        scl = "PA15";
    }
    if (!sda) {
        sda = "PA16";
    }
    if (!addr7) {
        addr7 = 0x3c;
    }
    if (!hz) {
        hz = 100000u;
    }
    if (s_open) {
        board_oled_close();
    }
    addrs[0] = addr7;
    naddr = 1;
    /* default 0x3C: also try 0x3D modules without caller change */
    if (addr7 == 0x3cu) {
        addrs[1] = 0x3du;
        naddr = 2;
    }

    for (ai = 0; ai < naddr; ai++) {
        s_addr = addrs[ai];
        for (try = 0; try < 3; try++) {
            if (board_i2c1_open(&s_bus, scl, sda, hz) != 0) {
                continue;
            }
            ok = 1;
            for (i = 0; i < sizeof(init); i++) {
                if (cmd1(init[i]) != 0) {
                    ok = 0;
                    break;
                }
            }
            if (ok) {
                s_open = 1;
                s_x = 0;
                s_page = 0;
                if (board_oled_clear() == 0) {
                    (void)board_oled_font_load("_run.fnt");
                    (void)board_oled_font16_load("_run.f16");
                    return 0;
                }
                s_open = 0;
            }
            board_i2c1_close(&s_bus);
            delay_cycles(80000);
        }
    }
    return -2;
}

void board_oled_close(void)
{
    if (s_open) {
        (void)cmd1(0xae);
    }
    board_i2c1_close(&s_bus);
    s_open = 0;
}

int board_oled_ready(void)
{
    return s_open && board_i2c1_ready(&s_bus);
}

int board_oled_fill(uint8_t v)
{
    uint8_t page;
    uint8_t z[9];
    int i;
    if (!s_open) {
        return -1;
    }
    z[0] = 0x40;
    for (i = 1; i < 9; i++) {
        z[i] = v;
    }
    for (page = 0; page < 8; page++) {
        uint8_t col = 0;
        if (cmd1((uint8_t)(0xb0 + page)) || cmd1(0x00) || cmd1(0x10)) {
            return -1;
        }
        while (col < 128) {
            size_t n = (128 - col) > 8 ? 8 : (size_t)(128 - col);
            if (wr(z, n + 1) != 0) {
                return -1;
            }
            col = (uint8_t)(col + n);
        }
    }
    s_x = 0;
    s_page = 0;
    return 0;
}

int board_oled_clear(void)
{
    return board_oled_fill(0);
}

int board_oled_cursor(uint8_t x, uint8_t page)
{
    uint8_t b[4];
    if (!s_open || page > 7 || x > 127) {
        return -1;
    }
    b[0] = 0x00;
    b[1] = (uint8_t)(0xb0 + page);
    b[2] = (uint8_t)(x & 0x0f);
    b[3] = (uint8_t)(0x10 + (x >> 4));
    if (wr(b, sizeof(b)) != 0) {
        return -1;
    }
    s_x = x;
    s_page = page;
    return 0;
}

int board_oled_putc(char c)
{
    if (!s_open) {
        return -1;
    }
    if (s_x > 122) {
        return -1;
    }
    if (data6(glyph(c)) != 0) {
        return -1;
    }
    s_x = (uint8_t)(s_x + 6);
    return 0;
}

int board_oled_puts(const char *s)
{
    if (!s) {
        return -1;
    }
    while (*s) {
        if (board_oled_putc(*s++) != 0) {
            return -1;
        }
    }
    return 0;
}

int board_oled_num(uint8_t x, uint8_t page, int32_t value, uint8_t dec)
{
    char buf[8];
    int neg = 0;
    uint32_t n;
    uint32_t ip, frac;
    int i = 0;
    int digs, w;

    if (dec > 2) {
        dec = 2;
    }
    if (value < 0) {
        neg = 1;
        /* -(INT32_MIN) overflows signed C; form the magnitude safely. */
        n = (uint32_t)(-(value + 1)) + 1u;
    } else {
        n = (uint32_t)value;
    }
    if (dec == 0) {
        ip = n;
        frac = 0;
    } else if (dec == 1) {
        ip = n / 10u;
        frac = n % 10u;
    } else {
        ip = n / 100u;
        frac = n % 100u;
    }
    if (ip > (dec ? 9999 : 999999)) {
        ip = dec ? 9999u : 999999u;
    }

    buf[i++] = neg ? '-' : ' ';
    digs = 1;
    if (ip >= 100000) {
        digs = 6;
    } else if (ip >= 10000) {
        digs = 5;
    } else if (ip >= 1000) {
        digs = 4;
    } else if (ip >= 100) {
        digs = 3;
    } else if (ip >= 10) {
        digs = 2;
    }
    w = digs;
    while (w--) {
        int div = 1;
        int k;
        for (k = 0; k < w; k++) {
            div *= 10;
        }
        buf[i++] = (char)('0' + (ip / div) % 10);
    }
    if (dec) {
        buf[i++] = '.';
        if (dec == 1) {
            buf[i++] = (char)('0' + frac);
        } else {
            buf[i++] = (char)('0' + (frac / 10));
            buf[i++] = (char)('0' + (frac % 10));
        }
    }
    buf[i] = 0;
    if (board_oled_cursor(x, page) != 0) {
        return -1;
    }
    return board_oled_puts(buf);
}

void board_oled_cjk_clear(void)
{
    s_cjk_n = 0;
}

int board_oled_cjk_set(uint16_t code, const uint8_t bmp32[32])
{
    uint8_t i;
    if (!bmp32) {
        return -1;
    }
    for (i = 0; i < s_cjk_n; i++) {
        if (s_cjk[i].code == code) {
            memcpy(s_cjk[i].bmp, bmp32, BOARD_OLED_CJK_BYTES);
            return 0;
        }
    }
    if (s_cjk_n >= BOARD_OLED_CJK_MAX) {
        return -2;
    }
    s_cjk[s_cjk_n].code = code;
    memcpy(s_cjk[s_cjk_n].bmp, bmp32, BOARD_OLED_CJK_BYTES);
    s_cjk_n++;
    return 0;
}

int board_oled_has_cjk(uint16_t code)
{
    uint8_t i;
    for (i = 0; i < s_cjk_n; i++) {
        if (s_cjk[i].code == code) {
            return 1;
        }
    }
    return 0;
}

static const uint8_t *cjk_find(uint16_t code)
{
    uint8_t i;
    for (i = 0; i < s_cjk_n; i++) {
        if (s_cjk[i].code == code) {
            return s_cjk[i].bmp;
        }
    }
    return NULL;
}

/* FN pack: 'F''N' ver=1 count_lo count_hi; then count × (u16 LE code + 32 bmp). */
int board_oled_font16_load(const char *path)
{
    /* max 32 glyphs × 34 + 4 header = 1092; read in chunks via open */
    uint8_t hdr[4];
    int n;
    int count;
    int i;
    int loaded = 0;
    uint8_t rec[34];

    if (!path || !path[0]) {
        return -1;
    }
    if (board_lfs_read_open(path) < 0) {
        return -1;
    }
    n = board_lfs_read_chunk(hdr, 4);
    if (n != 4 || hdr[0] != 'F' || hdr[1] != 'N' || hdr[2] != 1) {
        board_lfs_read_close();
        return -2;
    }
    /* Format: 'F' 'N' ver=1 count(u8) */
    count = (int)hdr[3];
    if (count > BOARD_OLED_CJK_MAX) {
        count = BOARD_OLED_CJK_MAX;
    }
    for (i = 0; i < count; i++) {
        n = board_lfs_read_chunk(rec, 34);
        if (n != 34) {
            break;
        }
        {
            uint16_t code = (uint16_t)rec[0] | ((uint16_t)rec[1] << 8);
            if (board_oled_cjk_set(code, &rec[2]) == 0) {
                loaded++;
            }
        }
    }
    board_lfs_read_close();
    return loaded;
}

/* Decode one UTF-8 codepoint; advance *pp. Returns 0 on end/invalid. */
static uint16_t utf8_next(const char **pp)
{
    const uint8_t *p = (const uint8_t *)*pp;
    uint32_t cp;
    if (!p || !*p) {
        return 0;
    }
    if (p[0] < 0x80u) {
        *pp = (const char *)(p + 1);
        return (uint16_t)p[0];
    }
    if ((p[0] & 0xe0u) == 0xc0u && p[1]) {
        cp = ((uint32_t)(p[0] & 0x1fu) << 6) | (uint32_t)(p[1] & 0x3fu);
        *pp = (const char *)(p + 2);
        return cp > 0xffffu ? 0xfffdu : (uint16_t)cp;
    }
    if ((p[0] & 0xf0u) == 0xe0u && p[1] && p[2]) {
        cp = ((uint32_t)(p[0] & 0x0fu) << 12) | ((uint32_t)(p[1] & 0x3fu) << 6) |
            (uint32_t)(p[2] & 0x3fu);
        *pp = (const char *)(p + 3);
        return cp > 0xffffu ? 0xfffdu : (uint16_t)cp;
    }
    if ((p[0] & 0xf8u) == 0xf0u && p[1] && p[2] && p[3]) {
        *pp = (const char *)(p + 4);
        return 0xfffdu; /* outside BMP — box */
    }
    *pp = (const char *)(p + 1);
    return 0xfffdu;
}

/* Convert row-major 16x16 bmp to two page strips (page0 top, page1 bottom), 16 cols each. */
static void cjk_to_pages(const uint8_t bmp[32], uint8_t top[16], uint8_t bot[16])
{
    int col;
    int row;
    for (col = 0; col < 16; col++) {
        uint8_t t = 0;
        uint8_t b = 0;
        for (row = 0; row < 8; row++) {
            int byte_i = row * 2 + (col >> 3);
            int bit = 7 - (col & 7);
            if (bmp[byte_i] & (1u << bit)) {
                t |= (uint8_t)(1u << row);
            }
        }
        for (row = 0; row < 8; row++) {
            int r = row + 8;
            int byte_i = r * 2 + (col >> 3);
            int bit = 7 - (col & 7);
            if (bmp[byte_i] & (1u << bit)) {
                b |= (uint8_t)(1u << row);
            }
        }
        top[col] = t;
        bot[col] = b;
    }
}

static void cjk_box(uint8_t top[16], uint8_t bot[16])
{
    int i;
    for (i = 0; i < 16; i++) {
        top[i] = (i == 0 || i == 15) ? 0xffu : 0x81u;
        bot[i] = (i == 0 || i == 15) ? 0xffu : 0x81u;
    }
}

static int write_cols(uint8_t x, uint8_t page, const uint8_t *cols, int n)
{
    uint8_t buf[17];
    int i;
    if (board_oled_cursor(x, page) != 0) {
        return -1;
    }
    buf[0] = 0x40;
    for (i = 0; i < n; i++) {
        buf[1 + i] = cols[i];
    }
    return wr(buf, (size_t)(1 + n));
}

/* Scope trace on pages 0..6: fixed 0..4095 scale, robust rising trigger. */
int board_oled_wave(const uint16_t *s, size_t n)
{
    uint8_t y[128];
    uint8_t page_data[129];
    uint16_t hist[16] = {0};
    uint16_t lo, hi;
    uint16_t dc = 0;
    size_t i, x, start = 0;
    size_t window;
    int active;
    uint8_t page;

    if (!s_open || !s || n < 2u) {
        return -1;
    }
    window = n < 128u ? n : 128u;
    for (i = 0; i < n; i++) {
        uint16_t v = (uint16_t)(s[i] & 0x0FFFu);
        hist[v >> 8]++;
    }
    {
        size_t trim = n >> 4;
        size_t count = 0;
        uint8_t lb = 0, hb = 15;
        for (i = 0; i < 16u; i++) {
            count += hist[i];
            if (count > trim) {
                lb = (uint8_t)i;
                break;
            }
        }
        count = 0;
        for (i = 16u; i > 0u; i--) {
            count += hist[i - 1u];
            if (count > trim) {
                hb = (uint8_t)(i - 1u);
                break;
            }
        }
        lo = (uint16_t)(lb << 8);
        hi = (uint16_t)((hb << 8) | 0xffu);
    }
    active = (uint16_t)(hi - lo) >= 384u;
    if (active) {
        uint16_t mid = (uint16_t)(((uint32_t)lo + hi) >> 1);
        uint16_t h = (uint16_t)((hi - lo) >> 3);
        uint16_t trig_lo, trig_hi;
        int armed;
        size_t pre = window > 16u ? 16u : window >> 2;
        size_t last_trigger = n - window + pre;
        if (h < 16u) {
            h = 16u;
        }
        trig_lo = mid > h ? (uint16_t)(mid - h) : 0u;
        trig_hi = mid < (uint16_t)(4095u - h)
            ? (uint16_t)(mid + h) : 4095u;
        armed = (s[0] & 0x0FFFu) <= trig_lo;
        for (i = 1; i < n; i++) {
            uint16_t v = (uint16_t)(s[i] & 0x0FFFu);
            if (v <= trig_lo) {
                armed = 1;
            } else if (armed && v >= trig_hi && i >= pre && i <= last_trigger) {
                start = i - pre;
                break;
            }
        }
    } else {
        uint32_t sum = 0;
        size_t count = 0;
        for (i = 0; i < n; i++) {
            uint16_t v = (uint16_t)(s[i] & 0x0FFFu);
            if (v >= lo && v <= hi) {
                sum += v;
                count++;
            }
        }
        dc = count ? (uint16_t)(sum / count) : (uint16_t)((lo + hi) >> 1);
    }
    for (i = 0; i < 128u; i++) {
        size_t si = start + (i * window) / 128u;
        uint16_t v = dc;
        if (active) {
            uint16_t a = (uint16_t)(s[si ? si - 1u : si] & 0x0FFFu);
            uint16_t b = (uint16_t)(s[si] & 0x0FFFu);
            uint16_t c = (uint16_t)(s[si + 1u < n ? si + 1u : si] & 0x0FFFu);
            if (a > b) { uint16_t t = a; a = b; b = t; }
            if (b > c) { uint16_t t = b; b = c; c = t; }
            if (a > b) { uint16_t t = a; a = b; b = t; }
            v = b;
        }
        y[i] = (uint8_t)(((uint32_t)(4095u - v) * 55u) / 4095u);
    }
    for (page = 0; page < 7u; page++) {
        uint8_t y0 = (uint8_t)(page * 8u);
        page_data[0] = 0x40;
        for (x = 0; x < 128u; x++) {
            int yy = (int)y[x];
            int prev = x ? (int)y[x - 1u] : yy;
            int a = yy < prev ? yy : prev;
            int b = yy > prev ? yy : prev;
            int bit;
            page_data[x + 1u] = 0;
            if (a < (int)y0) a = (int)y0;
            if (b > (int)y0 + 7) b = (int)y0 + 7;
            for (bit = a; bit <= b; bit++) {
                page_data[x + 1u] |= (uint8_t)(1u << (bit - (int)y0));
            }
        }
        if (board_oled_cursor(0, page) != 0 ||
                wr(page_data, sizeof(page_data)) != 0) {
            return -1;
        }
    }
    return 0;
}

int board_oled_text(uint8_t x, uint8_t page, const char *utf8)
{
    const char *p = utf8;
    uint8_t top[16];
    uint8_t bot[16];
    if (!s_open || !utf8 || page > 6) {
        return -1;
    }
    while (*p) {
        uint16_t cp = utf8_next(&p);
        const uint8_t *bmp;
        if (cp == 0) {
            break;
        }
        if (x > 112u) {
            return -1;
        }
        if (cp == (uint16_t)' ') {
            memset(top, 0, 16);
            memset(bot, 0, 16);
        } else {
            bmp = cjk_find(cp);
            if (bmp) {
                cjk_to_pages(bmp, top, bot);
            } else {
                cjk_box(top, bot);
            }
        }
        if (write_cols(x, page, top, 16) != 0) {
            return -1;
        }
        if (write_cols(x, (uint8_t)(page + 1), bot, 16) != 0) {
            return -1;
        }
        x = (uint8_t)(x + 16);
        s_x = x;
        s_page = page;
    }
    return 0;
}
