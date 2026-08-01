#include "board_qei.h"
#include "board_pins.h"
#include "board_reg.h"
#include "board_resource.h"
#include "ti_msp_dl_config.h"

#include <string.h>

#define QEI_INST TIMG8
#define QEI_LOAD 0xFFFFu

static uint8_t s_on;
static int32_t s_base;
static int32_t s_mark;
static uint16_t s_last_raw;

static board_pin_t pin_copy(const char *name)
{
    board_pin_t out;
    memset(&out, 0, sizeof(out));
    (void)board_pin_resolve(name, &out);
    return out;
}

static uint16_t qei_raw(void)
{
    return (uint16_t)DL_TimerG_getTimerCount(QEI_INST);
}

static void qei_sync_ext(void)
{
    uint16_t r = qei_raw();
    int16_t d = (int16_t)(r - s_last_raw);
    s_base += d;
    s_last_raw = r;
}

int board_qei_open(void)
{
    board_pin_t pa = pin_copy("PA26");
    board_pin_t pb = pin_copy("PA27");
    DL_TimerG_ClockConfig clk;

    if (s_on) {
        return 0;
    }
    if (!pa.port || !pb.port) {
        return -1;
    }
    if (board_pin_claim("PA26", PIN_OWN_QEI, 0) != 0 ||
            board_pin_claim("PA27", PIN_OWN_QEI, 0) != 0) {
        board_pin_release_owned("PA26", PIN_OWN_QEI);
        board_pin_release_owned("PA27", PIN_OWN_QEI);
        return -2;
    }
    if (board_resource_claim(BOARD_RES_TIMG8, PIN_OWN_QEI) != 0) {
        board_pin_release_owned("PA26", PIN_OWN_QEI);
        board_pin_release_owned("PA27", PIN_OWN_QEI);
        return -3;
    }

    /* ensure not driving as GPIO out from prior scripts */
    pa.port->DOECLR31_0 = pa.pin;
    pb.port->DOECLR31_0 = pb.pin;

    /* PA26=TIMG8_CCP0 PF4, PA27=TIMG8_CCP1 PF4 */
    DL_GPIO_initPeripheralInputFunctionFeatures(pa.iomux, 4,
        DL_GPIO_INVERSION_DISABLE, DL_GPIO_RESISTOR_PULL_UP,
        DL_GPIO_HYSTERESIS_DISABLE, DL_GPIO_WAKEUP_DISABLE);
    DL_GPIO_initPeripheralInputFunctionFeatures(pb.iomux, 4,
        DL_GPIO_INVERSION_DISABLE, DL_GPIO_RESISTOR_PULL_UP,
        DL_GPIO_HYSTERESIS_DISABLE, DL_GPIO_WAKEUP_DISABLE);

    clk.clockSel = DL_TIMER_CLOCK_BUSCLK;
    clk.divideRatio = DL_TIMER_CLOCK_DIVIDE_1;
    clk.prescale = 0;

    DL_TimerG_reset(QEI_INST);
    DL_TimerG_enablePower(QEI_INST);
    delay_cycles(POWER_STARTUP_DELAY);
    DL_TimerG_setClockConfig(QEI_INST, &clk);
    DL_TimerG_configQEI(QEI_INST, DL_TIMER_QEI_MODE_2_INPUT,
        DL_TIMER_CC_INPUT_INV_NOINVERT, DL_TIMER_CC_0_INDEX);
    DL_TimerG_configQEI(QEI_INST, DL_TIMER_QEI_MODE_2_INPUT,
        DL_TIMER_CC_INPUT_INV_NOINVERT, DL_TIMER_CC_1_INDEX);
    DL_TimerG_setLoadValue(QEI_INST, QEI_LOAD);
    DL_TimerG_enableClock(QEI_INST);
    DL_TimerG_setTimerCount(QEI_INST, 0);
    DL_TimerG_startCounter(QEI_INST);

    s_base = 0;
    s_mark = 0;
    s_last_raw = 0;
    s_on = 1;
    return 0;
}

void board_qei_close(void)
{
    if (!s_on) {
        return;
    }
    DL_TimerG_stopCounter(QEI_INST);
    DL_TimerG_disableClock(QEI_INST);
    DL_TimerG_reset(QEI_INST);
    board_pin_release_owned("PA26", PIN_OWN_QEI);
    board_pin_release_owned("PA27", PIN_OWN_QEI);
    board_resource_release(BOARD_RES_TIMG8, PIN_OWN_QEI);
    s_on = 0;
}

void board_qei_set(int32_t pos)
{
    if (!s_on) {
        return;
    }
    DL_TimerG_setTimerCount(QEI_INST, 0);
    s_base = pos;
    s_mark = pos;
    s_last_raw = 0;
}

int32_t board_qei_pos(void)
{
    if (!s_on) {
        return 0;
    }
    qei_sync_ext();
    return s_base;
}

int32_t board_qei_delta(void)
{
    int32_t p, d;
    if (!s_on) {
        return 0;
    }
    p = board_qei_pos();
    d = p - s_mark;
    s_mark = p;
    return d;
}

int board_qei_dir(void)
{
    if (!s_on) {
        return 0;
    }
    if (DL_TimerG_getQEIDirection(QEI_INST) == DL_TIMER_QEI_DIR_UP) {
        return 1;
    }
    return -1;
}

int board_qei_active(void)
{
    return s_on ? 1 : 0;
}

static void qgen_delay_us(uint32_t us)
{
    uint32_t c = g_cpuclk_hz ? (g_cpuclk_hz / 1000000u) * us : us * 32u;
    if (c < 8u) {
        c = 8u;
    }
    board_reg_delay_cycles(c);
}

int board_qgen_run(int steps, int dir, uint32_t half_us)
{
    board_pin_t pa = pin_copy("PA14");
    board_pin_t pb = pin_copy("PA25");
    int i;
    uint8_t a = 0, b = 0;

    if (!pa.port || !pb.port || steps < 1) {
        return -1;
    }
    if (half_us < 20u) {
        half_us = 20u;
    }
    if (half_us > 50000u) {
        half_us = 50000u;
    }
    if (board_pin_claim("PA14", PIN_OWN_GPIO, 0) != 0 ||
            board_pin_claim("PA25", PIN_OWN_GPIO, 0) != 0) {
        board_pin_release_owned("PA14", PIN_OWN_GPIO);
        board_pin_release_owned("PA25", PIN_OWN_GPIO);
        return -2;
    }
    board_reg_pin_out(pa.port, pa.pin, pa.iomux);
    board_reg_pin_out(pb.port, pb.pin, pb.iomux);
    board_reg_gpio_write(pa.port, pa.pin, 0);
    board_reg_gpio_write(pb.port, pb.pin, 0);

    for (i = 0; i < steps; i++) {
        if (dir >= 0) {
            a ^= 1u;
            board_reg_gpio_write(pa.port, pa.pin, a);
            qgen_delay_us(half_us);
            b ^= 1u;
            board_reg_gpio_write(pb.port, pb.pin, b);
            qgen_delay_us(half_us);
        } else {
            b ^= 1u;
            board_reg_gpio_write(pb.port, pb.pin, b);
            qgen_delay_us(half_us);
            a ^= 1u;
            board_reg_gpio_write(pa.port, pa.pin, a);
            qgen_delay_us(half_us);
        }
    }
    board_pin_release_owned("PA14", PIN_OWN_GPIO);
    board_pin_release_owned("PA25", PIN_OWN_GPIO);
    return 0;
}
