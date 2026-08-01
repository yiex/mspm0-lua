#include "board_pwm.h"
#include "board_pins.h"
#include "board_resource.h"
#include "ti_msp_dl_config.h"
#include <string.h>

#define PWM_N 2
#define COMP_N 2

typedef struct {
    uint8_t used;
    uint8_t ccp;
    uint8_t timer; /* 0 TIMG12, 1 TIMG7 */
    uint16_t period;
    char pin[8];
} pwm_hw_t;

typedef struct {
    uint8_t used;
    uint8_t tima; /* 0 TIMA0, 1 TIMA1 */
    uint16_t period;
    uint32_t timer_clk;
    char hi[8];
    char lo[8];
} comp_hw_t;

static pwm_hw_t s_pwm[PWM_N];
static comp_hw_t s_comp[COMP_N];

static GPTIMER_Regs *pwm_inst(int timer)
{
    return timer ? TIMG7 : TIMG12;
}

static board_resource_t pwm_res(int timer)
{
    return timer ? BOARD_RES_TIMG7 : BOARD_RES_TIMG12;
}

static uint8_t pwm_own(int timer)
{
    return timer ? PIN_OWN_PWM2 : PIN_OWN_PWM;
}

static GPTIMER_Regs *comp_inst(int tima)
{
    return tima ? TIMA1 : TIMA0;
}

static board_resource_t comp_res(int tima)
{
    return tima ? BOARD_RES_TIMA1 : BOARD_RES_TIMA0;
}

static uint8_t comp_own(int tima)
{
    return tima ? PIN_OWN_PWMCOMP2 : PIN_OWN_PWMCOMP;
}

static void cpy7(char *d, const char *s)
{
    size_t i = 0;
    while (s && s[i] && i < 7) {
        d[i] = s[i];
        i++;
    }
    d[i] = 0;
}

int board_pwm_open(const char *pin, uint32_t freq_hz)
{
    uint32_t bus = g_cpuclk_hz ? g_cpuclk_hz : 32000000u;
    uint32_t prescale = 0;
    uint32_t timer_clk;
    uint32_t period;
    unsigned pf;
    unsigned ccp;
    int timer;
    int id = -1;
    int st;
    int i;
    board_pin_t p;
    GPTIMER_Regs *inst;
    DL_TIMER_CC_INDEX cc_idx;
    uint32_t ccp_dir;

    if (!pin) {
        pin = "PA14";
    }
    if (board_pwm_route(pin, &pf, &ccp, &timer) != 0) {
        return -1;
    }
    for (i = 0; i < PWM_N; i++) {
        if (s_pwm[i].used && s_pwm[i].timer == (uint8_t)timer) {
            return -3; /* timer busy */
        }
        if (!s_pwm[i].used && id < 0) {
            id = i;
        }
    }
    if (id < 0) {
        return -1;
    }
    if (board_pin_resolve(pin, &p) != 0) {
        return -1;
    }
    st = board_pin_claim(pin, pwm_own(timer), 0);
    if (st != 0) {
        return st;
    }
    st = board_resource_claim(pwm_res(timer), pwm_own(timer));
    if (st != 0) {
        board_pin_release_owned(pin, pwm_own(timer));
        return st;
    }
    if (freq_hz < 100u) {
        freq_hz = 100u;
    }
    if (freq_hz > 50000u) {
        freq_hz = 50000u;
    }
    for (prescale = 0; prescale < 256u; prescale++) {
        timer_clk = bus / (prescale + 1u);
        period = timer_clk / freq_hz;
        if (period >= 20u && period <= 65535u) {
            break;
        }
    }
    if (prescale >= 256u) {
        board_pin_release_owned(pin, pwm_own(timer));
        board_resource_release(pwm_res(timer), pwm_own(timer));
        return -1;
    }

    inst = pwm_inst(timer);
    cc_idx = ccp ? DL_TIMERG_CAPTURE_COMPARE_1_INDEX
                 : DL_TIMERG_CAPTURE_COMPARE_0_INDEX;
    ccp_dir = ccp ? DL_TIMER_CC1_OUTPUT : DL_TIMER_CC0_OUTPUT;

    DL_TimerG_reset(inst);
    DL_TimerG_enablePower(inst);
    delay_cycles(POWER_STARTUP_DELAY);
    DL_GPIO_initPeripheralOutputFunction(p.iomux, pf);
    DL_GPIO_enableOutput(p.port, p.pin);
    {
        DL_TimerG_ClockConfig clk = {
            .clockSel = DL_TIMER_CLOCK_BUSCLK,
            .divideRatio = DL_TIMER_CLOCK_DIVIDE_1,
            .prescale = (uint8_t)prescale,
        };
        DL_TimerG_PWMConfig pwm = {
            .pwmMode = DL_TIMER_PWM_MODE_EDGE_ALIGN,
            .period = (uint16_t)period,
            .isTimerWithFourCC = false,
            .startTimer = DL_TIMER_STOP,
        };
        DL_TimerG_setClockConfig(inst, &clk);
        DL_TimerG_initPWMMode(inst, &pwm);
    }
    DL_TimerG_setCaptureCompareOutCtl(inst, DL_TIMER_CC_OCTL_INIT_VAL_LOW,
        DL_TIMER_CC_OCTL_INV_OUT_DISABLED, DL_TIMER_CC_OCTL_SRC_FUNCVAL, cc_idx);
    DL_TimerG_setCaptCompUpdateMethod(inst, DL_TIMER_CC_UPDATE_METHOD_IMMEDIATE,
        cc_idx);
    DL_TimerG_setCaptureCompareValue(inst, 0, cc_idx);
    DL_TimerG_setCCPDirection(inst, ccp_dir);
    DL_TimerG_enableClock(inst);
    DL_TimerG_startCounter(inst);

    s_pwm[id].used = 1;
    s_pwm[id].ccp = (uint8_t)(ccp ? 1u : 0u);
    s_pwm[id].timer = (uint8_t)timer;
    s_pwm[id].period = (uint16_t)period;
    cpy7(s_pwm[id].pin, pin);
    return id;
}

