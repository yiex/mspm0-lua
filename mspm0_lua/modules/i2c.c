#include "native_module.h"

#include "board_pins.h"
#include "board_resource.h"
#include "g3507_pin_routes.h"
#include <ti/driverlib/driverlib.h>

#define I2C_MAX_WRITE 4095u
#define I2C_MAX_READ 256u
#define I2C_ERROR_MASK \
    (DL_I2C_CONTROLLER_STATUS_ERROR | \
     DL_I2C_CONTROLLER_STATUS_ARBITRATION_LOST)
#define I2C_BUSY_MASK \
    (DL_I2C_CONTROLLER_STATUS_BUSY | \
     DL_I2C_CONTROLLER_STATUS_BUSY_BUS)
#define I2C_WAIT_SPINS_PER_MS 12000u
#define I2C_PROBE_TIMEOUT_MS 5u
#define I2C_PROBE_EVENTS \
    (DL_I2C_INTERRUPT_CONTROLLER_TX_DONE | \
     DL_I2C_INTERRUPT_CONTROLLER_NACK | \
     DL_I2C_INTERRUPT_CONTROLLER_STOP | \
     DL_I2C_INTERRUPT_CONTROLLER_ARBITRATION_LOST)

typedef struct {
    I2C_Regs *regs;
    const char *scl_name;
    const char *sda_name;
    native_pin_t scl;
    native_pin_t sda;
    uint8_t owner;
    uint8_t active;
} i2c_bus_t;

static int i2c_read_bytes(i2c_bus_t *bus, uint16_t address,
    uint8_t *data, size_t size, int repeated_start);

static int route_pf(unsigned instance, const native_pin_t *pin,
    unsigned role, unsigned *pf)
{
    unsigned i;
    for (i = 0; i < sizeof(g3507_i2c_routes) /
            sizeof(g3507_i2c_routes[0]); i++) {
        const g3507_route_t *route = &g3507_i2c_routes[i];
        if (route->instance == instance && route->iomux == pin->iomux &&
                route->role == role) {
            *pf = route->pf;
            return 0;
        }
    }
    return -1;
}

static int wait_status(I2C_Regs *regs, uint32_t busy_mask,
    uint32_t timeout_ms)
{
    uint32_t started = NATIVE_CORE_API->millis();
    uint32_t spins = timeout_ms * I2C_WAIT_SPINS_PER_MS;
    if (!spins) spins = I2C_WAIT_SPINS_PER_MS;
    for (;;) {
        uint32_t status = DL_I2C_getControllerStatus(regs);
        if (status & I2C_ERROR_MASK) return -1;
        if ((status & busy_mask) == 0u) return 0;
        if ((uint32_t)(NATIVE_CORE_API->millis() - started) >= timeout_ms ||
                --spins == 0u) {
            return -1;
        }
    }
}

/* A write followed by a repeated start deliberately keeps BUSY asserted.
 * Wait for TX_DONE instead of waiting for BUSY to clear in that case. */
static int wait_tx_done(I2C_Regs *regs, uint32_t timeout_ms)
{
    uint32_t started = NATIVE_CORE_API->millis();
    uint32_t spins = timeout_ms * I2C_WAIT_SPINS_PER_MS;
    if (!spins) spins = I2C_WAIT_SPINS_PER_MS;
    for (;;) {
        uint32_t events = DL_I2C_getRawInterruptStatus(regs,
            DL_I2C_INTERRUPT_CONTROLLER_TX_DONE |
            DL_I2C_INTERRUPT_CONTROLLER_NACK |
            DL_I2C_INTERRUPT_CONTROLLER_ARBITRATION_LOST);
        uint32_t status = DL_I2C_getControllerStatus(regs);
        if ((events & (DL_I2C_INTERRUPT_CONTROLLER_NACK |
                DL_I2C_INTERRUPT_CONTROLLER_ARBITRATION_LOST)) ||
                (status & I2C_ERROR_MASK)) return -1;
        if (events & DL_I2C_INTERRUPT_CONTROLLER_TX_DONE) return 0;
        if ((uint32_t)(NATIVE_CORE_API->millis() - started) >= timeout_ms ||
                --spins == 0u) return -1;
    }
}

static uint8_t i2c_resource(const i2c_bus_t *bus)
{
    return bus->regs == I2C1 ? BOARD_RES_I2C1 : BOARD_RES_I2C0;
}

