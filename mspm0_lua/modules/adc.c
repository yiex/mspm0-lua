#include "native_module.h"

#include "board_pins.h"
#include "board_resource.h"
#include "g3507_pin_routes.h"
#include <ti/driverlib/driverlib.h>

static int find_adc_route(lua_State *L, const char *name,
    native_pin_t *pin, unsigned *instance, unsigned *channel)
{
    unsigned i;
    if (NATIVE_CORE_API->pin_resolve(name, pin) != 0) {
        return NATIVE_CORE_API->raise_error(L, "adc:pin");
    }
    for (i = 0; i < sizeof(g3507_adc_routes) /
            sizeof(g3507_adc_routes[0]); i++) {
        const g3507_route_t *route = &g3507_adc_routes[i];
        if (route->iomux == pin->iomux) {
            *instance = route->instance;
            *channel = route->role;
            return 0;
        }
    }
    return NATIVE_CORE_API->raise_error(L, "adc:pin");
}

static ADC12_Regs *adc_regs(unsigned instance)
{
    return instance ? ADC1 : ADC0;
}

static uint8_t adc_resource(unsigned instance)
{
    return instance ? BOARD_RES_ADC1 : BOARD_RES_ADC0;
}

static uint32_t average_num(unsigned count)
{
    switch (count) {
        case 2: return DL_ADC12_HW_AVG_NUM_ACC_2;
        case 4: return DL_ADC12_HW_AVG_NUM_ACC_4;
        case 8: return DL_ADC12_HW_AVG_NUM_ACC_8;
        case 16: return DL_ADC12_HW_AVG_NUM_ACC_16;
        case 32: return DL_ADC12_HW_AVG_NUM_ACC_32;
        case 64: return DL_ADC12_HW_AVG_NUM_ACC_64;
        case 128: return DL_ADC12_HW_AVG_NUM_ACC_128;
        default: return DL_ADC12_HW_AVG_NUM_ACC_DISABLED;
    }
}

static uint32_t average_den(unsigned count)
{
    switch (count) {
        case 2: return DL_ADC12_HW_AVG_DEN_DIV_BY_2;
        case 4: return DL_ADC12_HW_AVG_DEN_DIV_BY_4;
        case 8: return DL_ADC12_HW_AVG_DEN_DIV_BY_8;
        case 16: return DL_ADC12_HW_AVG_DEN_DIV_BY_16;
        case 32: return DL_ADC12_HW_AVG_DEN_DIV_BY_32;
        case 64: return DL_ADC12_HW_AVG_DEN_DIV_BY_64;
        case 128: return DL_ADC12_HW_AVG_DEN_DIV_BY_128;
        default: return DL_ADC12_HW_AVG_DEN_DIV_BY_1;
    }
}

static DL_ADC12_SAMP_CONV_RES adc_resolution(unsigned bits)
{
    if (bits == 8u) return DL_ADC12_SAMP_CONV_RES_8_BIT;
    if (bits == 10u) return DL_ADC12_SAMP_CONV_RES_10_BIT;
    return DL_ADC12_SAMP_CONV_RES_12_BIT;
}

