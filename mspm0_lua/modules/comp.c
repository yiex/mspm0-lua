#include "native_module.h"

#include "board_pins.h"
#include "board_resource.h"
#include <ti/driverlib/driverlib.h>

typedef struct {
    const char *name;
    uint8_t instance;
    uint8_t positive;
    uint8_t channel;
} comp_route_t;

typedef struct {
    uint8_t active;
    uint8_t pos_route;
    uint8_t neg_route;
    uint8_t reserved;
} comp_unit_state_t;

typedef struct {
    comp_unit_state_t unit[3];
} comp_state_t;

static const comp_route_t k_routes[] = {
    {"PA26", 0, 1, 0}, {"PA18", 0, 1, 1},
    {"PA14", 0, 1, 2}, {"PA15", 0, 1, 3},
    {"PA27", 0, 0, 0}, {"PA17", 0, 0, 1}, {"PA13", 0, 0, 2},
    {"PB24", 1, 1, 1}, {"PB18", 1, 1, 2}, {"PA15", 1, 1, 3},
    {"PA23", 1, 0, 1}, {"PB17", 1, 0, 2},
    {"PB19", 2, 1, 1}, {"PA21", 2, 0, 1},
};

extern const native_module_header_t g_native_module_header;

static unsigned comp_slot(void)
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

static comp_state_t *comp_state(void)
{
    return (comp_state_t *)NATIVE_CORE_API->module_state(
        comp_slot(), sizeof(comp_state_t));
}

static int same_string(const char *a, const char *b)
{
    while (*a && *a == *b) { a++; b++; }
    return *a == *b;
}

static int find_route(unsigned instance, int positive, const char *name)
{
    unsigned i;
    for (i = 0; i < sizeof(k_routes) / sizeof(k_routes[0]); i++) {
        if (k_routes[i].instance == instance &&
                k_routes[i].positive == (unsigned)positive &&
                same_string(k_routes[i].name, name)) return (int)i;
    }
    return -1;
}

static COMP_Regs *comp_regs(unsigned id)
{
    return id == 0u ? COMP0 : id == 1u ? COMP1 : COMP2;
}

static uint8_t comp_owner(unsigned id)
{
    return (uint8_t)(PIN_OWN_COMP0 + id);
}

static uint8_t comp_resource(unsigned id)
{
    return (uint8_t)(BOARD_RES_COMP0 + id);
}

static void close_unit(comp_state_t *state, unsigned id)
{
    comp_unit_state_t *unit;
    uint8_t owner;
    if (!state || id > 2u || !state->unit[id].active) return;
    unit = &state->unit[id];
    owner = comp_owner(id);
    DL_COMP_disable(comp_regs(id));
    DL_COMP_reset(comp_regs(id));
    DL_COMP_disablePower(comp_regs(id));
    if (unit->pos_route < sizeof(k_routes) / sizeof(k_routes[0])) {
        NATIVE_CORE_API->pin_release(k_routes[unit->pos_route].name, owner);
    }
    if (unit->neg_route < sizeof(k_routes) / sizeof(k_routes[0])) {
        NATIVE_CORE_API->pin_release(k_routes[unit->neg_route].name, owner);
    }
    NATIVE_CORE_API->resource_release(comp_resource(id), owner);
    unit->active = 0;
}