static void i2c_finish(i2c_bus_t *bus)
{
    if (!bus->active) return;
    DL_I2C_resetControllerTransfer(bus->regs);
    DL_I2C_disableController(bus->regs);
    DL_I2C_reset(bus->regs);
    DL_I2C_disablePower(bus->regs);
    (void)NATIVE_CORE_API->pin_af(bus->scl_name, 0, 0);
    (void)NATIVE_CORE_API->pin_af(bus->sda_name, 0, 0);
    NATIVE_CORE_API->pin_release(bus->scl_name, bus->owner);
    NATIVE_CORE_API->pin_release(bus->sda_name, bus->owner);
    NATIVE_CORE_API->resource_release(i2c_resource(bus), bus->owner);
    bus->active = 0;
}

static int i2c_setup(i2c_bus_t *bus, unsigned instance,
    const char *scl_name, const char *sda_name, uint32_t hz)
{
    uint32_t source_hz = NATIVE_CORE_API->bus_clock_hz();
    uint32_t period;
    unsigned scl_pf;
    unsigned sda_pf;
    DL_I2C_ClockConfig clock = {
        .clockSel = DL_I2C_CLOCK_BUSCLK,
        .divideRatio = DL_I2C_CLOCK_DIVIDE_1,
    };

    bus->active = 0;
    if (instance > 1u || hz < 10000u || hz > 1000000u ||
            NATIVE_CORE_API->pin_resolve(scl_name, &bus->scl) != 0 ||
            NATIVE_CORE_API->pin_resolve(sda_name, &bus->sda) != 0 ||
            route_pf(instance, &bus->scl, G3507_I2C_SCL, &scl_pf) != 0 ||
            route_pf(instance, &bus->sda, G3507_I2C_SDA, &sda_pf) != 0 ||
            bus->scl.iomux == bus->sda.iomux) {
        return -1;
    }
    bus->regs = instance ? I2C1 : I2C0;
    bus->owner = instance ? PIN_OWN_I2C1 : PIN_OWN_I2C0;
    bus->scl_name = scl_name;
    bus->sda_name = sda_name;
    if (NATIVE_CORE_API->resource_claim(
            instance ? BOARD_RES_I2C1 : BOARD_RES_I2C0, bus->owner) != 0) {
        return -1;
    }
    if (NATIVE_CORE_API->pin_claim(scl_name, bus->owner) != 0) {
        NATIVE_CORE_API->resource_release(
            instance ? BOARD_RES_I2C1 : BOARD_RES_I2C0, bus->owner);
        return -1;
    }
    if (NATIVE_CORE_API->pin_claim(sda_name, bus->owner) != 0) {
        NATIVE_CORE_API->pin_release(scl_name, bus->owner);
        NATIVE_CORE_API->resource_release(
            instance ? BOARD_RES_I2C1 : BOARD_RES_I2C0, bus->owner);
        return -1;
    }
    bus->active = 1;

    DL_I2C_reset(bus->regs);
    DL_I2C_enablePower(bus->regs);
    delay_cycles(16);
    delay_cycles(8000);
    ((GPIO_Regs *)bus->scl.port)->DOECLR31_0 = bus->scl.pin;
    ((GPIO_Regs *)bus->sda.port)->DOECLR31_0 = bus->sda.pin;
    DL_GPIO_initPeripheralInputFunctionFeatures(bus->scl.iomux, scl_pf,
        DL_GPIO_INVERSION_DISABLE, DL_GPIO_RESISTOR_PULL_UP,
        DL_GPIO_HYSTERESIS_DISABLE, DL_GPIO_WAKEUP_DISABLE);
    DL_GPIO_initPeripheralInputFunctionFeatures(bus->sda.iomux, sda_pf,
        DL_GPIO_INVERSION_DISABLE, DL_GPIO_RESISTOR_PULL_UP,
        DL_GPIO_HYSTERESIS_DISABLE, DL_GPIO_WAKEUP_DISABLE);
    DL_GPIO_enableHiZ(bus->scl.iomux);
    DL_GPIO_enableHiZ(bus->sda.iomux);
    DL_I2C_setClockConfig(bus->regs, &clock);
    DL_I2C_disableAnalogGlitchFilter(bus->regs);
    DL_I2C_resetControllerTransfer(bus->regs);
    DL_I2C_flushControllerTXFIFO(bus->regs);
    DL_I2C_flushControllerRXFIFO(bus->regs);
    period = NATIVE_CORE_API->udiv32(source_hz, hz * 10u);
    if (period) period--;
    if (period < 1u) period = 1u;
    if (period > 127u) period = 127u;
    DL_I2C_setTimerPeriod(bus->regs, (uint8_t)period);
    DL_I2C_setControllerTXFIFOThreshold(bus->regs,
        DL_I2C_TX_FIFO_LEVEL_EMPTY);
    DL_I2C_setControllerRXFIFOThreshold(bus->regs,
        DL_I2C_RX_FIFO_LEVEL_BYTES_1);
    DL_I2C_enableControllerClockStretching(bus->regs);
    DL_I2C_enableController(bus->regs);
    delay_cycles(2000);
    return wait_status(bus->regs, I2C_BUSY_MASK, 20u);
}