void board_pwm_set_duty(int id, uint8_t duty)
{
    uint32_t cc;
    DL_TIMER_CC_INDEX cc_idx;
    GPTIMER_Regs *inst;
    if (id < 0 || id >= PWM_N || !s_pwm[id].used) {
        return;
    }
    if (duty > 100) {
        duty = 100;
    }
    cc = ((uint32_t)s_pwm[id].period * duty) / 100u;
    if (cc > s_pwm[id].period) {
        cc = s_pwm[id].period;
    }
    inst = pwm_inst((int)s_pwm[id].timer);
    cc_idx = s_pwm[id].ccp ? DL_TIMERG_CAPTURE_COMPARE_1_INDEX
                            : DL_TIMERG_CAPTURE_COMPARE_0_INDEX;
    DL_TimerG_setCaptureCompareValue(inst, cc, cc_idx);
}

void board_pwm_close(int id)
{
    board_pin_t p;
    int timer;
    if (id < 0 || id >= PWM_N || !s_pwm[id].used) {
        return;
    }
    timer = (int)s_pwm[id].timer;
    DL_TimerG_stopCounter(pwm_inst(timer));
    if (board_pin_resolve(s_pwm[id].pin, &p) == 0) {
        DL_GPIO_initDigitalOutput(p.iomux);
        DL_GPIO_clearPins(p.port, p.pin);
        DL_GPIO_enableOutput(p.port, p.pin);
    }
    board_pin_release_owner(pwm_own(timer));
    board_resource_release(pwm_res(timer), pwm_own(timer));
    memset(&s_pwm[id], 0, sizeof(s_pwm[id]));
}

void board_pwm_close_all(void)
{
    int i;
    for (i = 0; i < PWM_N; i++) {
        board_pwm_close(i);
    }
}

