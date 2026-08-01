#include "board_cap.h"
#include "board_pins.h"
#include "board_resource.h"
#include "ti_msp_dl_config.h"

#include <string.h>

#define CAP_INST TIMG6
#define CAP_IRQn TIMG6_INT_IRQn
#define CAP_LOAD 0xFFFFu

/* TIMG6 routes: CCP0/1 PF from device IOMUX table. */
static int cap_route(const char *pin, unsigned *pf_out, int *chan_out)
{
    static const struct {
        char pin[5];
        uint8_t pf;
        uint8_t chan;
    } k[] = {
        {"PA22", 8, 1}, /* TIMG6_CCP1 */
        {"PA21", 6, 0}, /* TIMG6_CCP0 */
        {"PB6", 7, 0},
        {"PB7", 7, 1},
        {"PB3", 7, 1},
        {"PB2", 7, 0},
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
            if (chan_out) {
                *chan_out = (int)k[i].chan;
            }
            return 0;
        }
    }
    return -1;
}

static uint8_t s_on;
static uint8_t s_ready;
static uint8_t s_have;
static uint8_t s_chan;
static volatile uint32_t s_period;
static volatile uint32_t s_hits;
static uint32_t s_prev;
static uint32_t s_tick_hz;
static char s_pin[8];

void TIMG6_IRQHandler(void)
{
    DL_TIMER_IIDX iidx = DL_TimerG_getPendingInterrupt(CAP_INST);
    int hit = 0;
    uint32_t cc = 0;

    if (s_chan == 1) {
        if (iidx == DL_TIMER_IIDX_CC1_DN || iidx == DL_TIMER_IIDX_CC1_UP) {
            hit = 1;
            cc = DL_TimerG_getCaptureCompareValue(CAP_INST, DL_TIMER_CC_1_INDEX);
        }
    } else {
        if (iidx == DL_TIMER_IIDX_CC0_DN || iidx == DL_TIMER_IIDX_CC0_UP) {
            hit = 1;
            cc = DL_TimerG_getCaptureCompareValue(CAP_INST, DL_TIMER_CC_0_INDEX);
        }
    }
    if (!hit) {
        return;
    }

    {
        uint32_t stamp = (cc <= CAP_LOAD) ? (CAP_LOAD - cc) : 0u;
        s_hits++;
        if (s_have) {
            uint32_t p = (stamp >= s_prev)
                ? (stamp - s_prev)
                : (stamp + (CAP_LOAD - s_prev) + 1u);
            if (p > 1u && p < (CAP_LOAD / 2u)) {
                s_period = p;
                s_ready = 1;
            }
        }
        s_prev = stamp;
        s_have = 1;
    }
}

