#include "board_pins.h"
#include <stddef.h>
#include <string.h>

/* MSPM0G3507 PM (64-pin LQFP): all 60 GPIOs. Smaller packages simply leave
 * their package-specific pins unbonded; the PINCM numbering is unchanged. */
static const uint8_t k_pincm_a[32] = {
     0,  1,  6,  7,  8,  9, 10, 13,
    18, 19, 20, 21, 33, 34, 35, 36,
    37, 38, 39, 40, 41, 45, 46, 52,
    53, 54, 58, 59,  2,  3,  4,  5,
};
static const uint8_t k_pincm_b[28] = {
    11, 12, 14, 15, 16, 17, 22, 23,
    24, 25, 26, 27, 28, 29, 30, 31,
    32, 42, 43, 44, 47, 48, 49, 50,
    51, 55, 56, 57,
};

/* PA0..31 + PB0..27 owners */
static uint8_t s_own[32 + 28];

/* ADC0 ch for known analog header pins: packed pin_id, ch */
static const uint8_t k_adc_map[][2] = {
    {27, 0},  /* PA27 A0_0 */
    {26, 1},  /* PA26 A0_1 */
    {25, 2},  /* PA25 A0_2 */
    {24, 3},  /* PA24 A0_3 */
    {32 + 24, 5}, /* PB24 A0_5 */
    {32 + 20, 6}, /* PB20 A0_6 */
    {22, 7},  /* PA22 A0_7 */
    {14, 12}, /* PA14 A0_12 — not 0..7 path; keep for docs, reject in ch0-7 API */
};

static int parse_pin(const char *name, unsigned *port_is_b, unsigned *n)
{
    unsigned v;
    if (!name || name[0] != 'P' || (name[1] != 'A' && name[1] != 'B') ||
            name[2] < '0' || name[2] > '9') {
        return -1;
    }
    v = (unsigned)(name[2] - '0');
    if (name[3]) {
        if (name[3] < '0' || name[3] > '9' || name[4]) {
            return -1;
        }
        v = v * 10u + (unsigned)(name[3] - '0');
    }
    *port_is_b = (name[1] == 'B') ? 1u : 0u;
    *n = v;
    return 0;
}

int board_pin_id(const char *name)
{
    unsigned is_b, n;
    const uint8_t *map;
    unsigned limit;
    if (parse_pin(name, &is_b, &n) != 0) {
        return -1;
    }
    map = is_b ? k_pincm_b : k_pincm_a;
    limit = is_b ? sizeof(k_pincm_b) : sizeof(k_pincm_a);
    if (n >= limit || map[n] == 0xFFu) {
        return -1;
    }
    return is_b ? (int)(32u + n) : (int)n;
}

void board_pin_init(void)
{
    static const uint8_t sys[] = {2, 3, 4, 5, 6, 19, 20};
    unsigned i;
    memset(s_own, 0, sizeof(s_own));
    for (i = 0; i < sizeof(sys); i++) {
        s_own[sys[i]] = PIN_OWN_SYS;
    }
    s_own[10] = PIN_OWN_UART0;
    s_own[11] = PIN_OWN_UART0;
    for (i = 14; i <= 17; i++) {
        s_own[32 + i] = PIN_OWN_FLASH;
    }
}

int board_pin_resolve(const char *name, board_pin_t *out)
{
    unsigned is_b, n;
    const uint8_t *map;
    unsigned limit;
    if (!out || parse_pin(name, &is_b, &n) != 0) {
        return -1;
    }
    map = is_b ? k_pincm_b : k_pincm_a;
    limit = is_b ? sizeof(k_pincm_b) : sizeof(k_pincm_a);
    if (n >= limit || map[n] == 0xFFu) {
        return -1;
    }
    out->port = is_b ? GPIOB : GPIOA;
    out->pin = BOARD_PIN_MASK(n);
    out->iomux = map[n];
    return 0;
}

