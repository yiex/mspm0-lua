#include "native_module.h"

#include "board_pins.h"
#include "board_reg.h"
#include "board_resource.h"
#include "g3507_pin_routes.h"
#include <ti/driverlib/driverlib.h>

#define SPI_MAX_TRANSFER 512u
#define SPI_WAIT_SPINS 200000u

typedef struct {
    SPI_Regs *regs;
    const char *sck_name;
    const char *pico_name;
    const char *poci_name;
    const char *cs_name;
    native_pin_t sck;
    native_pin_t pico;
    native_pin_t poci;
    native_pin_t cs;
    uint8_t owner;
    uint8_t instance;
    uint8_t active;
    uint8_t spi1_lent;
    uint8_t claimed;
} spi_bus_t;

static int route_pf(unsigned instance, const native_pin_t *pin,
    unsigned role, unsigned *pf)
{
    unsigned i;
    for (i = 0; i < sizeof(g3507_spi_routes) /
            sizeof(g3507_spi_routes[0]); i++) {
        const g3507_route_t *route = &g3507_spi_routes[i];
        if (route->instance == instance && route->iomux == pin->iomux &&
                route->role == role) {
            *pf = route->pf;
            return 0;
        }
    }
    return -1;
}

static DL_SPI_FRAME_FORMAT frame_format(unsigned mode)
{
    switch (mode) {
        case 1: return DL_SPI_FRAME_FORMAT_MOTO4_POL0_PHA1;
        case 2: return DL_SPI_FRAME_FORMAT_MOTO4_POL1_PHA0;
        case 3: return DL_SPI_FRAME_FORMAT_MOTO4_POL1_PHA1;
        default: return DL_SPI_FRAME_FORMAT_MOTO4_POL0_PHA0;
    }
}

static DL_SPI_CLOCK_DIVIDE_RATIO clock_ratio(unsigned ratio)
{
    switch (ratio) {
        case 2: return DL_SPI_CLOCK_DIVIDE_RATIO_2;
        case 3: return DL_SPI_CLOCK_DIVIDE_RATIO_3;
        case 4: return DL_SPI_CLOCK_DIVIDE_RATIO_4;
        case 5: return DL_SPI_CLOCK_DIVIDE_RATIO_5;
        case 6: return DL_SPI_CLOCK_DIVIDE_RATIO_6;
        case 7: return DL_SPI_CLOCK_DIVIDE_RATIO_7;
        case 8: return DL_SPI_CLOCK_DIVIDE_RATIO_8;
        default: return DL_SPI_CLOCK_DIVIDE_RATIO_1;
    }
}

static void spi_finish(spi_bus_t *bus)
{
    if (!bus->active) return;
    if (bus->claimed & 8u) {
        board_reg_gpio_set((GPIO_Regs *)bus->cs.port, bus->cs.pin);
    }
    DL_SPI_disable(bus->regs);
    DL_SPI_reset(bus->regs);
    DL_SPI_disablePower(bus->regs);
    if (bus->claimed & 1u) {
        (void)NATIVE_CORE_API->pin_af(bus->sck_name, 0, 0);
        NATIVE_CORE_API->pin_release(bus->sck_name, bus->owner);
    }
    if (bus->claimed & 2u) {
        (void)NATIVE_CORE_API->pin_af(bus->pico_name, 0, 0);
        NATIVE_CORE_API->pin_release(bus->pico_name, bus->owner);
    }
    if (bus->claimed & 4u) {
        (void)NATIVE_CORE_API->pin_af(bus->poci_name, 0, 0);
        NATIVE_CORE_API->pin_release(bus->poci_name, bus->owner);
    }
    if (bus->claimed & 8u) {
        NATIVE_CORE_API->pin_release(bus->cs_name, bus->owner);
    }
    if (bus->instance == 0u) {
        NATIVE_CORE_API->resource_release(BOARD_RES_SPI0, bus->owner);
    }
    bus->active = 0;
    bus->claimed = 0;
    if (bus->spi1_lent) {
        NATIVE_CORE_API->spi1_release();
        bus->spi1_lent = 0;
    }
}

static int claim_pin(spi_bus_t *bus, const char *name, uint8_t bit)
{
    if (NATIVE_CORE_API->pin_claim(name, bus->owner) != 0) return -1;
    bus->claimed |= bit;
    return 0;
}

