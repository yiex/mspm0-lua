#include "native_module.h"

#include "board_pins.h"
#include "board_reg.h"
#include "board_resource.h"
#include "g3507_pin_routes.h"
#include <ti/driverlib/driverlib.h>

#define SOFT_TIMER_COUNT 4u
#define HW_TIMER_COUNT 7u

extern const native_module_header_t g_native_module_header;

typedef struct {
    uint8_t mode[HW_TIMER_COUNT]; /* 0 free, 1 counter, 2 capture */
    uint8_t capture_iomux[HW_TIMER_COUNT];
    uint8_t capture_channel[HW_TIMER_COUNT];
    uint8_t capture_pin_id[HW_TIMER_COUNT];
} tmr_state_t;

static unsigned tmr_slot(void)
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

static tmr_state_t *tmr_state(void)
{
    return (tmr_state_t *)NATIVE_CORE_API->module_state(
        tmr_slot(), sizeof(tmr_state_t));
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

static int native_pin_id(const native_pin_t *pin)
{
    unsigned bit;
    for (bit = 0; bit < 32u; bit++) {
        if (pin->pin == (1u << bit)) {
            if (pin->port == (uintptr_t)GPIOA) return (int)bit;
            if (pin->port == (uintptr_t)GPIOB && bit < 28u) {
                return (int)(32u + bit);
            }
            break;
        }
    }
    return -1;
}

static void pin_name(uint8_t id, char name[5])
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

static int timer_id(lua_State *L, int index)
{
    int id = (int)NATIVE_CORE_API->check_integer(L, index);
    if (id < 0 || id >= (int)SOFT_TIMER_COUNT) return -1;
    return id;
}

static int hw_timer_id(lua_State *L, int index)
{
    int id = (int)NATIVE_CORE_API->check_integer(L, index);
    if (id < 0 || id >= (int)HW_TIMER_COUNT ||
            id == G3507_TIMER_TIMG0) return -1;
    return id;
}

static int capture_route(unsigned timer, const native_pin_t *pin,
    unsigned *pf, unsigned *channel)
{
    unsigned i;
    for (i = 0; i < sizeof(g3507_timer_routes) /
            sizeof(g3507_timer_routes[0]); i++) {
        uint16_t route = g3507_timer_routes[i];
        if (G3507_TIMER_ROUTE_IOMUX(route) == pin->iomux &&
                G3507_TIMER_ROUTE_INST(route) == timer &&
                !G3507_TIMER_ROUTE_CMPL(route)) {
            *pf = G3507_TIMER_ROUTE_PF(route);
            *channel = G3507_TIMER_ROUTE_CH(route);
            return 0;
        }
    }
    return -1;
}

static uint32_t cc_event(unsigned channel)
{
    static const uint32_t events[] = {
        DL_TIMER_INTERRUPT_CC0_DN_EVENT, DL_TIMER_INTERRUPT_CC1_DN_EVENT,
        DL_TIMER_INTERRUPT_CC2_DN_EVENT, DL_TIMER_INTERRUPT_CC3_DN_EVENT,
    };
    return events[channel];
}

static int l_tmr_start(lua_State *L)
{
    int id = timer_id(L, 1);
    int32_t period = NATIVE_CORE_API->check_integer(L, 2);
    if (id < 0 || period < 1 || NATIVE_CORE_API->timer_start(
            (unsigned)id, (uint32_t)period) != 0) {
        return NATIVE_CORE_API->raise_error(L, "tmr:start");
    }
    return 0;
}

static int l_tmr_ready(lua_State *L)
{
    int id = timer_id(L, 1);
    if (id < 0) return NATIVE_CORE_API->raise_error(L, "tmr:id");
    NATIVE_CORE_API->push_boolean(L,
        NATIVE_CORE_API->timer_take((unsigned)id) != 0u);
    return 1;
}

static int l_tmr_take(lua_State *L)
{
    int id = timer_id(L, 1);
    if (id < 0) return NATIVE_CORE_API->raise_error(L, "tmr:id");
    NATIVE_CORE_API->push_integer(L,
        (int32_t)NATIVE_CORE_API->timer_take((unsigned)id));
    return 1;
}

static int l_tmr_stop(lua_State *L)
{
    int id = timer_id(L, 1);
    if (id < 0) return NATIVE_CORE_API->raise_error(L, "tmr:id");
    NATIVE_CORE_API->timer_stop((unsigned)id);
    return 0;
}

static int l_tmr_millis(lua_State *L)
{
    NATIVE_CORE_API->push_integer(L, (int32_t)NATIVE_CORE_API->millis());
    return 1;
}

static int l_tmr_delay(lua_State *L)
{
    int32_t ms = NATIVE_CORE_API->check_integer(L, 1);
    if (ms < 0) return NATIVE_CORE_API->raise_error(L, "tmr:delay");
    NATIVE_CORE_API->delay_ms((uint32_t)ms);
    return 0;
}

static int l_tmr_hw_start(lua_State *L)
{
    tmr_state_t *state = tmr_state();
    int timer = hw_timer_id(L, 1);
    int32_t ticks = NATIVE_CORE_API->check_integer(L, 2);
    int32_t prescale = NATIVE_CORE_API->opt_integer(L, 3, 0);
    int periodic = NATIVE_CORE_API->opt_integer(L, 4, 1) != 0;
    GPTIMER_Regs *regs;
    DL_Timer_ClockConfig clock;
    DL_Timer_TimerConfig config;
    if (!state || timer < 0 || ticks < 2 || ticks > 65536 || prescale < 0 ||
            prescale > 255 || state->mode[timer] != 0u ||
            NATIVE_CORE_API->resource_claim(
                timer_resource((unsigned)timer), PIN_OWN_CAP) != 0) {
        return NATIVE_CORE_API->raise_error(L, "tmr:hw_start");
    }
    regs = timer_regs((unsigned)timer);
    DL_Timer_reset(regs);
    DL_Timer_enablePower(regs);
    delay_cycles(16);
    clock.clockSel = DL_TIMER_CLOCK_BUSCLK;
    clock.divideRatio = DL_TIMER_CLOCK_DIVIDE_1;
    clock.prescale = (uint8_t)prescale;
    config.timerMode = periodic ? DL_TIMER_TIMER_MODE_PERIODIC
                                : DL_TIMER_TIMER_MODE_ONE_SHOT;
    config.period = (uint32_t)ticks - 1u;
    config.startTimer = DL_TIMER_STOP;
    config.genIntermInt = DL_TIMER_INTERM_INT_DISABLED;
    config.counterVal = 0;
    DL_Timer_setClockConfig(regs, &clock);
    DL_Timer_initTimerMode(regs, &config);
    DL_Timer_clearInterruptStatus(regs, DL_TIMER_INTERRUPT_ZERO_EVENT);
    DL_Timer_enableClock(regs);
    DL_Timer_startCounter(regs);
    state->mode[timer] = 1u;
    return 0;
}

static int l_tmr_hw_value(lua_State *L)
{
    tmr_state_t *state = tmr_state();
    int timer = hw_timer_id(L, 1);
    if (!state || timer < 0 || state->mode[timer] != 1u) {
        return NATIVE_CORE_API->raise_error(L, "tmr:hw_id");
    }
    NATIVE_CORE_API->push_integer(L,
        (int32_t)DL_Timer_getTimerCount(timer_regs((unsigned)timer)));
    return 1;
}

static int l_tmr_hw_ready(lua_State *L)
{
    tmr_state_t *state = tmr_state();
    int timer = hw_timer_id(L, 1);
    uint32_t status;
    if (!state || timer < 0 || state->mode[timer] != 1u) {
        return NATIVE_CORE_API->raise_error(L, "tmr:hw_id");
    }
    status = DL_Timer_getRawInterruptStatus(timer_regs((unsigned)timer),
        DL_TIMER_INTERRUPT_ZERO_EVENT);
    if (status) DL_Timer_clearInterruptStatus(timer_regs((unsigned)timer),
        DL_TIMER_INTERRUPT_ZERO_EVENT);
    NATIVE_CORE_API->push_boolean(L, status != 0u);
    return 1;
}

static int l_tmr_hw_stop(lua_State *L)
{
    tmr_state_t *state = tmr_state();
    int timer = hw_timer_id(L, 1);
    if (!state || timer < 0 || state->mode[timer] != 1u) {
        return NATIVE_CORE_API->raise_error(L, "tmr:hw_id");
    }
    DL_Timer_stopCounter(timer_regs((unsigned)timer));
    DL_Timer_reset(timer_regs((unsigned)timer));
    DL_Timer_disablePower(timer_regs((unsigned)timer));
    NATIVE_CORE_API->resource_release(timer_resource((unsigned)timer),
        PIN_OWN_CAP);
    state->mode[timer] = 0u;
    return 0;
}

static int l_tmr_capture_open(lua_State *L)
{
    tmr_state_t *state = tmr_state();
    int timer = hw_timer_id(L, 1);
    const char *pin_name = NATIVE_CORE_API->check_string(L, 2);
    int edge = NATIVE_CORE_API->opt_integer(L, 3, 0);
    int prescale = NATIVE_CORE_API->opt_integer(L, 4, 0);
    native_pin_t pin;
    unsigned pf;
    unsigned channel;
    int pin_id;
    GPTIMER_Regs *regs;
    DL_Timer_ClockConfig clock;
    DL_Timer_CaptureConfig config;
    if (!state || timer < 0 || edge < 0 || edge > 2 || prescale < 0 ||
            prescale > 255 || state->mode[timer] != 0u ||
            NATIVE_CORE_API->pin_resolve(pin_name, &pin) != 0) {
        return NATIVE_CORE_API->raise_error(L, "tmr:capture");
    }
    pin_id = native_pin_id(&pin);
    if (pin_id < 0 ||
            capture_route((unsigned)timer, &pin, &pf, &channel) != 0 ||
            NATIVE_CORE_API->pin_claim(pin_name, PIN_OWN_CAP) != 0 ||
            NATIVE_CORE_API->resource_claim(timer_resource((unsigned)timer),
                PIN_OWN_CAP) != 0) {
        NATIVE_CORE_API->pin_release(pin_name, PIN_OWN_CAP);
        return NATIVE_CORE_API->raise_error(L, "tmr:capture");
    }
    if (NATIVE_CORE_API->pin_af(pin_name, pf, 1) != 0) {
        NATIVE_CORE_API->pin_release(pin_name, PIN_OWN_CAP);
        NATIVE_CORE_API->resource_release(timer_resource((unsigned)timer),
            PIN_OWN_CAP);
        return NATIVE_CORE_API->raise_error(L, "tmr:capture_af");
    }
    regs = timer_regs((unsigned)timer);
    DL_Timer_reset(regs);
    DL_Timer_enablePower(regs);
    delay_cycles(16);
    clock.clockSel = DL_TIMER_CLOCK_BUSCLK;
    clock.divideRatio = DL_TIMER_CLOCK_DIVIDE_1;
    clock.prescale = (uint8_t)prescale;
    config.captureMode = DL_TIMER_CAPTURE_MODE_EDGE_TIME;
    config.period = 0xffffu;
    config.startTimer = DL_TIMER_STOP;
    config.edgeCaptMode = edge == 1 ? DL_TIMER_CAPTURE_EDGE_DETECTION_MODE_FALLING
        : edge == 2 ? DL_TIMER_CAPTURE_EDGE_DETECTION_MODE_EDGE
                    : DL_TIMER_CAPTURE_EDGE_DETECTION_MODE_RISING;
    config.inputChan = (DL_TIMER_INPUT_CHAN)channel;
    config.inputInvMode = DL_TIMER_CC_INPUT_INV_NOINVERT;
    DL_Timer_setClockConfig(regs, &clock);
    DL_Timer_initCaptureMode(regs, &config);
    DL_Timer_clearInterruptStatus(regs, cc_event(channel));
    DL_Timer_enableClock(regs);
    DL_Timer_startCounter(regs);
    state->mode[timer] = 2u;
    state->capture_iomux[timer] = (uint8_t)pin.iomux;
    state->capture_channel[timer] = (uint8_t)channel;
    state->capture_pin_id[timer] = (uint8_t)pin_id;
    NATIVE_CORE_API->push_integer(L, timer * 4 + (int)channel);
    return 1;
}

static int l_tmr_capture_ready(lua_State *L)
{
    tmr_state_t *state = tmr_state();
    unsigned handle = (unsigned)NATIVE_CORE_API->check_integer(L, 1);
    unsigned timer = handle / 4u;
    unsigned channel = handle & 3u;
    uint32_t status;
    if (!state || timer >= HW_TIMER_COUNT || timer == G3507_TIMER_TIMG0 ||
            state->mode[timer] != 2u ||
            state->capture_channel[timer] != channel) {
        return NATIVE_CORE_API->raise_error(L, "tmr:capture_id");
    }
    status = DL_Timer_getRawInterruptStatus(timer_regs(timer), cc_event(channel));
    NATIVE_CORE_API->push_boolean(L, status != 0u);
    return 1;
}

static int l_tmr_capture_read(lua_State *L)
{
    tmr_state_t *state = tmr_state();
    unsigned handle = (unsigned)NATIVE_CORE_API->check_integer(L, 1);
    unsigned timer = handle / 4u;
    unsigned channel = handle & 3u;
    if (!state || timer >= HW_TIMER_COUNT || timer == G3507_TIMER_TIMG0 ||
            state->mode[timer] != 2u ||
            state->capture_channel[timer] != channel) {
        return NATIVE_CORE_API->raise_error(L, "tmr:capture_id");
    }
    DL_Timer_clearInterruptStatus(timer_regs(timer), cc_event(channel));
    NATIVE_CORE_API->push_integer(L, (int32_t)DL_Timer_getCaptureCompareValue(
        timer_regs(timer), (DL_TIMER_CC_INDEX)channel));
    return 1;
}

static int l_tmr_capture_close(lua_State *L)
{
    tmr_state_t *state = tmr_state();
    unsigned handle = (unsigned)NATIVE_CORE_API->check_integer(L, 1);
    unsigned timer = handle / 4u;
    unsigned channel = handle & 3u;
    const char *pin_name = NATIVE_CORE_API->check_string(L, 2);
    native_pin_t pin;
    unsigned pf;
    unsigned routed_channel;
    if (!state || timer >= HW_TIMER_COUNT || timer == G3507_TIMER_TIMG0 ||
            state->mode[timer] != 2u ||
            NATIVE_CORE_API->pin_resolve(pin_name, &pin) != 0 ||
            capture_route(timer, &pin, &pf, &routed_channel) != 0 ||
            routed_channel != channel || state->capture_channel[timer] != channel ||
            state->capture_iomux[timer] != pin.iomux) {
        return NATIVE_CORE_API->raise_error(L, "tmr:capture_id");
    }
    DL_Timer_stopCounter(timer_regs(timer));
    DL_Timer_reset(timer_regs(timer));
    DL_Timer_disablePower(timer_regs(timer));
    ((GPIO_Regs *)pin.port)->DOECLR31_0 = pin.pin;
    DL_GPIO_initDigitalInput(pin.iomux);
    NATIVE_CORE_API->pin_release(pin_name, PIN_OWN_CAP);
    NATIVE_CORE_API->resource_release(timer_resource(timer), PIN_OWN_CAP);
    state->mode[timer] = 0u;
    return 0;
}

static int l_tmr_route(lua_State *L)
{
    int timer = hw_timer_id(L, 1);
    native_pin_t pin;
    unsigned pf;
    unsigned channel;
    int result = -1;
    if (timer >= 0 && NATIVE_CORE_API->pin_resolve(
            NATIVE_CORE_API->check_string(L, 2), &pin) == 0 &&
            capture_route((unsigned)timer, &pin, &pf, &channel) == 0) {
        result = (int)channel;
    }
    NATIVE_CORE_API->push_integer(L, result);
    return 1;
}

static const native_lua_reg_t k_tmr_functions[] = {
    {"start", l_tmr_start}, {"ready", l_tmr_ready}, {"take", l_tmr_take},
    {"stop", l_tmr_stop}, {"millis", l_tmr_millis}, {"delay", l_tmr_delay},
    {"hw_start", l_tmr_hw_start}, {"hw_value", l_tmr_hw_value},
    {"hw_ready", l_tmr_hw_ready}, {"hw_stop", l_tmr_hw_stop},
    {"capture_open", l_tmr_capture_open},
    {"capture_ready", l_tmr_capture_ready},
    {"capture_read", l_tmr_capture_read},
    {"capture_close", l_tmr_capture_close}, {"route", l_tmr_route}, {0, 0},
};

static int tmr_init(lua_State *L, const native_core_api_t *api)
{
    tmr_state_t *state;
    unsigned i;
    if (!api || api->magic != NATIVE_CORE_API_MAGIC ||
            api->abi_version != NATIVE_CORE_ABI_VERSION ||
            api->struct_size < sizeof(native_core_api_t)) return -1;
    state = (tmr_state_t *)api->module_state(tmr_slot(), sizeof(*state));
    if (!state) return -1;
    for (i = 0; i < HW_TIMER_COUNT; i++) state->mode[i] = 0u;
    return api->register_lua_module(L, "tmr", k_tmr_functions);
}

static void tmr_deinit(void)
{
    tmr_state_t *state = tmr_state();
    unsigned timer;
    if (!state) return;
    for (timer = 0; timer < HW_TIMER_COUNT; timer++) {
        if (state->mode[timer]) {
            GPTIMER_Regs *regs = timer_regs(timer);
            if (state->mode[timer] == 2u) {
                char pin_name_buffer[5];
                pin_name(state->capture_pin_id[timer], pin_name_buffer);
                (void)NATIVE_CORE_API->pin_af(pin_name_buffer, 0, 0);
                NATIVE_CORE_API->pin_release(pin_name_buffer, PIN_OWN_CAP);
            }
            DL_Timer_stopCounter(regs);
            DL_Timer_reset(regs);
            DL_Timer_disablePower(regs);
            NATIVE_CORE_API->resource_release(timer_resource(timer),
                PIN_OWN_CAP);
            state->mode[timer] = 0u;
        }
    }
}

const native_module_header_t g_native_module_header
    __attribute__((section(".module_header"), used, aligned(4))) = {
        NATIVE_MODULE_MAGIC, NATIVE_MODULE_FORMAT, NATIVE_CORE_ABI_VERSION,
        0, 0, sizeof(native_module_header_t), tmr_init, tmr_deinit, "tmr",
    };
