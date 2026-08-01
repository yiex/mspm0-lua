#include "board_i2c1.h"

#include <string.h>
#include <ti/driverlib/driverlib.h>

#include "board_irq.h"
#include "board_pins.h"
#include "board_uart_app.h"
#include "ti_msp_dl_config.h"

#define I2C1_BUSY_MASK \
    (DL_I2C_CONTROLLER_STATUS_BUSY | DL_I2C_CONTROLLER_STATUS_BUSY_BUS)

static char s_scl[8];
static char s_sda[8];
static uint32_t s_hz;
static uint32_t s_generation;

static int wait_ctrl_ready(uint32_t timeout_ms)
{
    uint32_t start = board_millis();
    while (DL_I2C_getControllerStatus(I2C1) & I2C1_BUSY_MASK) {
        /* Keep app UART rings filled while OLED I2C holds the CPU. */
        board_uart_app_poll();
        if ((uint32_t)(board_millis() - start) >= timeout_ms) {
            return -1;
        }
    }
    return 0;
}

static void pins_release_doe(const char *scl, const char *sda)
{
    board_pin_t a, b;
    if (board_pin_resolve(scl, &a) != 0) {
        return;
    }
    if (board_pin_resolve(sda, &b) != 0) {
        return;
    }
    DL_GPIO_disableOutput(a.port, a.pin);
    DL_GPIO_disableOutput(b.port, b.pin);
}

static void i2c1_flush(void)
{
    DL_I2C_disableControllerBurst(I2C1);
    DL_I2C_flushControllerTXFIFO(I2C1);
    DL_I2C_flushControllerRXFIFO(I2C1);
    DL_I2C_resetControllerTransfer(I2C1);
}

/*
 * Recreate the recovery sequence that was validated with the SSD1306:
 * drive nine complete SCL cycles even when SDA initially reads high, then
 * issue STOP. A slave can be mid-byte without holding SDA low.
 */
static int bus_recover(const char *scl, const char *sda)
{
    board_pin_t pscl, psda;
    unsigned i;

    if (board_pin_resolve(scl, &pscl) != 0) {
        return -1;
    }
    if (board_pin_resolve(sda, &psda) != 0) {
        return -1;
    }

    DL_I2C_disableController(I2C1);
    DL_I2C_reset(I2C1);

    DL_GPIO_initDigitalOutput(pscl.iomux);
    DL_GPIO_initDigitalInputFeatures(psda.iomux, DL_GPIO_INVERSION_DISABLE,
        DL_GPIO_RESISTOR_PULL_UP, DL_GPIO_HYSTERESIS_DISABLE,
        DL_GPIO_WAKEUP_DISABLE);
    DL_GPIO_setPins(pscl.port, pscl.pin);
    DL_GPIO_enableOutput(pscl.port, pscl.pin);
    for (i = 0; i < 9u; i++) {
        DL_GPIO_clearPins(pscl.port, pscl.pin);
        delay_cycles(1600);
        DL_GPIO_setPins(pscl.port, pscl.pin);
        delay_cycles(1600);
    }

    /* STOP: SDA low, SCL high, then SDA high. */
    DL_GPIO_initDigitalOutput(psda.iomux);
    DL_GPIO_clearPins(psda.port, psda.pin);
    DL_GPIO_enableOutput(psda.port, psda.pin);
    delay_cycles(800);
    DL_GPIO_setPins(pscl.port, pscl.pin);
    delay_cycles(800);
    DL_GPIO_setPins(psda.port, psda.pin);
    delay_cycles(800);

    i = (DL_GPIO_readPins(pscl.port, pscl.pin) &&
            DL_GPIO_readPins(psda.port, psda.pin))
        ? 0u
        : 1u;
    DL_GPIO_disableOutput(pscl.port, pscl.pin);
    DL_GPIO_disableOutput(psda.port, psda.pin);
    return i ? -1 : 0;
}