static int l_comp_open(lua_State *L)
{
    static const DL_COMP_IPSEL_CHANNEL pos_channels[8] = {
        DL_COMP_IPSEL_CHANNEL_0, DL_COMP_IPSEL_CHANNEL_1,
        DL_COMP_IPSEL_CHANNEL_2, DL_COMP_IPSEL_CHANNEL_3,
        DL_COMP_IPSEL_CHANNEL_4, DL_COMP_IPSEL_CHANNEL_5,
        DL_COMP_IPSEL_CHANNEL_6, DL_COMP_IPSEL_CHANNEL_7,
    };
    static const DL_COMP_IMSEL_CHANNEL neg_channels[8] = {
        DL_COMP_IMSEL_CHANNEL_0, DL_COMP_IMSEL_CHANNEL_1,
        DL_COMP_IMSEL_CHANNEL_2, DL_COMP_IMSEL_CHANNEL_3,
        DL_COMP_IMSEL_CHANNEL_4, DL_COMP_IMSEL_CHANNEL_5,
        DL_COMP_IMSEL_CHANNEL_6, DL_COMP_IMSEL_CHANNEL_7,
    };
    static const DL_COMP_HYSTERESIS hysteresis[4] = {
        DL_COMP_HYSTERESIS_NONE, DL_COMP_HYSTERESIS_10,
        DL_COMP_HYSTERESIS_20, DL_COMP_HYSTERESIS_30,
    };
    comp_state_t *state = comp_state();
    unsigned id = (unsigned)NATIVE_CORE_API->check_integer(L, 1);
    const char *pos_name = NATIVE_CORE_API->check_string(L, 2);
    const char *neg_name = NATIVE_CORE_API->check_string(L, 3);
    int fast = NATIVE_CORE_API->opt_integer(L, 4, 1) != 0;
    unsigned hyst = (unsigned)NATIVE_CORE_API->opt_integer(L, 5, 0);
    int invert = NATIVE_CORE_API->opt_integer(L, 6, 0) != 0;
    int pos;
    int neg;
    uint8_t owner;
    COMP_Regs *regs;
    DL_COMP_Config config;
    if (!state || id > 2u || hyst > 3u) {
        return NATIVE_CORE_API->raise_error(L, "comp:config");
    }
    pos = find_route(id, 1, pos_name);
    neg = find_route(id, 0, neg_name);
    if (pos < 0 || neg < 0 || same_string(pos_name, neg_name)) {
        return NATIVE_CORE_API->raise_error(L, "comp:pin");
    }
    close_unit(state, id);
    owner = comp_owner(id);
    if (NATIVE_CORE_API->resource_claim(comp_resource(id), owner) != 0 ||
            NATIVE_CORE_API->pin_claim(pos_name, owner) != 0 ||
            NATIVE_CORE_API->pin_claim(neg_name, owner) != 0 ||
            NATIVE_CORE_API->pin_af(pos_name, 0, 0) != 0 ||
            NATIVE_CORE_API->pin_af(neg_name, 0, 0) != 0) {
        NATIVE_CORE_API->pin_release(pos_name, owner);
        NATIVE_CORE_API->pin_release(neg_name, owner);
        NATIVE_CORE_API->resource_release(comp_resource(id), owner);
        return NATIVE_CORE_API->raise_error(L, "comp:busy");
    }
    config.posChannel = pos_channels[k_routes[pos].channel];
    config.negChannel = neg_channels[k_routes[neg].channel];
    config.channelEnable = DL_COMP_ENABLE_CHANNEL_POS_NEG;
    config.mode = fast ? DL_COMP_MODE_FAST : DL_COMP_MODE_ULP;
    config.hysteresis = hysteresis[hyst];
    config.polarity = invert ? DL_COMP_POLARITY_INV
                             : DL_COMP_POLARITY_NON_INV;
    regs = comp_regs(id);
    DL_COMP_reset(regs);
    DL_COMP_enablePower(regs);
    delay_cycles(16);
    DL_COMP_init(regs, &config);
    DL_COMP_enable(regs);
    state->unit[id].active = 1;
    state->unit[id].pos_route = (uint8_t)pos;
    state->unit[id].neg_route = (uint8_t)neg;
    return 0;
}

static int l_comp_read(lua_State *L)
{
    comp_state_t *state = comp_state();
    unsigned id = (unsigned)NATIVE_CORE_API->check_integer(L, 1);
    if (!state || id > 2u || !state->unit[id].active) {
        return NATIVE_CORE_API->raise_error(L, "comp:closed");
    }
    NATIVE_CORE_API->push_boolean(L,
        DL_COMP_getComparatorOutput(comp_regs(id)) == DL_COMP_OUTPUT_HIGH);
    return 1;
}

static int l_comp_close(lua_State *L)
{
    comp_state_t *state = comp_state();
    unsigned id = (unsigned)NATIVE_CORE_API->check_integer(L, 1);
    if (!state || id > 2u) return NATIVE_CORE_API->raise_error(L, "comp:id");
    close_unit(state, id);
    return 0;
}

static const native_lua_reg_t k_comp_functions[] = {
    {"open", l_comp_open}, {"read", l_comp_read},
    {"close", l_comp_close}, {0, 0},
};

static int comp_init(lua_State *L, const native_core_api_t *api)
{
    comp_state_t *state;
    unsigned i;
    if (!api || api->magic != NATIVE_CORE_API_MAGIC ||
            api->abi_version != NATIVE_CORE_ABI_VERSION ||
            api->struct_size < sizeof(native_core_api_t)) return -1;
    state = (comp_state_t *)api->module_state(comp_slot(), sizeof(*state));
    if (!state) return -1;
    for (i = 0; i < 3u; i++) state->unit[i].active = 0;
    return api->register_lua_module(L, "comp", k_comp_functions);
}

static void comp_deinit(void)
{
    comp_state_t *state = comp_state();
    unsigned id;
    if (!state) return;
    for (id = 0; id < 3u; id++) close_unit(state, id);
}

const native_module_header_t g_native_module_header
    __attribute__((section(".module_header"), used, aligned(4))) = {
        NATIVE_MODULE_MAGIC, NATIVE_MODULE_FORMAT, NATIVE_CORE_ABI_VERSION,
        0, 0, sizeof(native_module_header_t), comp_init, comp_deinit, "comp",
    };