int board_cap_open(const char *pin, int edge)
{
    board_pin_t p;
    unsigned pf = 0;
    int chan = 0;
    DL_TimerG_ClockConfig clk;
    DL_TimerG_CaptureConfig cfg;
    DL_TIMER_CAPTURE_EDGE_DETECTION_MODE em;
    uint32_t bus;
    uint32_t ie;

    if (s_on) {
        board_cap_close();
    }
    if (!pin) {
        pin = "PA22";
    }
    if (cap_route(pin, &pf, &chan) != 0) {
        return -1;
    }
    if (board_pin_resolve(pin, &p) != 0) {
        return -1;
    }
    if (board_pin_claim(pin, PIN_OWN_CAP, 0) != 0) {
        return -2;
    }
    if (board_resource_claim(BOARD_RES_TIMG6, PIN_OWN_CAP) != 0) {
        board_pin_release_owned(pin, PIN_OWN_CAP);
        return -3;
    }

    DL_GPIO_initPeripheralInputFunctionFeatures(p.iomux, pf,
        DL_GPIO_INVERSION_DISABLE, DL_GPIO_RESISTOR_PULL_UP,
        DL_GPIO_HYSTERESIS_DISABLE, DL_GPIO_WAKEUP_DISABLE);

    if (edge == 1) {
        em = DL_TIMER_CAPTURE_EDGE_DETECTION_MODE_FALLING;
    } else if (edge == 2) {
        em = DL_TIMER_CAPTURE_EDGE_DETECTION_MODE_EDGE;
    } else {
        em = DL_TIMER_CAPTURE_EDGE_DETECTION_MODE_RISING;
    }

    bus = g_uart_busclk_hz ? g_uart_busclk_hz : 32000000u;
    s_tick_hz = bus / 16u;
    if (s_tick_hz < 1u) {
        s_tick_hz = 1u;
    }

    clk.clockSel = DL_TIMER_CLOCK_BUSCLK;
    clk.divideRatio = DL_TIMER_CLOCK_DIVIDE_1;
    clk.prescale = 15;

    cfg.captureMode = DL_TIMER_CAPTURE_MODE_EDGE_TIME;
    cfg.period = CAP_LOAD;
    cfg.startTimer = DL_TIMER_STOP;
    cfg.edgeCaptMode = em;
    cfg.inputChan = (chan == 1) ? DL_TIMER_INPUT_CHAN_1 : DL_TIMER_INPUT_CHAN_0;
    cfg.inputInvMode = DL_TIMER_CC_INPUT_INV_NOINVERT;

    DL_TimerG_reset(CAP_INST);
    DL_TimerG_enablePower(CAP_INST);
    delay_cycles(POWER_STARTUP_DELAY);
    DL_TimerG_setClockConfig(CAP_INST, &clk);
    DL_TimerG_initCaptureMode(CAP_INST, &cfg);
    ie = (chan == 1) ? DL_TIMERG_INTERRUPT_CC1_DN_EVENT
                     : DL_TIMERG_INTERRUPT_CC0_DN_EVENT;
    DL_TimerG_enableInterrupt(CAP_INST, ie);
    DL_TimerG_enableClock(CAP_INST);
    NVIC_ClearPendingIRQ(CAP_IRQn);
    NVIC_EnableIRQ(CAP_IRQn);
    DL_TimerG_setTimerCount(CAP_INST, CAP_LOAD);
    DL_TimerG_startCounter(CAP_INST);

    {
        size_t n = 0;
        while (pin[n] && n < 7) {
            s_pin[n] = pin[n];
            n++;
        }
        s_pin[n] = 0;
    }
    s_chan = (uint8_t)chan;
    s_period = 0;
    s_ready = 0;
    s_have = 0;
    s_hits = 0;
    s_prev = 0;
    s_on = 1;
    return 0;
}

void board_cap_close(void)
{
    if (!s_on) {
        return;
    }
    DL_TimerG_stopCounter(CAP_INST);
    NVIC_DisableIRQ(CAP_IRQn);
    DL_TimerG_disableInterrupt(CAP_INST,
        DL_TIMERG_INTERRUPT_CC0_DN_EVENT | DL_TIMERG_INTERRUPT_CC1_DN_EVENT);
    DL_TimerG_disableClock(CAP_INST);
    DL_TimerG_reset(CAP_INST);
    if (s_pin[0]) {
        board_pin_release_owned(s_pin, PIN_OWN_CAP);
        s_pin[0] = 0;
    }
    board_resource_release(BOARD_RES_TIMG6, PIN_OWN_CAP);
    s_on = 0;
    s_ready = 0;
    s_period = 0;
    s_hits = 0;
}

uint32_t board_cap_period(void)
{
    return s_period;
}

uint32_t board_cap_hz_x10(void)
{
    uint32_t p = s_period;
    if (p == 0u || s_tick_hz == 0u) {
        return 0u;
    }
    return (uint32_t)(((uint64_t)s_tick_hz * 10u) / p);
}

int board_cap_ready(void)
{
    return (s_on && s_ready) ? 1 : 0;
}

uint32_t board_cap_hits(void)
{
    return s_hits;
}
