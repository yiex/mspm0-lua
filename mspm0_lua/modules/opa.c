#include "native_module.h"

#include "board_pins.h"
#include "board_resource.h"
#include <ti/driverlib/driverlib.h>

typedef struct {
    uint8_t active;
    uint8_t psel;
    uint8_t nsel;
    uint8_t output;
} opa_unit_state_t;

typedef struct {
    opa_unit_state_t unit[2];
} opa_state_t;

extern const native_module_header_t g_native_module_header;

static unsigned opa_slot(void)
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

static opa_state_t *opa_state(void)
{
    return (opa_state_t *)NATIVE_CORE_API->module_state(
        opa_slot(), sizeof(opa_state_t));
}

static OA_Regs *opa_regs(unsigned id)
{
    return id ? OPA1 : OPA0;
}

static uint8_t opa_owner(unsigned id)
{
    return (uint8_t)(PIN_OWN_OPA0 + id);
}

static uint8_t opa_resource(unsigned id)
{
    return (uint8_t)(BOARD_RES_OPA0 + id);
}

static const char *opa_pin(unsigned id, unsigned kind, unsigned selector)
{
    static const char *const pins[2][3][2] = {
        {{"PA26", "PA25"}, {"PA27", "PA24"}, {"PA22", "PA22"}},
        {{"PB19", "PA18"}, {"PB20", "PA17"}, {"PA16", "PA16"}},
    };
    if (id > 1u || kind > 2u || selector < 1u || selector > 2u) return 0;
    return pins[id][kind][selector - 1u];
}

static void release_pin(const char *name, uint8_t owner)
{
    if (name) NATIVE_CORE_API->pin_release(name, owner);
}

static void close_unit(opa_state_t *state, unsigned id)
{
    opa_unit_state_t *unit;
    uint8_t owner;
    if (!state || id > 1u || !state->unit[id].active) return;
    unit = &state->unit[id];
    owner = opa_owner(id);
    DL_OPA_disable(opa_regs(id));
    DL_OPA_reset(opa_regs(id));
    DL_OPA_disablePower(opa_regs(id));
    release_pin(opa_pin(id, 0, unit->psel), owner);
    release_pin(opa_pin(id, 1, unit->nsel), owner);
    if (unit->output) release_pin(opa_pin(id, 2, 1), owner);
    NATIVE_CORE_API->resource_release(opa_resource(id), owner);
    unit->active = 0;
}

static int claim_analog(const char *name, uint8_t owner)
{
    if (!name) return 0;
    if (NATIVE_CORE_API->pin_claim(name, owner) != 0 ||
            NATIVE_CORE_API->pin_af(name, 0, 0) != 0) return -1;
    return 0;
}