static int i2c_probe_address(i2c_bus_t *bus, uint16_t address)
{
    uint8_t ignored;
    /* A zero-byte TX does not reliably issue an address phase on MSPM0.
     * One-byte RX produces a real address ACK/NACK without mutating a target. */
    return i2c_read_bytes(bus, address, &ignored, 1u, 0);
}

static int i2c_write_bytes(i2c_bus_t *bus, uint16_t address,
    const uint8_t *data, size_t size, int stop)
{
    size_t sent = 0;
    uint32_t started;
    uint32_t spins = 50u * I2C_WAIT_SPINS_PER_MS;
    if (size > I2C_MAX_WRITE || wait_status(bus->regs, I2C_BUSY_MASK, 20u)) {
        return -1;
    }
    DL_I2C_resetControllerTransfer(bus->regs);
    if (!stop) {
        DL_I2C_clearInterruptStatus(bus->regs,
            DL_I2C_INTERRUPT_CONTROLLER_TX_DONE |
            DL_I2C_INTERRUPT_CONTROLLER_NACK |
            DL_I2C_INTERRUPT_CONTROLLER_ARBITRATION_LOST);
    }
    if (size) {
        sent = DL_I2C_fillControllerTXFIFO(bus->regs, (uint8_t *)data,
            (uint16_t)size);
    }
    if (stop) {
        DL_I2C_startControllerTransfer(bus->regs, address,
            DL_I2C_CONTROLLER_DIRECTION_TX, (uint16_t)size);
    } else {
        if (address > 0x7fu) return -1;
        DL_I2C_startControllerTransferAdvanced(bus->regs, address,
            DL_I2C_CONTROLLER_DIRECTION_TX, (uint16_t)size,
            DL_I2C_CONTROLLER_START_ENABLE, DL_I2C_CONTROLLER_STOP_DISABLE,
            DL_I2C_CONTROLLER_ACK_DISABLE);
    }
    started = NATIVE_CORE_API->millis();
    while (sent < size) {
        uint32_t status = DL_I2C_getControllerStatus(bus->regs);
        if (status & I2C_ERROR_MASK) return -1;
        if (!DL_I2C_isControllerTXFIFOFull(bus->regs)) {
            DL_I2C_transmitControllerData(bus->regs, data[sent++]);
            started = NATIVE_CORE_API->millis();
            spins = 50u * I2C_WAIT_SPINS_PER_MS;
        } else if ((uint32_t)(NATIVE_CORE_API->millis() - started) >= 50u ||
                --spins == 0u) {
            return -1;
        }
    }
    return stop ? wait_status(bus->regs, I2C_BUSY_MASK, 50u) :
        wait_tx_done(bus->regs, 50u);
}

static int i2c_read_bytes(i2c_bus_t *bus, uint16_t address,
    uint8_t *data, size_t size, int repeated_start)
{
    size_t got = 0;
    uint32_t started;
    uint32_t spins = 50u * I2C_WAIT_SPINS_PER_MS;
    if (size > I2C_MAX_READ || (!repeated_start &&
            wait_status(bus->regs, I2C_BUSY_MASK, 20u))) return -1;
    if (!repeated_start) DL_I2C_resetControllerTransfer(bus->regs);
    if (repeated_start) {
        if (address > 0x7fu) return -1;
        DL_I2C_startControllerTransferAdvanced(bus->regs, address,
            DL_I2C_CONTROLLER_DIRECTION_RX, (uint16_t)size,
            DL_I2C_CONTROLLER_START_ENABLE, DL_I2C_CONTROLLER_STOP_ENABLE,
            DL_I2C_CONTROLLER_ACK_ENABLE);
    } else {
        DL_I2C_startControllerTransfer(bus->regs, address,
            DL_I2C_CONTROLLER_DIRECTION_RX, (uint16_t)size);
    }
    started = NATIVE_CORE_API->millis();
    while (got < size) {
        uint32_t status = DL_I2C_getControllerStatus(bus->regs);
        if (status & I2C_ERROR_MASK) return -1;
        if (!DL_I2C_isControllerRXFIFOEmpty(bus->regs)) {
            data[got++] = DL_I2C_receiveControllerData(bus->regs);
            started = NATIVE_CORE_API->millis();
            spins = 50u * I2C_WAIT_SPINS_PER_MS;
        } else if ((uint32_t)(NATIVE_CORE_API->millis() - started) >= 50u ||
                --spins == 0u) {
            return -1;
        }
    }
    return wait_status(bus->regs, I2C_BUSY_MASK, 50u);
}

