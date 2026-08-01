#include "native_module.h"

#include "board_pins.h"
#include "board_reg.h"
#include "board_resource.h"
#include "g3507_pin_routes.h"
#include <ti/driverlib/driverlib.h>

#define PWM_TIMER_COUNT 7u
#define PWM_CENTER_FLAG 0x80u

extern const native_module_header_t g_native_module_header;

static unsigned pwm_slot(void)
{
    uintptr_t address = (uintptr_t)&g_native_module_header;
    if (address < NATIVE_MODULE_SLOT_ADDR ||
            address >= NATIVE_MODULE_SLOT_ADDR +
                NATIVE_MODULE_SLOT_COUNT * NATIVE_MODULE_SLOT_SIZE) {
        return NATIVE_MODULE_SLOT_COUNT;
    }
    return (unsigned)((address - NATIVE_MODULE_SLOT_ADDR) /
        NATIVE_MODULE_SLOT_SIZE);
}

typedef struct {
    uint16_t top;
    uint8_t prescale;
    uint8_t active;
} pwm_timer_state_t;

typedef struct {
    pwm_timer_state_t timer[PWM_TIMER_COUNT];
} pwm_state_t;

static pwm_state_t *pwm_state(void)
{
    return (pwm_state_t *)NATIVE_CORE_API->module_state(pwm_slot(),
        sizeof(pwm_state_t));
}

static GPTIMER_Regs *timer_regs(unsigned timer)
{
    switch (timer) {
        case G3507_TIMER_TIMA0: return TIMA0;
        case G3507_TIMER_TIMA1: return TIMA1;
        case G3507_TIMER_TIMG0: return TIMG0;
        case G3507_TIMER_TIMG6: return TIMG6;
        case G3507_TIMER_TIMG7: return TIMG7;
        case G3507_TIMER_TIMG8: return TIMG8;
        case G3507_TIMER_TIMG12: return TIMG12;
        default: return 0;
    }
}

static uint8_t timer_resource(unsigned timer)
{
    return (uint8_t)(timer < G3507_TIMER_TIMG0
        ? BOARD_RES_TIMA0 + timer : BOARD_RES_TIMG0 + timer - 2u);
}

static void pin_name(unsigned id, char name[5])
{
    unsigned number = id;
    name[0] = 'P';
    name[1] = 'A';
    if (number >= 32u) {
        name[1] = 'B';
        number -= 32u;
    }
    if (number >= 10u) {
        name[2] = (char)('0' + number / 10u);
        name[3] = (char)('0' + number % 10u);
        name[4] = 0;
    } else {
        name[2] = (char)('0' + number);
        name[3] = 0;
        name[4] = 0;
    }
}

/* State storage is deliberately limited to 32 bytes per module slot.  PWM
 * can own more than four pins through complementary outputs, so release its
 * owned pins by owner rather than trying to retain every pin name in state. */
static void release_pwm_pins(void)
{
    unsigned id;
    for (id = 0; id < 60u; id++) {
        char name[5];
        pin_name(id, name);
        if (NATIVE_CORE_API->pin_owner(name) == PIN_OWN_PWM) {
            (void)NATIVE_CORE_API->pin_af(name, 0, 0);
            NATIVE_CORE_API->pin_release(name, PIN_OWN_PWM);
        }
    }
}

static void stop_timer(unsigned timer, pwm_timer_state_t *state)
{
    GPTIMER_Regs *regs = timer_regs(timer);
    DL_Timer_stopCounter(regs);
    DL_Timer_reset(regs);
    DL_Timer_disablePower(regs);
    NATIVE_CORE_API->resource_release(timer_resource(timer), PIN_OWN_PWM);
    state->active = 0u;
    state->top = 0u;
    state->prescale = 0u;
}

static DL_TIMER_CC_INDEX cc_index(unsigned channel)
{
    return (DL_TIMER_CC_INDEX)channel;
}

static uint32_t cc_direction(unsigned channel, int output)
{
    static const uint32_t outputs[] = {
        DL_TIMER_CC0_OUTPUT, DL_TIMER_CC1_OUTPUT,
        DL_TIMER_CC2_OUTPUT, DL_TIMER_CC3_OUTPUT,
    };
    static const uint32_t inputs[] = {
        DL_TIMER_CC0_INPUT, DL_TIMER_CC1_INPUT,
        DL_TIMER_CC2_INPUT, DL_TIMER_CC3_INPUT,
    };
    return output ? outputs[channel] : inputs[channel];
}