int board_pwm_comp_open(const char *hi, const char *lo,
    uint32_t freq_hz, uint8_t duty_pct, uint32_t dead_ns)
{
    uint32_t bus = g_cpuclk_hz ? g_cpuclk_hz : 32000000u;
    uint32_t prescale = 0;
    uint32_t period;
    uint32_t dead_ticks;
    uint32_t cc;
    unsigned pf_h, pf_l;
    int tima;
    int id = -1;
    int st;
    int i;
    board_pin_t ph, pl;
    GPTIMER_Regs *inst;

    if (!hi) {
        hi = "PA8";
    }
    if (!lo) {
        lo = "PA22";
    }
    if (board_pwm_comp_route(hi, lo, &pf_h, &pf_l, &tima) != 0) {
        return -1;
    }
    for (i = 0; i < COMP_N; i++) {
        if (s_comp[i].used && s_comp[i].tima == (uint8_t)tima) {
            return -3;
        }
        if (!s_comp[i].used && id < 0) {
            id = i;
        }
    }
    if (id < 0) {
        return -1;
    }
    if (board_pin_resolve(hi, &ph) != 0 || board_pin_resolve(lo, &pl) != 0) {
        return -1;
    }
    st = board_pin_claim(hi, comp_own(tima), 0);
    if (st != 0) {
        return st;
    }
    st = board_pin_claim(lo, comp_own(tima), 0);
    if (st != 0) {
        board_pin_release_owned(hi, comp_own(tima));
        return st;
    }
    st = board_resource_claim(comp_res(tima), comp_own(tima));
    if (st != 0) {
        board_pin_release_owner(comp_own(tima));
        return st;
    }
    if (freq_hz < 100u) {
        freq_hz = 100u;
    }
    if (freq_hz > 100000u) {
        freq_hz = 100000u;
    }
    if (duty_pct > 100u) {
        duty_pct = 100u;
    }
    for (prescale = 0; prescale < 256u; prescale++) {
        s_comp[id].timer_clk = bus / (prescale + 1u);
        period = s_comp[id].timer_clk / freq_hz;
        if (period >= 40u && period <= 65535u) {
            break;
        }
    }
    if (prescale >= 256u) {
        board_pin_release_owner(comp_own(tima));
        board_resource_release(comp_res(tima), comp_own(tima));
        return -1;
    }
    dead_ticks = (uint32_t)(((uint64_t)dead_ns * s_comp[id].timer_clk) /
        1000000000ull);
    if (dead_ticks == 0u && dead_ns > 0u) {
        dead_ticks = 1u;
    }
    if (dead_ticks > 255u) {
        dead_ticks = 255u;
    }

    inst = comp_inst(tima);
    DL_TimerA_reset(inst);
    DL_TimerA_enablePower(inst);
    delay_cycles(POWER_STARTUP_DELAY);
    DL_GPIO_initPeripheralOutputFunction(ph.iomux, pf_h);
    DL_GPIO_enableOutput(ph.port, ph.pin);
    DL_GPIO_initPeripheralOutputFunction(pl.iomux, pf_l);
    DL_GPIO_enableOutput(pl.port, pl.pin);
    {
        DL_TimerA_ClockConfig clk = {
            .clockSel = DL_TIMER_CLOCK_BUSCLK,
            .divideRatio = DL_TIMER_CLOCK_DIVIDE_1,
            .prescale = (uint8_t)prescale,
        };
        DL_TimerA_PWMConfig pwm = {
            .pwmMode = DL_TIMER_PWM_MODE_EDGE_ALIGN,
            .period = (uint16_t)period,
            .isTimerWithFourCC = true,
            .startTimer = DL_TIMER_STOP,
        };
        DL_TimerA_setClockConfig(inst, &clk);
        DL_TimerA_initPWMMode(inst, &pwm);
    }
    DL_TimerA_setCounterControl(inst, DL_TIMER_CZC_CCCTL0_ZCOND,
        DL_TIMER_CAC_CCCTL0_ACOND, DL_TIMER_CLC_CCCTL0_LCOND);
    DL_TimerA_setCaptureCompareOutCtl(inst, DL_TIMER_CC_OCTL_INIT_VAL_LOW,
        DL_TIMER_CC_OCTL_INV_OUT_DISABLED, DL_TIMER_CC_OCTL_SRC_DEAD_BAND,
        DL_TIMERA_CAPTURE_COMPARE_0_INDEX);
    DL_TimerA_setCaptCompUpdateMethod(inst, DL_TIMER_CC_UPDATE_METHOD_IMMEDIATE,
        DL_TIMERA_CAPTURE_COMPARE_0_INDEX);
    cc = ((uint32_t)period * duty_pct) / 100u;
    if (cc > period) {
        cc = period;
    }
    DL_TimerA_setCaptureCompareValue(inst, cc, DL_TIMER_CC_0_INDEX);
    DL_TimerA_setDeadBand(inst, (uint16_t)dead_ticks, (uint16_t)dead_ticks,
        DL_TIMER_DEAD_BAND_MODE_0);
    DL_TimerA_setCCPDirection(inst, DL_TIMER_CC0_OUTPUT);
    DL_TimerA_enableClock(inst);
    DL_TimerA_startCounter(inst);

    s_comp[id].used = 1;
    s_comp[id].tima = (uint8_t)tima;
    s_comp[id].period = (uint16_t)period;
    cpy7(s_comp[id].hi, hi);
    cpy7(s_comp[id].lo, lo);
    return id;
}

void board_pwm_comp_set_duty(int id, uint8_t duty_pct)
{
    uint32_t cc;
    if (id < 0 || id >= COMP_N || !s_comp[id].used) {
        return;
    }
    if (duty_pct > 100u) {
        duty_pct = 100u;
    }
    cc = ((uint32_t)s_comp[id].period * duty_pct) / 100u;
    if (cc > s_comp[id].period) {
        cc = s_comp[id].period;
    }
    DL_TimerA_setCaptureCompareValue(comp_inst((int)s_comp[id].tima), cc,
        DL_TIMER_CC_0_INDEX);
}

void board_pwm_comp_close(int id)
{
    board_pin_t p;
    int tima;
    if (id < 0 || id >= COMP_N || !s_comp[id].used) {
        return;
    }
    tima = (int)s_comp[id].tima;
    DL_TimerA_stopCounter(comp_inst(tima));
    if (board_pin_resolve(s_comp[id].hi, &p) == 0) {
        DL_GPIO_initDigitalOutput(p.iomux);
        DL_GPIO_clearPins(p.port, p.pin);
        DL_GPIO_enableOutput(p.port, p.pin);
    }
    if (board_pin_resolve(s_comp[id].lo, &p) == 0) {
        DL_GPIO_initDigitalOutput(p.iomux);
        DL_GPIO_clearPins(p.port, p.pin);
        DL_GPIO_enableOutput(p.port, p.pin);
    }
    board_pin_release_owner(comp_own(tima));
    board_resource_release(comp_res(tima), comp_own(tima));
    memset(&s_comp[id], 0, sizeof(s_comp[id]));
}

void board_pwm_comp_close_all(void)
{
    int i;
    for (i = 0; i < COMP_N; i++) {
        board_pwm_comp_close(i);
    }
}
