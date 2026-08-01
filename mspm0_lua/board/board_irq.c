#include "board_irq.h"
#ifndef MSPM0_MODULAR_CORE
#include "board_enc.h"
#endif
#include "board_pins.h"
#include "board_reg.h"
#include "ti_msp_dl_config.h"

#include <string.h>

/* Cap catch-up steps per 1 ms tick so a stalled main loop cannot hog the ISR. */
#define SOFT_TIMER_CATCHUP_MAX 32u

typedef struct {
    volatile uint8_t active;
    volatile uint32_t period;
    volatile uint32_t next;
    volatile uint32_t pending;
} soft_timer_slot_t;

typedef struct {
    uint8_t used;
    uint8_t edge;
    GPIO_Regs *port;
    uint32_t pin;
    char name[8];
    volatile uint32_t count;
    uint32_t debounce_ms;
    uint32_t last_ms;
} gpio_irq_slot_t;

static volatile uint32_t s_millis;
static soft_timer_slot_t s_timer[BOARD_SOFT_TIMER_MAX];
static gpio_irq_slot_t s_girq[BOARD_GPIO_IRQ_MAX];

static void tick_timer_init(void)
{
    uint32_t ticks = (g_cpuclk_hz ? g_cpuclk_hz : 32000000u) / 1000u;
    SysTick->CTRL = 0u;
    if (ticks < 2u) ticks = 2u;
    (void)SysTick_Config(ticks);
}

void SysTick_Handler(void)
{
    s_millis++;
    for (unsigned i = 0; i < BOARD_SOFT_TIMER_MAX; i++) {
        soft_timer_slot_t *t = &s_timer[i];
        unsigned steps = 0;
        if (!t->active || t->period == 0u) {
            continue;
        }
        while ((int32_t)(s_millis - t->next) >= 0 &&
                steps < SOFT_TIMER_CATCHUP_MAX) {
            t->next += t->period;
            if (t->pending != UINT32_MAX) {
                t->pending++;
            }
            steps++;
        }
        if (steps == SOFT_TIMER_CATCHUP_MAX &&
                (int32_t)(s_millis - t->next) >= 0) {
            /* Snap forward; pending already saturated for this burst. */
            t->next = s_millis + t->period;
        }
    }
}

/* POLARITY: 2 bits/pin — 01 rise, 10 fall, 11 both (DriverLib encoding). */
static void set_pin_edge(GPIO_Regs *port, uint32_t pin, int edge)
{
    int idx = 0;
    uint32_t p = pin;
    uint32_t code = (edge == 1) ? 1u : (edge == 0) ? 2u : 3u;
    uint32_t enc;

    while (p > 1u) {
        p >>= 1;
        idx++;
    }
    if (idx < 16) {
        enc = code << (uint32_t)(idx * 2);
        DL_GPIO_setLowerPinsPolarity(port, enc);
    } else {
        enc = code << (uint32_t)((idx - 16) * 2);
        DL_GPIO_setUpperPinsPolarity(port, enc);
    }
}

static void gpio_edge_hit(gpio_irq_slot_t *s)
{
    uint32_t now = s_millis;
    if (s->debounce_ms &&
            (uint32_t)(now - s->last_ms) < s->debounce_ms) {
        return;
    }
    if (s->count != UINT32_MAX) {
        s->count++;
    }
    s->last_ms = now;
}

void GROUP1_IRQHandler(void)
{
    /* MIS & clear via CPU_INT (posted writes); avoid DriverLib call overhead. */
    uint32_t st_a = GPIOA->CPU_INT.MIS;
    uint32_t st_b = GPIOB->CPU_INT.MIS;
    int i;

    for (i = 0; i < BOARD_GPIO_IRQ_MAX; i++) {
        if (!s_girq[i].used) {
            continue;
        }
        if (s_girq[i].port == GPIOA && (st_a & s_girq[i].pin)) {
            gpio_edge_hit(&s_girq[i]);
        }
        if (s_girq[i].port == GPIOB && (st_b & s_girq[i].pin)) {
            gpio_edge_hit(&s_girq[i]);
        }
    }
    if (st_a) {
        GPIOA->CPU_INT.ICLR = st_a;
    }
    if (st_b) {
        GPIOB->CPU_INT.ICLR = st_b;
    }
    /* Quadrature software decoder samples A/B after edges. */
#ifndef MSPM0_MODULAR_CORE
    board_enc_isr_tick();
#endif
}

void board_irq_init(void)
{
    s_millis = 0;
    memset((void *)s_timer, 0, sizeof(s_timer));
    memset((void *)s_girq, 0, sizeof(s_girq));
    tick_timer_init();
    NVIC_ClearPendingIRQ(GPIOA_INT_IRQn);
    NVIC_EnableIRQ(GPIOA_INT_IRQn);
    __enable_irq();
}

uint32_t board_millis(void)
{
    return s_millis;
}