static int i2c_recover(unsigned instance, const char *scl_name,
    const char *sda_name)
{
    native_pin_t scl;
    native_pin_t sda;
    unsigned pf;
    uint8_t owner = instance ? PIN_OWN_I2C1 : PIN_OWN_I2C0;
    unsigned i;
    if (instance > 1u ||
            NATIVE_CORE_API->pin_resolve(scl_name, &scl) != 0 ||
            NATIVE_CORE_API->pin_resolve(sda_name, &sda) != 0 ||
            scl.iomux == sda.iomux ||
            route_pf(instance, &scl, G3507_I2C_SCL, &pf) != 0 ||
            route_pf(instance, &sda, G3507_I2C_SDA, &pf) != 0) return -1;
    if (NATIVE_CORE_API->resource_claim(instance ? BOARD_RES_I2C1 :
            BOARD_RES_I2C0, owner) != 0) return -1;
    if (NATIVE_CORE_API->pin_claim(scl_name, owner) != 0) {
        NATIVE_CORE_API->resource_release(instance ? BOARD_RES_I2C1 :
            BOARD_RES_I2C0, owner);
        return -1;
    }
    if (NATIVE_CORE_API->pin_claim(sda_name, owner) != 0) {
        NATIVE_CORE_API->pin_release(scl_name, owner);
        NATIVE_CORE_API->resource_release(instance ? BOARD_RES_I2C1 :
            BOARD_RES_I2C0, owner);
        return -1;
    }
    DL_GPIO_initDigitalInputFeatures(scl.iomux, DL_GPIO_INVERSION_DISABLE,
        DL_GPIO_RESISTOR_PULL_UP, DL_GPIO_HYSTERESIS_DISABLE,
        DL_GPIO_WAKEUP_DISABLE);
    DL_GPIO_initDigitalInputFeatures(sda.iomux, DL_GPIO_INVERSION_DISABLE,
        DL_GPIO_RESISTOR_PULL_UP, DL_GPIO_HYSTERESIS_DISABLE,
        DL_GPIO_WAKEUP_DISABLE);
    for (i = 0; i < 9u; i++) {
        DL_GPIO_clearPins((GPIO_Regs *)scl.port, scl.pin);
        DL_GPIO_enableOutput((GPIO_Regs *)scl.port, scl.pin);
        delay_cycles(160);
        DL_GPIO_disableOutput((GPIO_Regs *)scl.port, scl.pin);
        delay_cycles(160);
    }
    DL_GPIO_clearPins((GPIO_Regs *)sda.port, sda.pin);
    DL_GPIO_enableOutput((GPIO_Regs *)sda.port, sda.pin);
    delay_cycles(160);
    DL_GPIO_disableOutput((GPIO_Regs *)scl.port, scl.pin);
    delay_cycles(160);
    DL_GPIO_disableOutput((GPIO_Regs *)sda.port, sda.pin);
    delay_cycles(160);
    i = DL_GPIO_readPins((GPIO_Regs *)scl.port, scl.pin) &&
        DL_GPIO_readPins((GPIO_Regs *)sda.port, sda.pin) ? 0u : 1u;
    (void)NATIVE_CORE_API->pin_af(scl_name, 0, 0);
    (void)NATIVE_CORE_API->pin_af(sda_name, 0, 0);
    NATIVE_CORE_API->pin_release(scl_name, owner);
    NATIVE_CORE_API->pin_release(sda_name, owner);
    NATIVE_CORE_API->resource_release(instance ? BOARD_RES_I2C1 :
        BOARD_RES_I2C0, owner);
    return i ? -1 : 0;
}

