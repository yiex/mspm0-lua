#include "board_btn.h"
#include "board_irq.h"
#include "board_pins.h"
#include "board_reg.h"

#include <string.h>

typedef struct {
    uint8_t used;
    uint8_t stable;   /* 1 = pressed (active low raw 0) */
    uint8_t raw_last;
    uint8_t long_fired;
    uint8_t ev_q[4];
    uint8_t ev_n;
    uint32_t debounce_ms;
    uint32_t long_ms;
    uint32_t edge_ms;
    uint32_t press_ms;
    GPIO_Regs *port;
    uint32_t pin;
    char name[8];
} btn_slot_t;

static btn_slot_t s_btn[BOARD_BTN_MAX];

static void ev_push(btn_slot_t *b, uint8_t ev)
{
    if (b->ev_n < sizeof(b->ev_q)) {
        b->ev_q[b->ev_n++] = ev;
    }
}

int board_btn_open(const char *pin, uint32_t debounce_ms, uint32_t long_ms)
{
    board_pin_t p;
    int i, id = -1;
    if (board_pin_resolve(pin, &p) != 0) {
        return -1;
    }
    if (debounce_ms < 1u) {
        debounce_ms = 10u;
    }
    if (long_ms < debounce_ms) {
        long_ms = 600u;
    }
    for (i = 0; i < BOARD_BTN_MAX; i++) {
        if (!s_btn[i].used) {
            id = i;
            break;
        }
    }
    if (id < 0) {
        return -1;
    }
    if (board_pin_claim(pin, PIN_OWN_BTN, 0) != 0) {
        return -2;
    }
    p.port->DOECLR31_0 = p.pin;
    board_reg_iomux_gpio_in_pu(p.iomux);

    memset(&s_btn[id], 0, sizeof(s_btn[id]));
    s_btn[id].used = 1;
    s_btn[id].port = p.port;
    s_btn[id].pin = p.pin;
    s_btn[id].debounce_ms = debounce_ms;
    s_btn[id].long_ms = long_ms;
    s_btn[id].raw_last = 1;
    s_btn[id].stable = 0;
    {
        size_t n = 0;
        while (pin[n] && n < 7) {
            s_btn[id].name[n] = pin[n];
            n++;
        }
        s_btn[id].name[n] = 0;
    }
    return id;
}

void board_btn_close(int id)
{
    if (id < 0 || id >= BOARD_BTN_MAX || !s_btn[id].used) {
        return;
    }
    board_pin_release_owned(s_btn[id].name, PIN_OWN_BTN);
    s_btn[id].used = 0;
}

int board_btn_scan(void)
{
    uint32_t now = board_millis();
    int n_ev = 0;
    int i;
    for (i = 0; i < BOARD_BTN_MAX; i++) {
        btn_slot_t *b = &s_btn[i];
        uint8_t raw;
        if (!b->used) {
            continue;
        }
        /* active low */
        raw = board_reg_gpio_read(b->port, b->pin) ? 1u : 0u;
        if (raw != b->raw_last) {
            b->raw_last = raw;
            b->edge_ms = now;
        }
        if ((uint32_t)(now - b->edge_ms) < b->debounce_ms) {
            continue;
        }
        {
            uint8_t pressed = raw ? 0u : 1u; /* low = pressed */
            if (pressed != b->stable) {
                b->stable = pressed;
                if (pressed) {
                    b->press_ms = now;
                    b->long_fired = 0;
                    ev_push(b, BOARD_BTN_PRESS);
                    n_ev++;
                } else {
                    ev_push(b, BOARD_BTN_RELEASE);
                    n_ev++;
                }
            } else if (pressed && !b->long_fired &&
                    (uint32_t)(now - b->press_ms) >= b->long_ms) {
                b->long_fired = 1;
                ev_push(b, BOARD_BTN_LONG);
                n_ev++;
            }
        }
    }
    return n_ev;
}

int board_btn_event(int id)
{
    btn_slot_t *b;
    uint8_t ev;
    if (id < 0 || id >= BOARD_BTN_MAX || !s_btn[id].used || s_btn[id].ev_n == 0) {
        return BOARD_BTN_NONE;
    }
    b = &s_btn[id];
    ev = b->ev_q[0];
    memmove(b->ev_q, b->ev_q + 1, b->ev_n - 1);
    b->ev_n--;
    return (int)ev;
}

int board_btn_down(int id)
{
    if (id < 0 || id >= BOARD_BTN_MAX || !s_btn[id].used) {
        return 0;
    }
    return s_btn[id].stable ? 1 : 0;
}

uint32_t board_btn_held_ms(int id)
{
    btn_slot_t *b;
    if (id < 0 || id >= BOARD_BTN_MAX || !s_btn[id].used) {
        return 0;
    }
    b = &s_btn[id];
    if (!b->stable) {
        return 0;
    }
    return (uint32_t)(board_millis() - b->press_ms);
}