int board_soft_timer_start(unsigned id, uint32_t period_ms)
{
    uint32_t primask;
    if (id >= BOARD_SOFT_TIMER_MAX || period_ms == 0u) {
        return -1;
    }
    primask = __get_PRIMASK();
    __disable_irq();
    s_timer[id].period = period_ms;
    s_timer[id].next = s_millis + period_ms;
    s_timer[id].pending = 0;
    s_timer[id].active = 1;
    if (!primask) {
        __enable_irq();
    }
    return 0;
}

void board_soft_timer_stop(unsigned id)
{
    uint32_t primask;
    if (id >= BOARD_SOFT_TIMER_MAX) {
        return;
    }
    primask = __get_PRIMASK();
    __disable_irq();
    s_timer[id].active = 0;
    s_timer[id].pending = 0;
    if (!primask) {
        __enable_irq();
    }
}

uint32_t board_soft_timer_take(unsigned id)
{
    uint32_t primask, n;
    if (id >= BOARD_SOFT_TIMER_MAX) {
        return 0;
    }
    primask = __get_PRIMASK();
    __disable_irq();
    n = s_timer[id].pending;
    s_timer[id].pending = 0;
    if (!primask) {
        __enable_irq();
    }
    return n;
}

void board_delay_ms_irq(uint32_t ms)
{
    uint32_t start = s_millis;
    uint32_t spins = 0;
    while ((uint32_t)(s_millis - start) < ms) {
        if (++spins > 100000u) {
            extern void board_delay_ms(uint32_t);
            board_delay_ms(ms);
            return;
        }
    }
}

int board_gpio_irq_enable(const char *pin_name, int edge)
{
    board_pin_t p;
    int slot = -1;
    int i;

    if (board_pin_resolve(pin_name, &p) != 0) {
        return -1;
    }
    if (edge < 0 || edge > 2) {
        edge = 0;
    }
    for (i = 0; i < BOARD_GPIO_IRQ_MAX; i++) {
        if (s_girq[i].used && strcmp(s_girq[i].name, pin_name) == 0) {
            slot = i;
            break;
        }
        if (!s_girq[i].used && slot < 0) {
            slot = i;
        }
    }
    if (slot < 0) {
        return -1;
    }

    s_girq[slot].used = 1;
    s_girq[slot].edge = (uint8_t)edge;
    s_girq[slot].port = p.port;
    s_girq[slot].pin = p.pin;
    s_girq[slot].count = 0;
    s_girq[slot].debounce_ms = 0;
    s_girq[slot].last_ms = s_millis;
    {
        size_t n = 0;
        while (pin_name[n] && n < 7) {
            s_girq[slot].name[n] = pin_name[n];
            n++;
        }
        s_girq[slot].name[n] = 0;
    }

    /* Input + pull-up; clear DOE so prior gpio.out does not fight the net. */
    p.port->DOECLR31_0 = p.pin;
    board_reg_iomux_gpio_in_pu(p.iomux);
    set_pin_edge(p.port, p.pin, edge);
    DL_GPIO_clearInterruptStatus(p.port, p.pin);
    DL_GPIO_enableInterrupt(p.port, p.pin);
    return 0;
}

int board_gpio_irq_set_debounce(const char *pin_name, uint32_t debounce_ms)
{
    int i;
    for (i = 0; i < BOARD_GPIO_IRQ_MAX; i++) {
        if (s_girq[i].used && strcmp(s_girq[i].name, pin_name) == 0) {
            s_girq[i].debounce_ms = debounce_ms;
            /* Allow the next edge immediately after reconfigure. */
            s_girq[i].last_ms = s_millis - debounce_ms;
            return 0;
        }
    }
    return -1;
}

int board_gpio_irq_disable(const char *pin_name)
{
    int i;
    for (i = 0; i < BOARD_GPIO_IRQ_MAX; i++) {
        if (s_girq[i].used && strcmp(s_girq[i].name, pin_name) == 0) {
            DL_GPIO_disableInterrupt(s_girq[i].port, s_girq[i].pin);
            s_girq[i].used = 0;
            return 0;
        }
    }
    return -1;
}

uint32_t board_gpio_irq_count(const char *pin_name)
{
    int i;
    for (i = 0; i < BOARD_GPIO_IRQ_MAX; i++) {
        if (s_girq[i].used && strcmp(s_girq[i].name, pin_name) == 0) {
            uint32_t primask = __get_PRIMASK();
            uint32_t c;
            __disable_irq();
            c = s_girq[i].count;
            s_girq[i].count = 0;
            if (!primask) {
                __enable_irq();
            }
            return c;
        }
    }
    return 0;
}

void board_gpio_irq_reset(void)
{
    uint32_t primask = __get_PRIMASK();
    int i;
    __disable_irq();
    for (i = 0; i < BOARD_GPIO_IRQ_MAX; i++) {
        if (s_girq[i].used) {
            DL_GPIO_disableInterrupt(s_girq[i].port, s_girq[i].pin);
        }
    }
    memset((void *)s_girq, 0, sizeof(s_girq));
    if (!primask) {
        __enable_irq();
    }
}