static int adc_read_raw(lua_State *L, const char *name, uint32_t sample_cycles,
    unsigned averages, unsigned bits, int32_t *result)
{
    native_pin_t pin;
    unsigned instance;
    unsigned channel;
    uint32_t started;
    uint32_t averaging;
    ADC12_Regs *adc;
    DL_ADC12_ClockConfig clock = {
        .clockSel = DL_ADC12_CLOCK_ULPCLK,
        .divideRatio = DL_ADC12_CLOCK_DIVIDE_8,
        .freqRange = DL_ADC12_CLOCK_FREQ_RANGE_24_TO_32,
    };
    if (find_adc_route(L, name, &pin, &instance, &channel) != 0) return -1;
    if (sample_cycles < 4u) sample_cycles = 4u;
    if (sample_cycles > 1023u) sample_cycles = 1023u;
    if (averages != 1u && averages != 2u && averages != 4u &&
            averages != 8u && averages != 16u && averages != 32u &&
            averages != 64u && averages != 128u) {
        return NATIVE_CORE_API->raise_error(L, "adc:average");
    }
    if (bits != 8u && bits != 10u && bits != 12u) {
        return NATIVE_CORE_API->raise_error(L, "adc:bits");
    }
    if (NATIVE_CORE_API->resource_claim(adc_resource(instance),
            PIN_OWN_ADC) != 0) {
        return NATIVE_CORE_API->raise_error(L, "adc:busy");
    }
    if (NATIVE_CORE_API->pin_claim(name, PIN_OWN_ADC) != 0) {
        NATIVE_CORE_API->resource_release(adc_resource(instance),
            PIN_OWN_ADC);
        return NATIVE_CORE_API->raise_error(L, "adc:busy");
    }
    if (NATIVE_CORE_API->pin_af(name, 0, 0) != 0) {
        NATIVE_CORE_API->pin_release(name, PIN_OWN_ADC);
        NATIVE_CORE_API->resource_release(adc_resource(instance),
            PIN_OWN_ADC);
        return NATIVE_CORE_API->raise_error(L, "adc:pin");
    }
    adc = adc_regs(instance);
    DL_ADC12_reset(adc);
    DL_ADC12_enablePower(adc);
    delay_cycles(16);
    DL_ADC12_setClockConfig(adc, &clock);
    DL_ADC12_initSingleSample(adc, DL_ADC12_REPEAT_MODE_DISABLED,
        DL_ADC12_SAMPLING_SOURCE_AUTO, DL_ADC12_TRIG_SRC_SOFTWARE,
        adc_resolution(bits), DL_ADC12_SAMP_CONV_DATA_FORMAT_UNSIGNED);
    DL_ADC12_setPowerDownMode(adc, DL_ADC12_POWER_DOWN_MODE_MANUAL);
    DL_ADC12_setSampleTime0(adc, (uint16_t)sample_cycles);
    averaging = averages > 1u ? DL_ADC12_AVERAGING_MODE_ENABLED
                              : DL_ADC12_AVERAGING_MODE_DISABLED;
    if (averages > 1u) {
        DL_ADC12_configHwAverage(adc, average_num(averages),
            average_den(averages));
    }
    DL_ADC12_configConversionMem(adc, DL_ADC12_MEM_IDX_0, channel,
        DL_ADC12_REFERENCE_VOLTAGE_VDDA,
        DL_ADC12_SAMPLE_TIMER_SOURCE_SCOMP0, averaging,
        DL_ADC12_BURN_OUT_SOURCE_DISABLED, DL_ADC12_TRIGGER_MODE_AUTO_NEXT,
        DL_ADC12_WINDOWS_COMP_MODE_DISABLED);
    DL_ADC12_clearInterruptStatus(adc,
        DL_ADC12_INTERRUPT_MEM0_RESULT_LOADED);
    DL_ADC12_enableConversions(adc);
    DL_ADC12_startConversion(adc);
    started = NATIVE_CORE_API->millis();
    while (DL_ADC12_getRawInterruptStatus(adc,
            DL_ADC12_INTERRUPT_MEM0_RESULT_LOADED) == 0u) {
        if ((uint32_t)(NATIVE_CORE_API->millis() - started) >= 20u) {
            DL_ADC12_reset(adc);
            DL_ADC12_disablePower(adc);
            NATIVE_CORE_API->pin_release(name, PIN_OWN_ADC);
            NATIVE_CORE_API->resource_release(adc_resource(instance),
                PIN_OWN_ADC);
            return NATIVE_CORE_API->raise_error(L, "adc:timeout");
        }
    }
    *result = (int32_t)DL_ADC12_getMemResult(adc, DL_ADC12_MEM_IDX_0);
    DL_ADC12_disableConversions(adc);
    DL_ADC12_reset(adc);
    DL_ADC12_disablePower(adc);
    NATIVE_CORE_API->pin_release(name, PIN_OWN_ADC);
    NATIVE_CORE_API->resource_release(adc_resource(instance), PIN_OWN_ADC);
    return 0;
}