static int i2c1_hw_init(const char *scl, const char *sda, uint32_t hz,
    int recover_bus)
{
    DL_I2C_ClockConfig clk = {
        .clockSel = DL_I2C_CLOCK_BUSCLK,
        .divideRatio = DL_I2C_CLOCK_DIVIDE_1,
    };
    uint32_t bus = g_uart_busclk_hz ? g_uart_busclk_hz : 32000000u;
    uint32_t tp;
    board_pin_t p;

    if (recover_bus) {
        (void)bus_recover(scl, sda);
    } else {
        DL_I2C_disableController(I2C1);
        pins_release_doe(scl, sda);
    }

    if (board_pin_resolve(scl, &p) != 0) {
        return -1;
    }
    DL_GPIO_disableOutput(p.port, p.pin);
    DL_GPIO_initPeripheralInputFunctionFeatures(p.iomux, 4,
        DL_GPIO_INVERSION_DISABLE, DL_GPIO_RESISTOR_PULL_UP,
        DL_GPIO_HYSTERESIS_DISABLE, DL_GPIO_WAKEUP_DISABLE);
    DL_GPIO_enableHiZ(p.iomux);

    if (board_pin_resolve(sda, &p) != 0) {
        return -1;
    }
    DL_GPIO_disableOutput(p.port, p.pin);
    DL_GPIO_initPeripheralInputFunctionFeatures(p.iomux, 4,
        DL_GPIO_INVERSION_DISABLE, DL_GPIO_RESISTOR_PULL_UP,
        DL_GPIO_HYSTERESIS_DISABLE, DL_GPIO_WAKEUP_DISABLE);
    DL_GPIO_enableHiZ(p.iomux);

    DL_I2C_reset(I2C1);
    DL_I2C_enablePower(I2C1);
    delay_cycles(POWER_STARTUP_DELAY);
    delay_cycles(8000);

    DL_I2C_setClockConfig(I2C1, &clk);
    DL_I2C_disableAnalogGlitchFilter(I2C1);
    i2c1_flush();

    if (hz < 10000u) {
        hz = 10000u;
    }
    tp = bus / (hz * 10u);
    if (tp) {
        tp--;
    }
    if (tp < 1u) {
        tp = 1u;
    }
    if (tp > 127u) {
        tp = 127u;
    }
    DL_I2C_setTimerPeriod(I2C1, tp);
    DL_I2C_setControllerTXFIFOThreshold(I2C1, DL_I2C_TX_FIFO_LEVEL_EMPTY);
    DL_I2C_setControllerRXFIFOThreshold(I2C1, DL_I2C_RX_FIFO_LEVEL_BYTES_1);
    DL_I2C_enableControllerClockStretching(I2C1);
    DL_I2C_enableController(I2C1);
    delay_cycles(2000);
    return 0;
}

/* Reset ACTIVE/peripheral state first; only clock the bus if it remains busy. */
static int i2c1_recover(uint32_t failed_status)
{
    int hard = (failed_status & I2C1_BUSY_MASK) != 0;

    i2c1_flush();
    if (i2c1_hw_init(s_scl, s_sda, s_hz, hard) != 0) {
        return -1;
    }
    if (wait_ctrl_ready(3) == 0) {
        return 0;
    }
    if (i2c1_hw_init(s_scl, s_sda, s_hz, 1) != 0) {
        return -1;
    }
    return wait_ctrl_ready(10);
}

static int bus_valid(const board_i2c1_t *bus)
{
    return bus && bus->open && bus->generation == s_generation;
}

int board_i2c1_open(board_i2c1_t *bus, const char *scl, const char *sda,
    uint32_t hz)
{
    int pair1 = scl && sda && !strcmp(scl, "PA17") && !strcmp(sda, "PA18");
    int pair2 = scl && sda && !strcmp(scl, "PA15") && !strcmp(sda, "PA16");
    if (!bus || (!pair1 && !pair2) || hz < 10000u || hz > 1000000u) {
        return -1;
    }

    if (s_hz) {
        DL_I2C_disableController(I2C1);
        pins_release_doe(s_scl, s_sda);
    }
    strncpy(s_scl, scl, sizeof(s_scl) - 1u);
    s_scl[sizeof(s_scl) - 1u] = 0;
    strncpy(s_sda, sda, sizeof(s_sda) - 1u);
    s_sda[sizeof(s_sda) - 1u] = 0;
    s_hz = hz;

    if (i2c1_hw_init(scl, sda, hz, 1) != 0 || wait_ctrl_ready(20) != 0) {
        i2c1_flush();
        DL_I2C_disableController(I2C1);
        pins_release_doe(scl, sda);
        bus->open = 0;
        bus->generation = 0;
        s_hz = 0;
        return -1;
    }
    s_generation++;
    if (!s_generation) {
        s_generation++;
    }
    bus->generation = s_generation;
    bus->open = 1;
    return 0;
}

void board_i2c1_close(board_i2c1_t *bus)
{
    if (!bus) {
        return;
    }
    if (bus_valid(bus)) {
        i2c1_flush();
        DL_I2C_disableController(I2C1);
        pins_release_doe(s_scl, s_sda);
    }
    bus->open = 0;
    bus->generation = 0;
}

int board_i2c1_ready(const board_i2c1_t *bus)
{
    return bus_valid(bus);
}