static int route_for(unsigned timer, const native_pin_t *pin,
    int complementary, unsigned *pf, unsigned *channel)
{
    unsigned i;
    for (i = 0; i < sizeof(g3507_timer_routes) /
            sizeof(g3507_timer_routes[0]); i++) {
        uint16_t route = g3507_timer_routes[i];
        if (G3507_TIMER_ROUTE_IOMUX(route) == pin->iomux &&
                G3507_TIMER_ROUTE_INST(route) == timer &&
                G3507_TIMER_ROUTE_CMPL(route) == !!complementary) {
            *pf = G3507_TIMER_ROUTE_PF(route);
            *channel = G3507_TIMER_ROUTE_CH(route);
            return 0;
        }
    }
    return -1;
}

static int auto_route(const native_pin_t *pin, unsigned *timer,
    unsigned *pf, unsigned *channel)
{
    static const uint8_t preference[] = {
        G3507_TIMER_TIMG12, G3507_TIMER_TIMG7, G3507_TIMER_TIMG6,
        G3507_TIMER_TIMG8, G3507_TIMER_TIMA0, G3507_TIMER_TIMA1,
    };
    unsigned i;
    for (i = 0; i < sizeof(preference); i++) {
        if (route_for(preference[i], pin, 0, pf, channel) == 0) {
            *timer = preference[i];
            return 0;
        }
    }
    return -1;
}

static int timer_geometry(uint32_t frequency, unsigned center,
    uint8_t *prescale_out, uint16_t *top_out)
{
    uint32_t source = NATIVE_CORE_API->bus_clock_hz();
    uint32_t prescale;
    if (frequency < 1u || frequency > source / 2u) return -1;
    for (prescale = 0; prescale < 256u; prescale++) {
        uint32_t counts = (source / (prescale + 1u)) / frequency;
        uint32_t top = center ? counts / 2u : counts;
        if (counts <= 65535u && top >= 2u) {
            *prescale_out = (uint8_t)prescale;
            *top_out = (uint16_t)top;
            return 0;
        }
    }
    return -1;
}

static int start_timer(lua_State *L, unsigned timer, uint32_t frequency,
    unsigned center, pwm_timer_state_t *state)
{
    GPTIMER_Regs *regs = timer_regs(timer);
    uint8_t prescale;
    uint16_t top;
    DL_Timer_ClockConfig clock;
    DL_Timer_PWMConfig pwm;
    if (!regs || timer == G3507_TIMER_TIMG0 ||
            timer_geometry(frequency, center, &prescale, &top) != 0) {
        return NATIVE_CORE_API->raise_error(L, "pwm:range");
    }
    if (state->active) {
        if (state->top != top || state->prescale != prescale ||
                !!(state->active & PWM_CENTER_FLAG) != !!center) {
            return NATIVE_CORE_API->raise_error(L, "pwm:shared_freq");
        }
        return 0;
    }
    if (NATIVE_CORE_API->resource_claim(timer_resource(timer),
            PIN_OWN_PWM) != 0) {
        return NATIVE_CORE_API->raise_error(L, "pwm:timer_busy");
    }
    DL_Timer_reset(regs);
    DL_Timer_enablePower(regs);
    delay_cycles(16);
    clock.clockSel = DL_TIMER_CLOCK_BUSCLK;
    clock.divideRatio = DL_TIMER_CLOCK_DIVIDE_1;
    clock.prescale = prescale;
    pwm.pwmMode = center ? DL_TIMER_PWM_MODE_CENTER_ALIGN
                         : DL_TIMER_PWM_MODE_EDGE_ALIGN;
    pwm.period = center ? (uint16_t)(top * 2u) : top;
    pwm.isTimerWithFourCC = timer <= G3507_TIMER_TIMA1;
    pwm.startTimer = DL_TIMER_STOP;
    DL_Timer_setClockConfig(regs, &clock);
    DL_Timer_initPWMMode(regs, &pwm);
    DL_Timer_enableClock(regs);
    DL_Timer_startCounter(regs);
    state->top = top;
    state->prescale = prescale;
    state->active = center ? PWM_CENTER_FLAG : 0u;
    return 0;
}