unsigned board_pin_policy(const char *name)
{
    int id = board_pin_id(name);
    unsigned p = 0;
    if (id < 0) {
        return 0;
    }
    /* SYS: crystals, ROSC, SWD */
    if (id == 2 || id == 3 || id == 4 || id == 5 || id == 6 ||
            id == 19 || id == 20) {
        p |= PIN_POL_SYS;
    }
    if (id == 10 || id == 11) {
        p |= PIN_POL_CONSOLE;
    }
    if (id == 32 + 14 || id == 32 + 15 || id == 32 + 16 || id == 32 + 17) {
        p |= PIN_POL_FLASH;
    }
    if (id == 18) {
        p |= PIN_POL_BSL;
    }
    if (id == 14) {
        p |= PIN_POL_LED;
    }
    /* Expansion-header set from DIMENGXING_PINMUX (approx) */
    {
        static const uint8_t hdr[] = {
            0, 1, 2, 7, 8, 9, 12, 13, 14, 15, 16, 17, 18,
            21, 22, 23, 24, 25, 26, 27, 28, 31,
            32 + 3, 32 + 6, 32 + 7, 32 + 8, 32 + 9,
            32 + 17, 32 + 18, 32 + 19, 32 + 20, 32 + 24,
        };
        unsigned i;
        for (i = 0; i < sizeof(hdr); i++) {
            if (hdr[i] == (uint8_t)id) {
                p |= PIN_POL_HEADER;
                break;
            }
        }
    }
    return p;
}

int board_pin_claim_id(int id, uint8_t owner, int force)
{
    uint8_t cur;
    if (id < 0 || id >= (int)sizeof(s_own)) {
        return -1;
    }
    if (owner == PIN_OWN_FREE) {
        return -1;
    }
    /* SYS pins never for app owners */
    if (id == 2 || id == 3 || id == 4 || id == 5 || id == 6 ||
            id == 19 || id == 20) {
        if (owner != PIN_OWN_SYS) {
            return -2;
        }
    }
    /* Console / Flash: only matching owners (or SYS) */
    if ((id == 10 || id == 11) &&
            owner != PIN_OWN_UART0 && owner != PIN_OWN_SYS) {
        return -2;
    }
    if ((id == 32 + 14 || id == 32 + 15 || id == 32 + 16 || id == 32 + 17) &&
            owner != PIN_OWN_FLASH && owner != PIN_OWN_SYS) {
        return -2;
    }
    cur = s_own[id];
    if (cur == PIN_OWN_FREE || cur == owner) {
        s_own[id] = owner;
        return 0;
    }
    if (cur == PIN_OWN_SYS) {
        return -2;
    }
    if (force) {
        s_own[id] = owner;
        return 0;
    }
    return -3;
}

int board_pin_claim(const char *name, uint8_t owner, int force)
{
    return board_pin_claim_id(board_pin_id(name), owner, force);
}

void board_pin_release(const char *name)
{
    int id = board_pin_id(name);
    if (id >= 0 && id < (int)sizeof(s_own) &&
            s_own[id] != PIN_OWN_SYS && s_own[id] != PIN_OWN_UART0 &&
            s_own[id] != PIN_OWN_FLASH) {
        s_own[id] = PIN_OWN_FREE;
    }
}

void board_pin_release_owned(const char *name, uint8_t owner)
{
    int id = board_pin_id(name);
    if (id >= 0 && id < (int)sizeof(s_own) && s_own[id] == owner &&
            owner != PIN_OWN_SYS && owner != PIN_OWN_UART0 &&
            owner != PIN_OWN_FLASH) {
        s_own[id] = PIN_OWN_FREE;
    }
}

void board_pin_release_owner(uint8_t owner)
{
    unsigned i;
    if (owner == PIN_OWN_FREE || owner == PIN_OWN_SYS) {
        return;
    }
    for (i = 0; i < sizeof(s_own); i++) {
        if (s_own[i] == owner) {
            s_own[i] = PIN_OWN_FREE;
        }
    }
}