static int do_write(lua_State *L, unsigned instance, const char *scl,
    const char *sda, int addr_index, int data_index, int hz_index)
{
    i2c_bus_t bus;
    size_t size;
    uint32_t address = (uint32_t)NATIVE_CORE_API->check_integer(L, addr_index);
    const uint8_t *data = (const uint8_t *)NATIVE_CORE_API->check_lstring(
        L, data_index, &size);
    uint32_t hz = (uint32_t)NATIVE_CORE_API->opt_integer(L, hz_index, 100000);
    int ok;
    bus.active = 0;
    ok = address <= 0x3ffu && size <= I2C_MAX_WRITE &&
        i2c_setup(&bus, instance, scl, sda, hz) == 0 &&
        i2c_write_bytes(&bus, address, data, size, 1) == 0;
    i2c_finish(&bus);
    NATIVE_CORE_API->push_boolean(L, ok);
    return 1;
}

static int do_read(lua_State *L, unsigned instance, const char *scl,
    const char *sda, int addr_index, int size_index, int hz_index)
{
    uint8_t data[I2C_MAX_READ];
    i2c_bus_t bus;
    uint32_t address = (uint32_t)NATIVE_CORE_API->check_integer(L, addr_index);
    int32_t size = NATIVE_CORE_API->check_integer(L, size_index);
    uint32_t hz = (uint32_t)NATIVE_CORE_API->opt_integer(L, hz_index, 100000);
    int ok;
    bus.active = 0;
    ok = address <= 0x3ffu && size >= 0 &&
        size <= (int32_t)I2C_MAX_READ &&
        i2c_setup(&bus, instance, scl, sda, hz) == 0 &&
        i2c_read_bytes(&bus, address, data, (size_t)size, 0) == 0;
    i2c_finish(&bus);
    if (!ok) return NATIVE_CORE_API->raise_error(L, "i2c:read");
    NATIVE_CORE_API->push_lstring(L, (const char *)data, (size_t)size);
    return 1;
}

static int do_write_read(lua_State *L, unsigned instance, const char *scl,
    const char *sda, int addr_index, int write_index, int size_index,
    int hz_index)
{
    uint8_t data[I2C_MAX_READ];
    i2c_bus_t bus;
    size_t write_size;
    uint32_t address = (uint32_t)NATIVE_CORE_API->check_integer(L, addr_index);
    const uint8_t *write_data =
        (const uint8_t *)NATIVE_CORE_API->check_lstring(L, write_index,
            &write_size);
    int32_t read_size = NATIVE_CORE_API->check_integer(L, size_index);
    uint32_t hz = (uint32_t)NATIVE_CORE_API->opt_integer(L, hz_index, 100000);
    int ok;
    bus.active = 0;
    ok = address <= 0x7fu && write_size <= I2C_MAX_WRITE &&
        read_size >= 0 && read_size <= (int32_t)I2C_MAX_READ &&
        i2c_setup(&bus, instance, scl, sda, hz) == 0;
    if (ok) ok = i2c_write_bytes(&bus, address, write_data, write_size, 1) == 0;
    /* Reinitialise between phases.  On MSPM0 a completed TX transfer can
     * leave controller state that rejects a following RX transfer even after
     * STOP; separate write_on/read_on calls demonstrate the required reset. */
    i2c_finish(&bus);
    if (ok) {
        bus.active = 0;
        ok = i2c_setup(&bus, instance, scl, sda, hz) == 0 &&
            i2c_read_bytes(&bus, address, data, (size_t)read_size, 0) == 0;
    }
    i2c_finish(&bus);
    if (!ok) return NATIVE_CORE_API->raise_error(L, "i2c:xfer");
    NATIVE_CORE_API->push_lstring(L, (const char *)data, (size_t)read_size);
    return 1;
}

static int l_i2c_write(lua_State *L)
{
    return do_write(L, 1u, "PA15", "PA16", 1, 2, 3);
}

static int l_i2c_read(lua_State *L)
{
    return do_read(L, 1u, "PA15", "PA16", 1, 2, 3);
}

static int l_i2c_write_read(lua_State *L)
{
    return do_write_read(L, 1u, "PA15", "PA16", 1, 2, 3, 4);
}

static int l_i2c_write_on(lua_State *L)
{
    unsigned instance = (unsigned)NATIVE_CORE_API->check_integer(L, 1);
    return do_write(L, instance, NATIVE_CORE_API->check_string(L, 2),
        NATIVE_CORE_API->check_string(L, 3), 4, 5, 6);
}

static int l_i2c_read_on(lua_State *L)
{
    unsigned instance = (unsigned)NATIVE_CORE_API->check_integer(L, 1);
    return do_read(L, instance, NATIVE_CORE_API->check_string(L, 2),
        NATIVE_CORE_API->check_string(L, 3), 4, 5, 6);
}