static int spi_setup(spi_bus_t *bus, unsigned instance, const char *sck_name,
    const char *pico_name, const char *poci_name, const char *cs_name,
    uint32_t hz, unsigned mode, unsigned lsb)
{
    uint32_t source_hz = NATIVE_CORE_API->bus_clock_hz();
    uint32_t divider = 0;
    unsigned ratio;
    unsigned sck_pf;
    unsigned pico_pf;
    unsigned poci_pf;
    DL_SPI_Config config;
    DL_SPI_ClockConfig clock;

    bus->active = 0;
    bus->spi1_lent = 0;
    bus->claimed = 0;
    if (instance > 1u || mode > 3u || lsb > 1u || hz < 1900u ||
            hz > source_hz / 2u ||
            NATIVE_CORE_API->pin_resolve(sck_name, &bus->sck) != 0 ||
            NATIVE_CORE_API->pin_resolve(pico_name, &bus->pico) != 0 ||
            NATIVE_CORE_API->pin_resolve(poci_name, &bus->poci) != 0 ||
            NATIVE_CORE_API->pin_resolve(cs_name, &bus->cs) != 0 ||
            route_pf(instance, &bus->sck, G3507_SPI_SCLK, &sck_pf) != 0 ||
            route_pf(instance, &bus->pico, G3507_SPI_PICO, &pico_pf) != 0 ||
            route_pf(instance, &bus->poci, G3507_SPI_POCI, &poci_pf) != 0 ||
            bus->sck.iomux == bus->pico.iomux ||
            bus->sck.iomux == bus->poci.iomux ||
            bus->pico.iomux == bus->poci.iomux ||
            bus->cs.iomux == bus->sck.iomux ||
            bus->cs.iomux == bus->pico.iomux ||
            bus->cs.iomux == bus->poci.iomux) return -1;

    bus->instance = (uint8_t)instance;
    bus->regs = instance ? SPI1 : SPI0;
    bus->owner = instance ? PIN_OWN_SPI1 : PIN_OWN_SPI0;
    bus->sck_name = sck_name;
    bus->pico_name = pico_name;
    bus->poci_name = poci_name;
    bus->cs_name = cs_name;
    if (instance) {
        if (NATIVE_CORE_API->spi1_acquire(100u) != 0) return -1;
        bus->spi1_lent = 1;
    } else if (NATIVE_CORE_API->resource_claim(BOARD_RES_SPI0,
            bus->owner) != 0) return -1;

    bus->active = 1;
    if (claim_pin(bus, sck_name, 1u) != 0 ||
            claim_pin(bus, pico_name, 2u) != 0 ||
            claim_pin(bus, poci_name, 4u) != 0 ||
            claim_pin(bus, cs_name, 8u) != 0) {
        spi_finish(bus);
        return -1;
    }
    if (NATIVE_CORE_API->pin_af(sck_name, sck_pf, 0) != 0 ||
            NATIVE_CORE_API->pin_af(pico_name, pico_pf, 0) != 0 ||
            NATIVE_CORE_API->pin_af(poci_name, poci_pf, 1) != 0) {
        spi_finish(bus);
        return -1;
    }
    board_reg_pin_out((GPIO_Regs *)bus->cs.port, bus->cs.pin, bus->cs.iomux);
    board_reg_gpio_set((GPIO_Regs *)bus->cs.port, bus->cs.pin);

    for (ratio = 1u; ratio <= 8u; ratio++) {
        uint32_t denominator = 2u * ratio * hz;
        divider = NATIVE_CORE_API->udiv32(
            source_hz + denominator - 1u, denominator);
        if (divider >= 1u && divider <= 1024u) break;
    }
    if (ratio > 8u) {
        spi_finish(bus);
        return -1;
    }
    DL_SPI_reset(bus->regs);
    DL_SPI_enablePower(bus->regs);
    delay_cycles(16);
    clock.clockSel = DL_SPI_CLOCK_BUSCLK;
    clock.divideRatio = clock_ratio(ratio);
    config.mode = DL_SPI_MODE_CONTROLLER;
    config.frameFormat = frame_format(mode);
    config.parity = DL_SPI_PARITY_NONE;
    config.dataSize = DL_SPI_DATA_SIZE_8;
    config.bitOrder = lsb ? DL_SPI_BIT_ORDER_LSB_FIRST
                          : DL_SPI_BIT_ORDER_MSB_FIRST;
    config.chipSelectPin = DL_SPI_CHIP_SELECT_NONE;
    DL_SPI_setClockConfig(bus->regs, &clock);
    DL_SPI_init(bus->regs, &config);
    DL_SPI_setBitRateSerialClockDivider(bus->regs, divider - 1u);
    DL_SPI_enable(bus->regs);
    return 0;
}