void board_pin_reset_app_owners(void)
{
    unsigned i;
    for (i = 0; i < sizeof(s_own); i++) {
        uint8_t owner = s_own[i];
        if (owner != PIN_OWN_SYS && owner != PIN_OWN_UART0 &&
                owner != PIN_OWN_FLASH) {
            unsigned bit = i < 32u ? i : i - 32u;
            GPIO_Regs *port = i < 32u ? GPIOA : GPIOB;
            unsigned iomux = i < 32u ? k_pincm_a[i] : k_pincm_b[bit];
            port->DOECLR31_0 = 1u << bit;
            IOMUX->SECCFG.PINCM[iomux] = 0u;
            s_own[i] = PIN_OWN_FREE;
        }
    }
}

int board_pin_owner(const char *name)
{
    int id = board_pin_id(name);
    if (id < 0) {
        return -1;
    }
    return (int)s_own[id];
}

const char *board_pin_owner_str(uint8_t owner)
{
    switch (owner) {
        case PIN_OWN_FREE: return "free";
        case PIN_OWN_GPIO: return "gpio";
        case PIN_OWN_IRQ: return "irq";
        case PIN_OWN_UART0: return "uart0";
        case PIN_OWN_UART1: return "uart1";
        case PIN_OWN_UART2: return "uart2";
        case PIN_OWN_UART3: return "uart3";
        case PIN_OWN_I2C0: return "i2c0";
        case PIN_OWN_I2C1: return "i2c1";
        case PIN_OWN_SPI0: return "spi0";
        case PIN_OWN_SPI1: return "spi1";
        case PIN_OWN_PWM: return "pwm";
        case PIN_OWN_PWM2: return "pwm2";
        case PIN_OWN_PWMCOMP: return "pwmcomp";
        case PIN_OWN_PWMCOMP2: return "pwmcomp2";
        case PIN_OWN_ADC: return "adc";
        case PIN_OWN_OLED: return "oled";
        case PIN_OWN_BTN: return "btn";
        case PIN_OWN_ENC: return "enc";
        case PIN_OWN_CAP: return "cap";
        case PIN_OWN_QEI: return "qei";
        case PIN_OWN_FLASH: return "flash";
        case PIN_OWN_SYS: return "sys";
        case PIN_OWN_UART_APP0: return "uart0_app";
        case PIN_OWN_CAN: return "can";
        case PIN_OWN_DAC: return "dac";
        case PIN_OWN_COMP0: return "comp0";
        case PIN_OWN_COMP1: return "comp1";
        case PIN_OWN_COMP2: return "comp2";
        case PIN_OWN_RTC: return "rtc";
        case PIN_OWN_OPA0: return "opa0";
        case PIN_OWN_OPA1: return "opa1";
        default: return "?";
    }
}

const char *board_pin_errstr(int claim_rc)
{
    if (claim_rc == -1) {
        return "pin";
    }
    if (claim_rc == -2) {
        return "locked";
    }
    if (claim_rc == -3) {
        return "busy";
    }
    return "ok";
}

int board_pin_af(const char *name, unsigned pf, int input_enable)
{
    board_pin_t pin;
    unsigned pol;
    if (board_pin_resolve(name, &pin) != 0 || pf > 9u) {
        return -1;
    }
    pol = board_pin_policy(name);
    if (pol & PIN_POL_SYS) {
        return -2;
    }
    if ((pol & PIN_POL_CONSOLE) || (pol & PIN_POL_FLASH)) {
        return -2;
    }
    if (pf == 0u) {
        /* A disconnected pin must not retain a stale GPIO output driver. */
        pin.port->DOECLR31_0 = pin.pin;
        IOMUX->SECCFG.PINCM[pin.iomux] = 0u;
    } else {
        IOMUX->SECCFG.PINCM[pin.iomux] = IOMUX_PINCM_PC_CONNECTED | pf |
            (input_enable ? IOMUX_PINCM_INENA_ENABLE : 0u);
    }
    return 0;
}

int board_adc_pin_channel(const char *name)
{
    int id = board_pin_id(name);
    unsigned i;
    if (id < 0) {
        return -1;
    }
    for (i = 0; i < sizeof(k_adc_map) / sizeof(k_adc_map[0]); i++) {
        if ((int)k_adc_map[i][0] == id) {
            int ch = (int)k_adc_map[i][1];
            /* exposed capture/read only ch 0..7 */
            return (ch <= 7) ? ch : -1;
        }
    }
    return -1;
}