static int l_i2c_write_read_on(lua_State *L)
{
    unsigned instance = (unsigned)NATIVE_CORE_API->check_integer(L, 1);
    return do_write_read(L, instance, NATIVE_CORE_API->check_string(L, 2),
        NATIVE_CORE_API->check_string(L, 3), 4, 5, 6, 7);
}

static int l_i2c_probe_on(lua_State *L)
{
    i2c_bus_t bus;
    unsigned instance = (unsigned)NATIVE_CORE_API->check_integer(L, 1);
    const char *scl = NATIVE_CORE_API->check_string(L, 2);
    const char *sda = NATIVE_CORE_API->check_string(L, 3);
    uint32_t address = (uint32_t)NATIVE_CORE_API->check_integer(L, 4);
    uint32_t hz = (uint32_t)NATIVE_CORE_API->opt_integer(L, 5, 100000);
    int ok;
    bus.active = 0;
    ok = address <= 0x3ffu && i2c_setup(&bus, instance, scl, sda, hz) == 0 &&
        i2c_probe_address(&bus, (uint16_t)address) == 0;
    i2c_finish(&bus);
    NATIVE_CORE_API->push_boolean(L, ok);
    return 1;
}

static int l_i2c_recover(lua_State *L)
{
    int ok = i2c_recover((unsigned)NATIVE_CORE_API->check_integer(L, 1),
        NATIVE_CORE_API->check_string(L, 2),
        NATIVE_CORE_API->check_string(L, 3)) == 0;
    NATIVE_CORE_API->push_boolean(L, ok);
    return 1;
}

static int l_i2c_valid(lua_State *L)
{
    native_pin_t scl;
    native_pin_t sda;
    unsigned pf;
    unsigned instance = (unsigned)NATIVE_CORE_API->check_integer(L, 1);
    int ok = instance <= 1u &&
        NATIVE_CORE_API->pin_resolve(NATIVE_CORE_API->check_string(L, 2),
            &scl) == 0 &&
        NATIVE_CORE_API->pin_resolve(NATIVE_CORE_API->check_string(L, 3),
            &sda) == 0 &&
        route_pf(instance, &scl, G3507_I2C_SCL, &pf) == 0 &&
        route_pf(instance, &sda, G3507_I2C_SDA, &pf) == 0 &&
        scl.iomux != sda.iomux;
    NATIVE_CORE_API->push_boolean(L, ok);
    return 1;
}

static int l_i2c_bytes(lua_State *L)
{
    uint8_t data[3];
    int32_t value;
    size_t size = 1;
    unsigned i;

    value = NATIVE_CORE_API->check_integer(L, 1);
    if (value < 0 || value > 255) {
        return NATIVE_CORE_API->raise_error(L, "i2c:byte");
    }
    data[0] = (uint8_t)value;
    for (i = 1; i < 3; i++) {
        value = NATIVE_CORE_API->opt_integer(L, (int)i + 1, -1);
        if (value == -1) break;
        if (value < 0 || value > 255) {
            return NATIVE_CORE_API->raise_error(L, "i2c:byte");
        }
        data[size++] = (uint8_t)value;
    }
    NATIVE_CORE_API->push_lstring(L, (const char *)data, size);
    return 1;
}

static const native_lua_reg_t k_i2c_functions[] = {
    {"write", l_i2c_write}, {"read", l_i2c_read},
    {"write_read", l_i2c_write_read},
    {"write_on", l_i2c_write_on}, {"read_on", l_i2c_read_on},
    {"write_read_on", l_i2c_write_read_on},
    {"probe_on", l_i2c_probe_on}, {"recover", l_i2c_recover},
    {"valid", l_i2c_valid}, {"bytes", l_i2c_bytes}, {0, 0},
};

static int i2c_init(lua_State *L, const native_core_api_t *api)
{
    if (!api || api->magic != NATIVE_CORE_API_MAGIC ||
            api->abi_version != NATIVE_CORE_ABI_VERSION ||
            api->struct_size < sizeof(native_core_api_t)) return -1;
    return api->register_lua_module(L, "i2c", k_i2c_functions);
}

const native_module_header_t g_native_module_header
    __attribute__((section(".module_header"), used, aligned(4))) = {
        NATIVE_MODULE_MAGIC, NATIVE_MODULE_FORMAT, NATIVE_CORE_ABI_VERSION,
        0, 0, sizeof(native_module_header_t), i2c_init, 0, "i2c",
    };