static int i2c1_write_once(uint8_t addr7, const uint8_t *data, size_t n,
    uint32_t *failed_status)
{
    size_t sent;
    uint32_t st;
    uint32_t t0;

    if (wait_ctrl_ready(10) != 0) {
        *failed_status = DL_I2C_getControllerStatus(I2C1) | I2C1_BUSY_MASK;
        return -1;
    }
    i2c1_flush();

    if (n == 0) {
        DL_I2C_startControllerTransfer(I2C1, addr7,
            DL_I2C_CONTROLLER_DIRECTION_TX, 0);
    } else {
        sent = (size_t)DL_I2C_fillControllerTXFIFO(
            I2C1, data, (uint16_t)n);
        DL_I2C_startControllerTransfer(I2C1, addr7,
            DL_I2C_CONTROLLER_DIRECTION_TX, (uint16_t)n);

        t0 = board_millis();
        while (sent < n) {
            board_uart_app_poll();
            st = DL_I2C_getControllerStatus(I2C1);
            if (st & (DL_I2C_CONTROLLER_STATUS_ERROR |
                    DL_I2C_CONTROLLER_STATUS_ARBITRATION_LOST)) {
                *failed_status = st;
                return -1;
            }
            if (!DL_I2C_isControllerTXFIFOFull(I2C1)) {
                DL_I2C_transmitControllerData(I2C1, data[sent++]);
                t0 = board_millis();
            } else if ((uint32_t)(board_millis() - t0) >= 30u) {
                *failed_status = st | I2C1_BUSY_MASK;
                return -1;
            }
        }
    }

    if (wait_ctrl_ready(30) != 0) {
        *failed_status = DL_I2C_getControllerStatus(I2C1) | I2C1_BUSY_MASK;
        return -1;
    }
    st = DL_I2C_getControllerStatus(I2C1);
    if (st & (DL_I2C_CONTROLLER_STATUS_ERROR |
            DL_I2C_CONTROLLER_STATUS_ARBITRATION_LOST)) {
        *failed_status = st;
        return -1;
    }
    return 0;
}

int board_i2c1_write(board_i2c1_t *bus, uint8_t addr7,
    const uint8_t *data, size_t n)
{
    uint32_t failed_status = 0;
    int attempt;

    if (!bus_valid(bus) || (!data && n) || n > 1024u) {
        return -1;
    }
    for (attempt = 0; attempt < 2; attempt++) {
        if (i2c1_write_once(addr7, data, n, &failed_status) == 0) {
            return 0;
        }
        if (attempt || i2c1_recover(failed_status) != 0) {
            break;
        }
    }
    /* A NACK is a transaction failure, not a dead handle. Keep the bus open
     * when controller recovery succeeded so the caller can try another
     * address or transaction without reopening I2C1. */
    if (i2c1_recover(failed_status) != 0) {
        board_i2c1_close(bus);
    }
    return -1;
}

static int i2c1_read_once(uint8_t addr7, uint8_t *data, size_t n,
    uint32_t *failed_status)
{
    size_t got = 0;
    uint32_t st;
    uint32_t t0;

    if (wait_ctrl_ready(10) != 0) {
        *failed_status = DL_I2C_getControllerStatus(I2C1) | I2C1_BUSY_MASK;
        return -1;
    }
    i2c1_flush();
    DL_I2C_startControllerTransfer(I2C1, addr7,
        DL_I2C_CONTROLLER_DIRECTION_RX, (uint16_t)n);
    t0 = board_millis();
    while (got < n) {
        board_uart_app_poll();
        st = DL_I2C_getControllerStatus(I2C1);
        if (st & (DL_I2C_CONTROLLER_STATUS_ERROR |
                DL_I2C_CONTROLLER_STATUS_ARBITRATION_LOST)) {
            *failed_status = st;
            return -1;
        }
        if (!DL_I2C_isControllerRXFIFOEmpty(I2C1)) {
            data[got++] = DL_I2C_receiveControllerData(I2C1);
            t0 = board_millis();
        } else if ((uint32_t)(board_millis() - t0) >= 30u) {
            *failed_status = st | I2C1_BUSY_MASK;
            return -1;
        }
    }
    if (wait_ctrl_ready(30) != 0) {
        *failed_status = DL_I2C_getControllerStatus(I2C1) | I2C1_BUSY_MASK;
        return -1;
    }
    st = DL_I2C_getControllerStatus(I2C1);
    if (st & (DL_I2C_CONTROLLER_STATUS_ERROR |
            DL_I2C_CONTROLLER_STATUS_ARBITRATION_LOST)) {
        *failed_status = st;
        return -1;
    }
    return 0;
}

int board_i2c1_read(board_i2c1_t *bus, uint8_t addr7, uint8_t *data, size_t n)
{
    uint32_t failed_status = 0;
    int attempt;

    if (!bus_valid(bus) || !data || n > 255u) {
        return -1;
    }
    if (!n) {
        return 0;
    }
    for (attempt = 0; attempt < 2; attempt++) {
        if (i2c1_read_once(addr7, data, n, &failed_status) == 0) {
            return 0;
        }
        if (attempt || i2c1_recover(failed_status) != 0) {
            break;
        }
    }
    if (i2c1_recover(failed_status) != 0) {
        board_i2c1_close(bus);
    }
    return -1;
}

int board_i2c1_write_read(board_i2c1_t *bus, uint8_t addr7,
    const uint8_t *w, size_t wn, uint8_t *r, size_t rn)
{
    return board_i2c1_write(bus, addr7, w, wn) ||
            board_i2c1_read(bus, addr7, r, rn)
        ? -1
        : 0;
}