static int l_adc_channel(lua_State *L)
{
    native_pin_t pin;
    unsigned instance;
    unsigned channel;
    if (find_adc_route(L, NATIVE_CORE_API->check_string(L, 1), &pin,
            &instance, &channel) != 0) return -1;
    NATIVE_CORE_API->push_integer(L, (int32_t)channel);
    return 1;
}

static int l_adc_instance(lua_State *L)
{
    native_pin_t pin;
    unsigned instance;
    unsigned channel;
    if (find_adc_route(L, NATIVE_CORE_API->check_string(L, 1), &pin,
            &instance, &channel) != 0) return -1;
    NATIVE_CORE_API->push_integer(L, (int32_t)instance);
    return 1;
}

static int l_adc_read(lua_State *L)
{
    const char *pin = NATIVE_CORE_API->check_string(L, 1);
    uint32_t sample_cycles = (uint32_t)NATIVE_CORE_API->opt_integer(L, 2, 500);
    unsigned averages = (unsigned)NATIVE_CORE_API->opt_integer(L, 3, 1);
    unsigned bits = (unsigned)NATIVE_CORE_API->opt_integer(L, 4, 12);
    int32_t result;
    if (adc_read_raw(L, pin, sample_cycles, averages, bits, &result) != 0) {
        return -1;
    }
    NATIVE_CORE_API->push_integer(L, result);
    return 1;
}

static int l_adc_read_mv(lua_State *L)
{
    const char *pin = NATIVE_CORE_API->check_string(L, 1);
    int32_t vdda_mv = NATIVE_CORE_API->check_integer(L, 2);
    uint32_t sample_cycles = (uint32_t)NATIVE_CORE_API->opt_integer(L, 3, 500);
    unsigned averages = (unsigned)NATIVE_CORE_API->opt_integer(L, 4, 1);
    unsigned bits = (unsigned)NATIVE_CORE_API->opt_integer(L, 5, 12);
    int32_t result;
    uint32_t full_scale;
    if (vdda_mv <= 0 || vdda_mv > 100000) {
        return NATIVE_CORE_API->raise_error(L, "adc:vdda");
    }
    if (adc_read_raw(L, pin, sample_cycles, averages, bits, &result) != 0) {
        return -1;
    }
    full_scale = (1u << bits) - 1u;
    NATIVE_CORE_API->push_integer(L, (int32_t)NATIVE_CORE_API->udiv32(
        (uint32_t)result * (uint32_t)vdda_mv + full_scale / 2u,
        full_scale));
    return 1;
}

static int l_adc_release(lua_State *L)
{
    native_pin_t pin;
    unsigned instance;
    unsigned channel;
    const char *name = NATIVE_CORE_API->check_string(L, 1);
    if (find_adc_route(L, name, &pin, &instance, &channel) != 0) return -1;
    DL_ADC12_reset(adc_regs(instance));
    DL_ADC12_disablePower(adc_regs(instance));
    NATIVE_CORE_API->pin_release(name, PIN_OWN_ADC);
    NATIVE_CORE_API->resource_release(adc_resource(instance), PIN_OWN_ADC);
    return 0;
}

static const native_lua_reg_t k_adc_functions[] = {
    {"channel", l_adc_channel}, {"instance", l_adc_instance},
    {"read", l_adc_read}, {"read_mv", l_adc_read_mv},
    {"release", l_adc_release}, {0, 0},
};

static int adc_init(lua_State *L, const native_core_api_t *api)
{
    if (!api || api->magic != NATIVE_CORE_API_MAGIC ||
            api->abi_version != NATIVE_CORE_ABI_VERSION ||
            api->struct_size < sizeof(native_core_api_t)) return -1;
    return api->register_lua_module(L, "adc", k_adc_functions);
}

const native_module_header_t g_native_module_header
    __attribute__((section(".module_header"), used, aligned(4))) = {
        NATIVE_MODULE_MAGIC, NATIVE_MODULE_FORMAT, NATIVE_CORE_ABI_VERSION,
        0, 0, sizeof(native_module_header_t), adc_init, 0, "adc",
    };