static int l_opa_open(lua_State *L)
{
    static const DL_OPA_PSEL psel_values[9] = {
        DL_OPA_PSEL_OPEN, DL_OPA_PSEL_IN0_POS, DL_OPA_PSEL_IN1_POS,
        DL_OPA_PSEL_DAC_OUT, DL_OPA_PSEL_DAC8_OUT, DL_OPA_PSEL_VREF,
        DL_OPA_PSEL_RTOP, DL_OPA_PSEL_GPAMP_OUT, DL_OPA_PSEL_GND,
    };
    static const DL_OPA_NSEL nsel_values[7] = {
        DL_OPA_NSEL_OPEN, DL_OPA_NSEL_IN0_NEG, DL_OPA_NSEL_IN1_NEG,
        DL_OPA_NSEL_RBOT, DL_OPA_NSEL_RTAP, DL_OPA_NSEL_RTOP,
        DL_OPA_NSEL_SPARE,
    };
    static const DL_OPA_MSEL msel_values[5] = {
        DL_OPA_MSEL_OPEN, DL_OPA_MSEL_IN1_NEG, DL_OPA_MSEL_GND,
        DL_OPA_MSEL_DAC_OUT, DL_OPA_MSEL_RTOP,
    };
    static const DL_OPA_GAIN gains[6] = {
        DL_OPA_GAIN_N0_P1, DL_OPA_GAIN_N1_P2, DL_OPA_GAIN_N3_P4,
        DL_OPA_GAIN_N7_P8, DL_OPA_GAIN_N15_P16, DL_OPA_GAIN_N31_P32,
    };
    opa_state_t *state = opa_state();
    unsigned id = (unsigned)NATIVE_CORE_API->check_integer(L, 1);
    unsigned psel = (unsigned)NATIVE_CORE_API->opt_integer(L, 2, 1);
    unsigned nsel = (unsigned)NATIVE_CORE_API->opt_integer(L, 3, 4);
    unsigned msel = (unsigned)NATIVE_CORE_API->opt_integer(L, 4, 2);
    unsigned gain_index = (unsigned)NATIVE_CORE_API->opt_integer(L, 5, 0);
    int output = NATIVE_CORE_API->opt_integer(L, 6, 1) != 0;
    unsigned chop = (unsigned)NATIVE_CORE_API->opt_integer(L, 7, 0);
    int high_bandwidth = NATIVE_CORE_API->opt_integer(L, 8, 1) != 0;
    int rail_to_rail = NATIVE_CORE_API->opt_integer(L, 9, 0) != 0;
    const char *pos_pin;
    const char *neg_pin;
    const char *out_pin;
    uint8_t owner;
    uint32_t started;
    OA_Regs *regs;
    DL_OPA_Config config;
    if (!state || id > 1u || psel > 8u || nsel > 6u || msel > 4u ||
            gain_index > 5u || chop > 2u) {
        return NATIVE_CORE_API->raise_error(L, "opa:config");
    }
    pos_pin = opa_pin(id, 0, psel);
    neg_pin = opa_pin(id, 1, nsel);
    out_pin = output ? opa_pin(id, 2, 1) : 0;
    close_unit(state, id);
    owner = opa_owner(id);
    if (NATIVE_CORE_API->resource_claim(opa_resource(id), owner) != 0 ||
            claim_analog(pos_pin, owner) != 0 ||
            claim_analog(neg_pin, owner) != 0 ||
            claim_analog(out_pin, owner) != 0) {
        release_pin(pos_pin, owner);
        release_pin(neg_pin, owner);
        release_pin(out_pin, owner);
        NATIVE_CORE_API->resource_release(opa_resource(id), owner);
        return NATIVE_CORE_API->raise_error(L, "opa:busy");
    }
    config.choppingMode = (DL_OPA_CHOPPING_MODE)chop;
    config.outputPinState = output ? DL_OPA_OUTPUT_PIN_ENABLED
                                   : DL_OPA_OUTPUT_PIN_DISABLED;
    config.pselChannel = psel_values[psel];
    config.nselChannel = nsel_values[nsel];
    config.mselChannel = msel_values[msel];
    config.gain = gains[gain_index];
    regs = opa_regs(id);
    DL_OPA_reset(regs);
    DL_OPA_enablePower(regs);
    delay_cycles(16);
    DL_OPA_init(regs, &config);
    DL_OPA_enable(regs);
    started = NATIVE_CORE_API->millis();
    while (!DL_OPA_isReady(regs)) {
        if ((uint32_t)(NATIVE_CORE_API->millis() - started) >= 20u) {
            DL_OPA_disable(regs);
            DL_OPA_reset(regs);
            DL_OPA_disablePower(regs);
            release_pin(pos_pin, owner);
            release_pin(neg_pin, owner);
            release_pin(out_pin, owner);
            NATIVE_CORE_API->resource_release(opa_resource(id), owner);
            return NATIVE_CORE_API->raise_error(L, "opa:timeout");
        }
    }
    DL_OPA_setGainBandwidth(regs,
        high_bandwidth ? DL_OPA_GBW_HIGH : DL_OPA_GBW_LOW);
    if (rail_to_rail) DL_OPA_enableRailToRailInput(regs);
    else DL_OPA_disableRailToRailInput(regs);
    state->unit[id].active = 1;
    state->unit[id].psel = (uint8_t)psel;
    state->unit[id].nsel = (uint8_t)nsel;
    state->unit[id].output = (uint8_t)output;
    return 0;
}

static int l_opa_ready(lua_State *L)
{
    opa_state_t *state = opa_state();
    unsigned id = (unsigned)NATIVE_CORE_API->check_integer(L, 1);
    if (!state || id > 1u || !state->unit[id].active) {
        return NATIVE_CORE_API->raise_error(L, "opa:closed");
    }
    NATIVE_CORE_API->push_boolean(L, DL_OPA_isReady(opa_regs(id)));
    return 1;
}

static int l_opa_close(lua_State *L)
{
    opa_state_t *state = opa_state();
    unsigned id = (unsigned)NATIVE_CORE_API->check_integer(L, 1);
    if (!state || id > 1u) return NATIVE_CORE_API->raise_error(L, "opa:id");
    close_unit(state, id);
    return 0;
}

static const native_lua_reg_t k_opa_functions[] = {
    {"open", l_opa_open}, {"ready", l_opa_ready},
    {"close", l_opa_close}, {0, 0},
};

static int opa_init(lua_State *L, const native_core_api_t *api)
{
    opa_state_t *state;
    if (!api || api->magic != NATIVE_CORE_API_MAGIC ||
            api->abi_version != NATIVE_CORE_ABI_VERSION ||
            api->struct_size < sizeof(native_core_api_t)) return -1;
    state = (opa_state_t *)api->module_state(opa_slot(), sizeof(*state));
    if (!state) return -1;
    state->unit[0].active = 0;
    state->unit[1].active = 0;
    return api->register_lua_module(L, "opa", k_opa_functions);
}

static void opa_deinit(void)
{
    opa_state_t *state = opa_state();
    unsigned id;
    if (!state) return;
    for (id = 0; id < 2u; id++) close_unit(state, id);
}

const native_module_header_t g_native_module_header
    __attribute__((section(".module_header"), used, aligned(4))) = {
        NATIVE_MODULE_MAGIC, NATIVE_MODULE_FORMAT, NATIVE_CORE_ABI_VERSION,
        0, 0, sizeof(native_module_header_t), opa_init, opa_deinit, "opa",
    };