static int open_route(lua_State *L, unsigned timer, const char *pin_name,
    uint32_t frequency, unsigned duty, unsigned center, unsigned invert,
    int complementary, unsigned source_deadband)
{
    native_pin_t pin;
    pwm_state_t *all = pwm_state();
    pwm_timer_state_t *state;
    GPTIMER_Regs *regs;
    unsigned pf;
    unsigned channel;
    uint32_t compare;
    if (!all || timer >= PWM_TIMER_COUNT || duty > 100u || center > 1u ||
            invert > 1u || NATIVE_CORE_API->pin_resolve(pin_name, &pin) != 0 ||
            route_for(timer, &pin, complementary, &pf, &channel) != 0) {
        return NATIVE_CORE_API->raise_error(L, "pwm:pin");
    }
    state = &all->timer[timer];
    if (state->active & (1u << channel)) {
        return NATIVE_CORE_API->raise_error(L, "pwm:channel_busy");
    }
    if (NATIVE_CORE_API->pin_owner(pin_name) == PIN_OWN_PWM ||
            NATIVE_CORE_API->pin_claim(pin_name, PIN_OWN_PWM) != 0) {
        return NATIVE_CORE_API->raise_error(L, "pwm:pin");
    }
    if (start_timer(L, timer, frequency, center, state) != 0) {
        NATIVE_CORE_API->pin_release(pin_name, PIN_OWN_PWM);
        return -1;
    }
    regs = timer_regs(timer);
    if (NATIVE_CORE_API->pin_af(pin_name, pf, 0) != 0) {
        NATIVE_CORE_API->pin_release(pin_name, PIN_OWN_PWM);
        if ((state->active & 0x0fu) == 0u) {
            stop_timer(timer, state);
        }
        return NATIVE_CORE_API->raise_error(L, "pwm:af");
    }
    ((GPIO_Regs *)pin.port)->DOESET31_0 = pin.pin;
    DL_Timer_setCaptureCompareOutCtl(regs, DL_TIMER_CC_OCTL_INIT_VAL_LOW,
        invert ? DL_TIMER_CC_OCTL_INV_OUT_ENABLED
               : DL_TIMER_CC_OCTL_INV_OUT_DISABLED,
        source_deadband ? DL_TIMER_CC_OCTL_SRC_DEAD_BAND
                        : DL_TIMER_CC_OCTL_SRC_FUNCVAL,
        cc_index(channel));
    DL_Timer_setCaptCompUpdateMethod(regs, DL_TIMER_CC_UPDATE_METHOD_IMMEDIATE,
        cc_index(channel));
    compare = ((uint32_t)state->top * duty) / 100u;
    DL_Timer_setCaptureCompareValue(regs, compare, cc_index(channel));
    DL_Timer_setCCPDirection(regs, cc_direction(channel, 1));
    state->active |= (uint8_t)(1u << channel);
    NATIVE_CORE_API->push_integer(L, (int32_t)(timer * 4u + channel));
    return 1;
}

static int l_pwm_open(lua_State *L)
{
    const char *pin_name = NATIVE_CORE_API->opt_string(L, 1, "PA14");
    uint32_t frequency = (uint32_t)NATIVE_CORE_API->opt_integer(L, 2, 1000);
    unsigned duty = (unsigned)NATIVE_CORE_API->opt_integer(L, 3, 0);
    unsigned center = (unsigned)NATIVE_CORE_API->opt_integer(L, 4, 0);
    unsigned invert = (unsigned)NATIVE_CORE_API->opt_integer(L, 5, 0);
    native_pin_t pin;
    unsigned timer;
    unsigned pf;
    unsigned channel;
    if (NATIVE_CORE_API->pin_resolve(pin_name, &pin) != 0 ||
            auto_route(&pin, &timer, &pf, &channel) != 0) {
        return NATIVE_CORE_API->raise_error(L, "pwm:pin");
    }
    return open_route(L, timer, pin_name, frequency, duty, center, invert, 0, 0);
}

static int l_pwm_open_on(lua_State *L)
{
    return open_route(L, (unsigned)NATIVE_CORE_API->check_integer(L, 1),
        NATIVE_CORE_API->check_string(L, 2),
        (uint32_t)NATIVE_CORE_API->check_integer(L, 3),
        (unsigned)NATIVE_CORE_API->opt_integer(L, 4, 0),
        (unsigned)NATIVE_CORE_API->opt_integer(L, 5, 0),
        (unsigned)NATIVE_CORE_API->opt_integer(L, 6, 0), 0, 0);
}

static int l_pwm_duty(lua_State *L)
{
    unsigned handle = (unsigned)NATIVE_CORE_API->check_integer(L, 1);
    unsigned duty = (unsigned)NATIVE_CORE_API->check_integer(L, 2);
    unsigned timer = handle / 4u;
    unsigned channel = handle & 3u;
    pwm_state_t *all = pwm_state();
    if (!all || timer >= PWM_TIMER_COUNT || duty > 100u ||
            !(all->timer[timer].active & (1u << channel))) {
        return NATIVE_CORE_API->raise_error(L, "pwm:id");
    }
    DL_Timer_setCaptureCompareValue(timer_regs(timer),
        ((uint32_t)all->timer[timer].top * duty) / 100u, cc_index(channel));
    return 0;
}