/*
 * Independent PWM: timer 0=TIMG12, 1=TIMG7 (no pin needed for soft tick TIMG0).
 * Prefer TIMG12 for PA14 LED; TIMG7 for second free channel.
 */
int board_pwm_route(const char *pin, unsigned *pf_out, unsigned *ccp_out,
    int *timer_out)
{
    static const struct {
        char pin[5];
        uint8_t pf;
        uint8_t ccp;
        uint8_t timer; /* 0 TIMG12, 1 TIMG7 */
    } k[] = {
        /* TIMG12 */
        {"PA14", 5, 0, 0},
        {"PB20", 5, 0, 0},
        {"PA25", 4, 1, 0},
        {"PA31", 5, 1, 0},
        {"PB24", 5, 1, 0},
        /* TIMG7 */
        {"PA17", 6, 0, 1},
        {"PA18", 6, 1, 1},
        {"PA23", 7, 0, 1},
        {"PA24", 7, 1, 1},
        {"PA28", 6, 0, 1},
        {"PA7", 7, 1, 1},
        {"PB19", 6, 1, 1},
    };
    unsigned i;
    if (!pin) {
        return -1;
    }
    for (i = 0; i < sizeof(k) / sizeof(k[0]); i++) {
        if (!strcmp(pin, k[i].pin)) {
            if (pf_out) {
                *pf_out = k[i].pf;
            }
            if (ccp_out) {
                *ccp_out = k[i].ccp;
            }
            if (timer_out) {
                *timer_out = (int)k[i].timer;
            }
            return 0;
        }
    }
    return -1;
}

/* Complementary: tima 0=TIMA0, 1=TIMA1; CCP0 + CCP0_CMPL + dead-band. */
int board_pwm_comp_route(const char *hi, const char *lo,
    unsigned *pf_hi, unsigned *pf_lo, int *tima_out)
{
    static const struct {
        char hi[5];
        char lo[5];
        uint8_t pf_h;
        uint8_t pf_l;
        uint8_t tima;
    } k[] = {
        /* TIMA0 */
        {"PA8", "PA22", 5, 7, 0},
        {"PA0", "PA9", 4, 7, 0},
        {"PA21", "PA22", 5, 7, 0},
        {"PB8", "PB9", 4, 5, 0},
        {"PA8", "PA9", 5, 7, 0},
        {"PA0", "PA22", 4, 7, 0},
        {"PA21", "PA9", 5, 7, 0},
        {"PB8", "PA22", 4, 7, 0},
        {"PB8", "PA9", 4, 7, 0},
        {"PA8", "PB9", 5, 5, 0},
        {"PA0", "PB9", 4, 5, 0},
        {"PA21", "PB9", 5, 5, 0},
        /* TIMA1 CCP0 + CCP0_CMPL */
        {"PA15", "PB6", 5, 8, 1},
        {"PA15", "PB24", 5, 7, 1},
        {"PA17", "PA15", 7, 7, 1},
        {"PA17", "PB6", 7, 8, 1},
        {"PA17", "PB24", 7, 7, 1},
        {"PA28", "PA15", 7, 7, 1},
        {"PA28", "PB6", 7, 8, 1},
        {"PA28", "PB24", 7, 7, 1},
        {"PB2", "PA15", 8, 7, 1},
        {"PB2", "PB6", 8, 8, 1},
        {"PB2", "PB24", 8, 7, 1},
    };
    unsigned i;
    if (!hi || !lo || !strcmp(hi, lo)) {
        return -1;
    }
    for (i = 0; i < sizeof(k) / sizeof(k[0]); i++) {
        if (!strcmp(hi, k[i].hi) && !strcmp(lo, k[i].lo)) {
            if (pf_hi) {
                *pf_hi = k[i].pf_h;
            }
            if (pf_lo) {
                *pf_lo = k[i].pf_l;
            }
            if (tima_out) {
                *tima_out = (int)k[i].tima;
            }
            return 0;
        }
    }
    return -1;
}
