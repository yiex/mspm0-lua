#include "board_enc.h"
#include "board_irq.h"
#include "board_pins.h"
#include "board_reg.h"

#include <string.h>

/* Full-step gray transitions → +1/-1, illegal → 0 */
static const int8_t k_qtab[16] = {
    0, +1, -1, 0, -1, 0, 0, +1, +1, 0, 0, -1, 0, -1, +1, 0,
};

typedef struct {
    uint8_t used;
    volatile int32_t pos;
    int32_t mark;
    int32_t mark_cps;
    uint32_t mark_ms;
    volatile uint8_t ab; /* bit1=A bit0=B last */
    GPIO_Regs *porta;
    GPIO_Regs *portb;
    uint32_t pina;
    uint32_t pinb;
    char name_a[8];
    char name_b[8];
} enc_slot_t;

static enc_slot_t s_enc[BOARD_ENC_MAX];

static void cpy7(char *d, const char *s)
{
    size_t n = 0;
    while (s[n] && n < 7) {
        d[n] = s[n];
        n++;
    }
    d[n] = 0;
}

static uint8_t read_ab(const enc_slot_t *e)
{
    uint8_t a = board_reg_gpio_read(e->porta, e->pina) ? 1u : 0u;
    uint8_t b = board_reg_gpio_read(e->portb, e->pinb) ? 1u : 0u;
    return (uint8_t)((a << 1) | b);
}

void board_enc_isr_tick(void)
{
    int i;
    for (i = 0; i < BOARD_ENC_MAX; i++) {
        enc_slot_t *e = &s_enc[i];
        uint8_t now, idx;
        if (!e->used) {
            continue;
        }
        now = read_ab(e);
        if (now == e->ab) {
            continue;
        }
        idx = (uint8_t)((e->ab << 2) | now);
        e->pos += k_qtab[idx & 15u];
        e->ab = now;
    }
}

void board_enc_poll(void)
{
    board_enc_isr_tick();
}

int board_enc_open(const char *pin_a, const char *pin_b)
{
    board_pin_t pa, pb;
    int id = -1, i;
    if (!pin_a || !pin_b || strcmp(pin_a, pin_b) == 0) {
        return -1;
    }
    if (board_pin_resolve(pin_a, &pa) != 0) {
        return -1;
    }
    if (board_pin_resolve(pin_b, &pb) != 0) {
        return -1;
    }
    for (i = 0; i < BOARD_ENC_MAX; i++) {
        if (!s_enc[i].used) {
            id = i;
            break;
        }
    }
    if (id < 0) {
        return -1;
    }
    if (board_pin_claim(pin_a, PIN_OWN_ENC, 0) != 0 ||
            board_pin_claim(pin_b, PIN_OWN_ENC, 0) != 0) {
        board_pin_release_owned(pin_a, PIN_OWN_ENC);
        board_pin_release_owned(pin_b, PIN_OWN_ENC);
        return -2;
    }
    if (board_gpio_irq_enable(pin_a, 2) != 0 ||
            board_gpio_irq_enable(pin_b, 2) != 0) {
        board_gpio_irq_disable(pin_a);
        board_gpio_irq_disable(pin_b);
        board_pin_release_owned(pin_a, PIN_OWN_ENC);
        board_pin_release_owned(pin_b, PIN_OWN_ENC);
        return -3;
    }

    memset(&s_enc[id], 0, sizeof(s_enc[id]));
    s_enc[id].used = 1;
    s_enc[id].porta = pa.port;
    s_enc[id].portb = pb.port;
    s_enc[id].pina = pa.pin;
    s_enc[id].pinb = pb.pin;
    cpy7(s_enc[id].name_a, pin_a);
    cpy7(s_enc[id].name_b, pin_b);
    s_enc[id].ab = read_ab(&s_enc[id]);
    s_enc[id].pos = 0;
    s_enc[id].mark = 0;
    s_enc[id].mark_cps = 0;
    s_enc[id].mark_ms = board_millis();
    return id;
}

void board_enc_close(int id)
{
    if (id < 0 || id >= BOARD_ENC_MAX || !s_enc[id].used) {
        return;
    }
    board_gpio_irq_disable(s_enc[id].name_a);
    board_gpio_irq_disable(s_enc[id].name_b);
    board_pin_release_owned(s_enc[id].name_a, PIN_OWN_ENC);
    board_pin_release_owned(s_enc[id].name_b, PIN_OWN_ENC);
    s_enc[id].used = 0;
}

void board_enc_set(int id, int32_t pos)
{
    if (id < 0 || id >= BOARD_ENC_MAX || !s_enc[id].used) {
        return;
    }
    s_enc[id].pos = pos;
    s_enc[id].mark = pos;
    s_enc[id].mark_cps = pos;
    s_enc[id].mark_ms = board_millis();
}

int32_t board_enc_pos(int id)
{
    if (id < 0 || id >= BOARD_ENC_MAX || !s_enc[id].used) {
        return 0;
    }
    return s_enc[id].pos;
}

int32_t board_enc_delta(int id)
{
    int32_t p, d;
    if (id < 0 || id >= BOARD_ENC_MAX || !s_enc[id].used) {
        return 0;
    }
    p = board_enc_pos(id);
    d = p - s_enc[id].mark;
    s_enc[id].mark = p;
    return d;
}

int32_t board_enc_cps(int id)
{
    enc_slot_t *e;
    int32_t p, d;
    uint32_t now, dt;
    if (id < 0 || id >= BOARD_ENC_MAX || !s_enc[id].used) {
        return 0;
    }
    e = &s_enc[id];
    now = board_millis();
    dt = (uint32_t)(now - e->mark_ms);
    p = e->pos;
    d = p - e->mark_cps;
    e->mark_cps = p;
    e->mark_ms = now;
    if (dt == 0u) {
        return 0;
    }
    /* counts/s = d * 1000 / dt */
    return (int32_t)(((int64_t)d * 1000) / (int32_t)dt);
}
