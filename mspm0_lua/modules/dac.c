#include "native_module.h"

#include "board_pins.h"
#include "board_resource.h"
#include <ti/driverlib/driverlib.h>

typedef struct {
    uint8_t active;
    uint8_t bits;
    uint8_t pin_enabled;
    uint8_t reserved;
} dac_state_t;

extern const native_module_header_t g_native_module_header;

static unsigned dac_slot(void)
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

static dac_state_t *dac_state(void)
{
    return (dac_state_t *)NATIVE_CORE_API->module_state(
        dac_slot(), sizeof(dac_state_t));
}

static void dac_close(dac_state_t *state)
{
    if (!state || !state->active) return;
    DL_DAC12_disable(DAC0);
    DL_DAC12_reset(DAC0);
    DL_DAC12_disablePower(DAC0);
    if (state->pin_enabled) {
        NATIVE_CORE_API->pin_release("PA15", PIN_OWN_DAC);
    }
    NATIVE_CORE_API->resource_release(BOARD_RES_DAC0, PIN_OWN_DAC);
    state->active = 0;
    state->pin_enabled = 0;
}

static int l_dac_open(lua_State *L)
{
    static const DL_DAC12_VREF_SOURCE refs[4] = {
        DL_DAC12_VREF_SOURCE_VDDA_VSSA,
        DL_DAC12_VREF_SOURCE_VDDA_VEREFN,
        DL_DAC12_VREF_SOURCE_VEREFP_VSSA,
        DL_DAC12_VREF_SOURCE_VEREFP_VEREFN,
    };
    dac_state_t *state = dac_state();
    unsigned bits = (unsigned)NATIVE_CORE_API->opt_integer(L, 1, 12);
    unsigned reference = (unsigned)NATIVE_CORE_API->opt_integer(L, 2, 0);
    int pin_enabled = NATIVE_CORE_API->opt_integer(L, 3, 1) != 0;
    DL_DAC12_Config config;
    if (!state || (bits != 8u && bits != 12u) || reference > 3u) {
        return NATIVE_CORE_API->raise_error(L, "dac:config");
    }
    dac_close(state);
    if (NATIVE_CORE_API->resource_claim(BOARD_RES_DAC0, PIN_OWN_DAC) != 0) {
        return NATIVE_CORE_API->raise_error(L, "dac:busy");
    }
    if (pin_enabled && (NATIVE_CORE_API->pin_claim("PA15", PIN_OWN_DAC) != 0 ||
            NATIVE_CORE_API->pin_af("PA15", 0, 0) != 0)) {
        NATIVE_CORE_API->pin_release("PA15", PIN_OWN_DAC);
        NATIVE_CORE_API->resource_release(BOARD_RES_DAC0, PIN_OWN_DAC);
        return NATIVE_CORE_API->raise_error(L, "dac:pin");
    }
    config.outputEnable = pin_enabled ? DL_DAC12_OUTPUT_ENABLED
                                      : DL_DAC12_OUTPUT_DISABLED;
    config.resolution = bits == 8u ? DL_DAC12_RESOLUTION_8BIT
                                   : DL_DAC12_RESOLUTION_12BIT;
    config.representation = DL_DAC12_REPRESENTATION_BINARY;
    config.voltageReferenceSource = refs[reference];
    config.amplifierSetting = DL_DAC12_AMP_ON;
    config.fifoEnable = DL_DAC12_FIFO_DISABLED;
    config.fifoTriggerSource = DL_DAC12_FIFO_TRIGGER_SAMPLETIMER;
    config.dmaTriggerEnable = DL_DAC12_DMA_TRIGGER_DISABLED;
    config.dmaTriggerThreshold = DL_DAC12_FIFO_THRESHOLD_ONE_QTR_EMPTY;
    config.sampleTimeGeneratorEnable = DL_DAC12_SAMPLETIMER_DISABLE;
    config.sampleRate = DL_DAC12_SAMPLES_PER_SECOND_1K;
    DL_DAC12_reset(DAC0);
    DL_DAC12_enablePower(DAC0);
    delay_cycles(16);
    DL_DAC12_init(DAC0, &config);
    DL_DAC12_enable(DAC0);
    state->active = 1;
    state->bits = (uint8_t)bits;
    state->pin_enabled = (uint8_t)pin_enabled;
    return 0;
}

static int l_dac_write(lua_State *L)
{
    dac_state_t *state = dac_state();
    int32_t value = NATIVE_CORE_API->check_integer(L, 1);
    uint32_t maximum;
    if (!state || !state->active || !DL_DAC12_isEnabled(DAC0)) {
        return NATIVE_CORE_API->raise_error(L, "dac:closed");
    }
    maximum = state->bits == 8u ? 255u : 4095u;
    if (value < 0 || (uint32_t)value > maximum) {
        return NATIVE_CORE_API->raise_error(L, "dac:range");
    }
    if (state->bits == 8u) DL_DAC12_output8(DAC0, (uint8_t)value);
    else DL_DAC12_output12(DAC0, (uint32_t)value);
    return 0;
}

static int l_dac_write_mv(lua_State *L)
{
    dac_state_t *state = dac_state();
    int32_t millivolts = NATIVE_CORE_API->check_integer(L, 1);
    int32_t reference_mv = NATIVE_CORE_API->check_integer(L, 2);
    uint32_t maximum;
    uint32_t value;
    if (!state || !state->active) {
        return NATIVE_CORE_API->raise_error(L, "dac:closed");
    }
    if (reference_mv <= 0 || reference_mv > 100000 || millivolts < 0 ||
            millivolts > reference_mv) {
        return NATIVE_CORE_API->raise_error(L, "dac:range");
    }
    maximum = state->bits == 8u ? 255u : 4095u;
    value = NATIVE_CORE_API->udiv32(
        (uint32_t)millivolts * maximum + (uint32_t)reference_mv / 2u,
        (uint32_t)reference_mv);
    if (state->bits == 8u) DL_DAC12_output8(DAC0, (uint8_t)value);
    else DL_DAC12_output12(DAC0, value);
    NATIVE_CORE_API->push_integer(L, (int32_t)value);
    return 1;
}

static int l_dac_close(lua_State *L)
{
    (void)L;
    dac_close(dac_state());
    return 0;
}

static const native_lua_reg_t k_dac_functions[] = {
    {"open", l_dac_open}, {"write", l_dac_write},
    {"write_mv", l_dac_write_mv}, {"close", l_dac_close}, {0, 0},
};

static int dac_init(lua_State *L, const native_core_api_t *api)
{
    dac_state_t *state;
    if (!api || api->magic != NATIVE_CORE_API_MAGIC ||
            api->abi_version != NATIVE_CORE_ABI_VERSION ||
            api->struct_size < sizeof(native_core_api_t)) return -1;
    state = (dac_state_t *)api->module_state(dac_slot(), sizeof(*state));
    if (!state) return -1;
    state->active = 0;
    state->pin_enabled = 0;
    return api->register_lua_module(L, "dac", k_dac_functions);
}

static void dac_deinit(void)
{
    dac_close(dac_state());
}

const native_module_header_t g_native_module_header
    __attribute__((section(".module_header"), used, aligned(4))) = {
        NATIVE_MODULE_MAGIC, NATIVE_MODULE_FORMAT, NATIVE_CORE_ABI_VERSION,
        0, 0, sizeof(native_module_header_t), dac_init, dac_deinit, "dac",
    };