static int close_route(lua_State *L, unsigned handle, const char *pin_name,
    int complementary)
{
    unsigned timer = handle / 4u;
    unsigned channel = handle & 3u;
    unsigned pf;
    unsigned routed_channel;
    native_pin_t pin;
    pwm_state_t *all = pwm_state();
    pwm_timer_state_t *state;
    GPTIMER_Regs *regs;
    if (!all || timer >= PWM_TIMER_COUNT ||
            NATIVE_CORE_API->pin_resolve(pin_name, &pin) != 0 ||
            route_for(timer, &pin, complementary, &pf, &routed_channel) != 0 ||
            routed_channel != channel ||
            !(all->timer[timer].active & (1u << channel)) ||
            NATIVE_CORE_API->pin_owner(pin_name) != PIN_OWN_PWM) {
        return NATIVE_CORE_API->raise_error(L, "pwm:id");
    }
    state = &all->timer[timer];
    regs = timer_regs(timer);
    DL_Timer_setCaptureCompareValue(regs, 0, cc_index(channel));
    DL_Timer_setCCPDirection(regs, cc_direction(channel, 0));
    board_reg_pin_out((GPIO_Regs *)pin.port, pin.pin, pin.iomux);
    board_reg_gpio_clr((GPIO_Regs *)pin.port, pin.pin);
    NATIVE_CORE_API->pin_release(pin_name, PIN_OWN_PWM);
    state->active &= (uint8_t)~(1u << channel);
    if ((state->active & 0x0fu) == 0u) {
        stop_timer(timer, state);
    }
    return 0;
}

static int l_pwm_close(lua_State *L)
{
    return close_route(L, (unsigned)NATIVE_CORE_API->check_integer(L, 1),
        NATIVE_CORE_API->check_string(L, 2), 0);
}

static int l_pwm_open_pair(lua_State *L)
{
    unsigned timer = (unsigned)NATIVE_CORE_API->check_integer(L, 1);
    const char *high = NATIVE_CORE_API->check_string(L, 2);
    const char *low = NATIVE_CORE_API->check_string(L, 3);
    uint32_t frequency = (uint32_t)NATIVE_CORE_API->check_integer(L, 4);
    unsigned duty = (unsigned)NATIVE_CORE_API->opt_integer(L, 5, 50);
    int32_t dead_value = NATIVE_CORE_API->opt_integer(L, 6, 0);
    uint32_t dead_ns = (uint32_t)dead_value;
    unsigned center = (unsigned)NATIVE_CORE_API->opt_integer(L, 7, 0);
    native_pin_t high_pin;
    native_pin_t low_pin;
    unsigned high_pf;
    unsigned low_pf;
    unsigned high_channel;
    unsigned low_channel;
    uint32_t timer_hz;
    uint32_t dead_ticks;
    uint32_t tick_ns;
    int result;
    if (timer > G3507_TIMER_TIMA1 || dead_value < 0 ||
            NATIVE_CORE_API->pin_resolve(high, &high_pin) != 0 ||
            NATIVE_CORE_API->pin_resolve(low, &low_pin) != 0 ||
            high_pin.iomux == low_pin.iomux ||
            NATIVE_CORE_API->pin_owner(low) == PIN_OWN_PWM ||
            route_for(timer, &high_pin, 0, &high_pf, &high_channel) != 0 ||
            route_for(timer, &low_pin, 1, &low_pf, &low_channel) != 0 ||
            high_channel != low_channel) {
        return NATIVE_CORE_API->raise_error(L, "pwm:pair");
    }
    result = open_route(L, timer, high, frequency, duty, center, 0, 0, 1);
    if (result != 1) return result;
    if (NATIVE_CORE_API->pin_claim(low, PIN_OWN_PWM) != 0 ||
            NATIVE_CORE_API->pin_af(low, low_pf, 0) != 0) {
        if (NATIVE_CORE_API->pin_owner(low) == PIN_OWN_PWM) {
            (void)NATIVE_CORE_API->pin_af(low, 0, 0);
            NATIVE_CORE_API->pin_release(low, PIN_OWN_PWM);
        }
        (void)close_route(L, timer * 4u + high_channel, high, 0);
        return NATIVE_CORE_API->raise_error(L, "pwm:pair_busy");
    }
    ((GPIO_Regs *)low_pin.port)->DOESET31_0 = low_pin.pin;
    timer_hz = NATIVE_CORE_API->bus_clock_hz() /
        ((uint32_t)pwm_state()->timer[timer].prescale + 1u);
    tick_ns = (1000000000u + timer_hz - 1u) / timer_hz;
    dead_ticks = dead_ns ? (dead_ns + tick_ns - 1u) / tick_ns : 0u;
    if (dead_ns && !dead_ticks) dead_ticks = 1u;
    if (dead_ticks > 255u) dead_ticks = 255u;
    DL_Timer_setDeadBand(timer_regs(timer), (uint16_t)dead_ticks,
        (uint16_t)dead_ticks, DL_TIMER_DEAD_BAND_MODE_0);
    return 1;
}