static int spi_transfer(spi_bus_t *bus, const uint8_t *send, uint8_t *receive,
    size_t size, uint8_t fill)
{
    size_t i;
    board_reg_gpio_clr((GPIO_Regs *)bus->cs.port, bus->cs.pin);
    for (i = 0; i < size; i++) {
        uint32_t spins = SPI_WAIT_SPINS;
        while (DL_SPI_isTXFIFOFull(bus->regs)) {
            if (!--spins) return -1;
        }
        DL_SPI_transmitData8(bus->regs, send ? send[i] : fill);
        spins = SPI_WAIT_SPINS;
        while (DL_SPI_isRXFIFOEmpty(bus->regs)) {
            if (!--spins) return -1;
        }
        receive[i] = DL_SPI_receiveData8(bus->regs);
    }
    board_reg_gpio_set((GPIO_Regs *)bus->cs.port, bus->cs.pin);
    return 0;
}

static int do_xfer(lua_State *L, unsigned instance, const char *sck,
    const char *pico, const char *poci, const char *cs, int data_index,
    int hz_index, int mode_index, int lsb_index)
{
    uint8_t receive[SPI_MAX_TRANSFER];
    spi_bus_t bus;
    size_t size;
    const uint8_t *send = (const uint8_t *)NATIVE_CORE_API->check_lstring(
        L, data_index, &size);
    uint32_t hz = (uint32_t)NATIVE_CORE_API->opt_integer(L, hz_index, 1000000);
    unsigned mode = (unsigned)NATIVE_CORE_API->opt_integer(L, mode_index, 0);
    unsigned lsb = (unsigned)NATIVE_CORE_API->opt_integer(L, lsb_index, 0);
    int ok;
    bus.active = 0;
    bus.spi1_lent = 0;
    bus.claimed = 0;
    if (size > SPI_MAX_TRANSFER) {
        return NATIVE_CORE_API->raise_error(L, "spi:size");
    }
    if (spi_setup(&bus, instance, sck, pico, poci, cs, hz, mode, lsb) != 0) {
        spi_finish(&bus);
        return NATIVE_CORE_API->raise_error(L, "spi:config");
    }
    ok = spi_transfer(&bus, send, receive, size, 0xffu) == 0;
    spi_finish(&bus);
    if (!ok) return NATIVE_CORE_API->raise_error(L, "spi:timeout");
    NATIVE_CORE_API->push_lstring(L, (const char *)receive, size);
    return 1;
}

static int l_spi_xfer(lua_State *L)
{
    return do_xfer(L, 0u, "PA12", "PA14", "PA13",
        NATIVE_CORE_API->opt_string(L, 1, "PA18"), 2, 3, 4, 5);
}

static int l_spi_xfer_on(lua_State *L)
{
    unsigned instance = (unsigned)NATIVE_CORE_API->check_integer(L, 1);
    return do_xfer(L, instance, NATIVE_CORE_API->check_string(L, 2),
        NATIVE_CORE_API->check_string(L, 3),
        NATIVE_CORE_API->check_string(L, 4),
        NATIVE_CORE_API->check_string(L, 5), 6, 7, 8, 9);
}