static int l_pwm_close_pair(lua_State *L)
{
    unsigned handle = (unsigned)NATIVE_CORE_API->check_integer(L, 1);
    const char *high = NATIVE_CORE_API->check_string(L, 2);
    const char *low = NATIVE_CORE_API->check_string(L, 3);
    native_pin_t high_pin;
    native_pin_t low_pin;
    unsigned high_pf;
    unsigned low_pf;
    unsigned high_channel;
    unsigned low_channel;
    unsigned timer = handle / 4u;
    pwm_state_t *state = pwm_state();
    if (!state || timer >= PWM_TIMER_COUNT ||
            NATIVE_CORE_API->pin_resolve(high, &high_pin) != 0 ||
            NATIVE_CORE_API->pin_resolve(low, &low_pin) != 0 ||
            high_pin.iomux == low_pin.iomux ||
            route_for(timer, &high_pin, 0, &high_pf, &high_channel) != 0 ||
            route_for(timer, &low_pin, 1, &low_pf, &low_channel) != 0 ||
            high_channel != (handle & 3u) || low_channel != high_channel ||
            !(state->timer[timer].active & (1u << high_channel)) ||
            NATIVE_CORE_API->pin_owner(high) != PIN_OWN_PWM ||
            NATIVE_CORE_API->pin_owner(low) != PIN_OWN_PWM) {
        return NATIVE_CORE_API->raise_error(L, "pwm:pair");
    }
    board_reg_pin_out((GPIO_Regs *)low_pin.port, low_pin.pin, low_pin.iomux);
    board_reg_gpio_clr((GPIO_Regs *)low_pin.port, low_pin.pin);
    NATIVE_CORE_API->pin_release(low, PIN_OWN_PWM);
    return close_route(L, handle, high, 0);
}

static int l_pwm_route(lua_State *L)
{
    unsigned timer = (unsigned)NATIVE_CORE_API->check_integer(L, 1);
    native_pin_t pin;
    unsigned pf;
    unsigned channel;
    int result = -1;
    if (timer < PWM_TIMER_COUNT && NATIVE_CORE_API->pin_resolve(
            NATIVE_CORE_API->check_string(L, 2), &pin) == 0 &&
            route_for(timer, &pin, 0, &pf, &channel) == 0) {
        result = (int)channel;
    }
    NATIVE_CORE_API->push_integer(L, result);
    return 1;
}

static const native_lua_reg_t k_pwm_functions[] = {
    {"open", l_pwm_open}, {"open_on", l_pwm_open_on},
    {"duty", l_pwm_duty}, {"close", l_pwm_close},
    {"open_pair", l_pwm_open_pair}, {"close_pair", l_pwm_close_pair},
    {"route", l_pwm_route}, {0, 0},
};

static int pwm_init(lua_State *L, const native_core_api_t *api)
{
    pwm_state_t *state;
    unsigned i;
    if (!api || api->magic != NATIVE_CORE_API_MAGIC ||
            api->abi_version != NATIVE_CORE_ABI_VERSION ||
            api->struct_size < sizeof(native_core_api_t)) {
        return -1;
    }
    state = (pwm_state_t *)api->module_state(pwm_slot(), sizeof(*state));
    if (!state) return -1;
    for (i = 0; i < PWM_TIMER_COUNT; i++) state->timer[i].active = 0u;
    return api->register_lua_module(L, "pwm", k_pwm_functions);
}

static void pwm_deinit(void)
{
    pwm_state_t *state = pwm_state();
    unsigned timer;
    if (!state) return;
    release_pwm_pins();
    for (timer = 0; timer < PWM_TIMER_COUNT; timer++) {
        if (state->timer[timer].active) {
            stop_timer(timer, &state->timer[timer]);
        }
    }
}

const native_module_header_t g_native_module_header
    __attribute__((section(".module_header"), used, aligned(4))) = {
        NATIVE_MODULE_MAGIC, NATIVE_MODULE_FORMAT, NATIVE_CORE_ABI_VERSION,
        0, 0, sizeof(native_module_header_t), pwm_init, pwm_deinit, "pwm",
    };