static int l_spi_read_on(lua_State *L)
{
    uint8_t receive[SPI_MAX_TRANSFER];
    spi_bus_t bus;
    unsigned instance = (unsigned)NATIVE_CORE_API->check_integer(L, 1);
    const char *sck = NATIVE_CORE_API->check_string(L, 2);
    const char *pico = NATIVE_CORE_API->check_string(L, 3);
    const char *poci = NATIVE_CORE_API->check_string(L, 4);
    const char *cs = NATIVE_CORE_API->check_string(L, 5);
    int32_t size = NATIVE_CORE_API->check_integer(L, 6);
    uint8_t fill = (uint8_t)NATIVE_CORE_API->opt_integer(L, 7, 0xff);
    uint32_t hz = (uint32_t)NATIVE_CORE_API->opt_integer(L, 8, 1000000);
    unsigned mode = (unsigned)NATIVE_CORE_API->opt_integer(L, 9, 0);
    unsigned lsb = (unsigned)NATIVE_CORE_API->opt_integer(L, 10, 0);
    int ok;
    bus.active = 0;
    bus.spi1_lent = 0;
    bus.claimed = 0;
    if (size < 0 || size > (int32_t)SPI_MAX_TRANSFER) {
        return NATIVE_CORE_API->raise_error(L, "spi:size");
    }
    if (spi_setup(&bus, instance, sck, pico, poci, cs, hz, mode, lsb) != 0) {
        spi_finish(&bus);
        return NATIVE_CORE_API->raise_error(L, "spi:config");
    }
    ok = spi_transfer(&bus, 0, receive, (size_t)size, fill) == 0;
    spi_finish(&bus);
    if (!ok) return NATIVE_CORE_API->raise_error(L, "spi:timeout");
    NATIVE_CORE_API->push_lstring(L, (const char *)receive, (size_t)size);
    return 1;
}

static int l_spi_valid(lua_State *L)
{
    native_pin_t sck;
    native_pin_t pico;
    native_pin_t poci;
    unsigned pf;
    unsigned instance = (unsigned)NATIVE_CORE_API->check_integer(L, 1);
    int ok = instance <= 1u &&
        NATIVE_CORE_API->pin_resolve(NATIVE_CORE_API->check_string(L, 2),
            &sck) == 0 &&
        NATIVE_CORE_API->pin_resolve(NATIVE_CORE_API->check_string(L, 3),
            &pico) == 0 &&
        NATIVE_CORE_API->pin_resolve(NATIVE_CORE_API->check_string(L, 4),
            &poci) == 0 &&
        route_pf(instance, &sck, G3507_SPI_SCLK, &pf) == 0 &&
        route_pf(instance, &pico, G3507_SPI_PICO, &pf) == 0 &&
        route_pf(instance, &poci, G3507_SPI_POCI, &pf) == 0 &&
        sck.iomux != pico.iomux && sck.iomux != poci.iomux &&
        pico.iomux != poci.iomux;
    NATIVE_CORE_API->push_boolean(L, ok);
    return 1;
}

static int l_spi_bytes(lua_State *L)
{
    uint8_t data[3];
    int32_t value = NATIVE_CORE_API->check_integer(L, 1);
    size_t size = 1;
    unsigned i;
    if (value < 0 || value > 255) {
        return NATIVE_CORE_API->raise_error(L, "spi:byte");
    }
    data[0] = (uint8_t)value;
    for (i = 1; i < sizeof(data); i++) {
        value = NATIVE_CORE_API->opt_integer(L, (int)i + 1, -1);
        if (value == -1) break;
        if (value < 0 || value > 255) {
            return NATIVE_CORE_API->raise_error(L, "spi:byte");
        }
        data[size++] = (uint8_t)value;
    }
    NATIVE_CORE_API->push_lstring(L, (const char *)data, size);
    return 1;
}

static const native_lua_reg_t k_spi_functions[] = {
    {"xfer", l_spi_xfer}, {"xfer_on", l_spi_xfer_on},
    {"read_on", l_spi_read_on}, {"valid", l_spi_valid},
    {"bytes", l_spi_bytes}, {0, 0},
};

static int spi_init(lua_State *L, const native_core_api_t *api)
{
    if (!api || api->magic != NATIVE_CORE_API_MAGIC ||
            api->abi_version != NATIVE_CORE_ABI_VERSION ||
            api->struct_size < sizeof(native_core_api_t)) return -1;
    return api->register_lua_module(L, "spi", k_spi_functions);
}

const native_module_header_t g_native_module_header
    __attribute__((section(".module_header"), used, aligned(4))) = {
        NATIVE_MODULE_MAGIC, NATIVE_MODULE_FORMAT, NATIVE_CORE_ABI_VERSION,
        0, 0, sizeof(native_module_header_t), spi_init, 0, "spi",
    };
