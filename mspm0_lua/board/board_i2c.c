#include "board_i2c.h"
#include "board_delay.h"
#include "board_irq.h"
#include "board_pins.h"
#include "ti_msp_dl_config.h"

#include <string.h>

static int wait_idle(uint32_t timeout_ms)
{
    uint32_t t0 = board_millis();
    while (!(DL_I2C_getControllerStatus(I2C0) & DL_I2C_CONTROLLER_STATUS_IDLE)) {
        if ((uint32_t)(board_millis() - t0) > timeout_ms) {
            return -1;
        }
    }
    return 0;
}

static int wait_not_busy(uint32_t timeout_ms)
{
    uint32_t t0 = board_millis();
    while (DL_I2C_getControllerStatus(I2C0) & DL_I2C_CONTROLLER_STATUS_BUSY) {
        if ((uint32_t)(board_millis() - t0) > timeout_ms) {
            return -1;
        }
    }
    return 0;
}

static void i2c0_hw_init(int hz)
{
    DL_I2C_ClockConfig clk = {
        .clockSel = DL_I2C_CLOCK_BUSCLK,
        .divideRatio = DL_I2C_CLOCK_DIVIDE_1,
    };
    uint32_t bus = g_uart_busclk_hz ? g_uart_busclk_hz : 32000000u;
    uint32_t tp;

    DL_I2C_reset(I2C0);
    DL_I2C_enablePower(I2C0);
    delay_cycles(POWER_STARTUP_DELAY);

    DL_GPIO_initPeripheralInputFunctionFeatures(IOMUX_PINCM1,
        IOMUX_PINCM1_PF_I2C0_SDA, DL_GPIO_INVERSION_DISABLE,
        DL_GPIO_RESISTOR_NONE, DL_GPIO_HYSTERESIS_DISABLE,
        DL_GPIO_WAKEUP_DISABLE);
    DL_GPIO_initPeripheralInputFunctionFeatures(IOMUX_PINCM2,
        IOMUX_PINCM2_PF_I2C0_SCL, DL_GPIO_INVERSION_DISABLE,
        DL_GPIO_RESISTOR_NONE, DL_GPIO_HYSTERESIS_DISABLE,
        DL_GPIO_WAKEUP_DISABLE);
    DL_GPIO_enableHiZ(IOMUX_PINCM1);
    DL_GPIO_enableHiZ(IOMUX_PINCM2);

    DL_I2C_setClockConfig(I2C0, &clk);
    DL_I2C_disableAnalogGlitchFilter(I2C0);
    DL_I2C_resetControllerTransfer(I2C0);
    if (hz < 10000) {
        hz = 10000;
    }
    tp = bus / ((uint32_t)hz * 10u);
    if (tp > 0) {
        tp -= 1u;
    }
    if (tp > 127u) {
        tp = 127u;
    }
    DL_I2C_setTimerPeriod(I2C0, tp);
    DL_I2C_setControllerTXFIFOThreshold(I2C0, DL_I2C_TX_FIFO_LEVEL_EMPTY);
    DL_I2C_setControllerRXFIFOThreshold(I2C0, DL_I2C_RX_FIFO_LEVEL_BYTES_1);
    DL_I2C_enableControllerClockStretching(I2C0);
    DL_I2C_enableController(I2C0);
}

static int i2c0_fail(board_i2c_t *bus)
{
    /* Abort the failed transfer before touching ACTIVE/controller state
     * (MSPM0G350x I2C_ERR_05), then restore a known configuration. */
    DL_I2C_resetControllerTransfer(I2C0);
    i2c0_hw_init(bus->hz);
    return -1;
}

int board_i2c_open(board_i2c_t *bus, const char *scl, const char *sda, int hz)
{
    if (!bus) {
        return -1;
    }
    if (!scl) {
        scl = "PA1";
    }
    if (!sda) {
        sda = "PA0";
    }
    /* HW I2C0 only on PA1/PA0 */
    if (strcmp(scl, "PA1") != 0 || strcmp(sda, "PA0") != 0) {
        return -1;
    }
    bus->hz = hz > 0 ? hz : 100000;
    bus->open = 1;
    bus->hw = 1;
    i2c0_hw_init(bus->hz);
    return 0;
}

void board_i2c_close(board_i2c_t *bus)
{
    if (bus) {
        if (bus->open && bus->hw) {
            DL_I2C_resetControllerTransfer(I2C0);
            DL_I2C_disableController(I2C0);
            DL_I2C_reset(I2C0);
            (void)board_pin_af("PA1", 0, 0);
            (void)board_pin_af("PA0", 0, 0);
        }
        bus->open = 0;
        bus->hw = 0;
    }
}

int board_i2c_write(board_i2c_t *bus, uint8_t addr7, const uint8_t *data, size_t n)
{
    size_t sent = 0;
    if (!bus || !bus->open || !bus->hw) {
        return -1;
    }
    if (wait_idle(20) != 0) {
        return i2c0_fail(bus);
    }
    if (n == 0) {
        DL_I2C_startControllerTransfer(I2C0, addr7, DL_I2C_CONTROLLER_DIRECTION_TX, 0);
        return wait_not_busy(50) == 0 ? 0 : i2c0_fail(bus);
    }
    while (sent < n) {
        size_t chunk = n - sent;
        if (chunk > 8) {
            chunk = 8;
        }
        DL_I2C_fillControllerTXFIFO(I2C0, (uint8_t *)&data[sent], (uint16_t)chunk);
        DL_I2C_startControllerTransfer(I2C0, addr7, DL_I2C_CONTROLLER_DIRECTION_TX,
            (uint16_t)chunk);
        delay_cycles(200);
        if (wait_not_busy(50) != 0) {
            return i2c0_fail(bus);
        }
        if (DL_I2C_getControllerStatus(I2C0) & DL_I2C_CONTROLLER_STATUS_ERROR) {
            return i2c0_fail(bus);
        }
        sent += chunk;
    }
    return 0;
}

int board_i2c_read(board_i2c_t *bus, uint8_t addr7, uint8_t *data, size_t n)
{
    size_t got = 0;
    if (!bus || !bus->open || !bus->hw || !data) {
        return -1;
    }
    if (wait_idle(20) != 0) {
        return i2c0_fail(bus);
    }
    while (got < n) {
        size_t chunk = n - got;
        if (chunk > 8) {
            chunk = 8;
        }
        DL_I2C_startControllerTransfer(I2C0, addr7, DL_I2C_CONTROLLER_DIRECTION_RX,
            (uint16_t)chunk);
        for (size_t i = 0; i < chunk; i++) {
            uint32_t t0 = board_millis();
            while (DL_I2C_isControllerRXFIFOEmpty(I2C0)) {
                if ((uint32_t)(board_millis() - t0) > 50) {
                    return i2c0_fail(bus);
                }
            }
            data[got + i] = DL_I2C_receiveControllerData(I2C0);
        }
        if (wait_not_busy(50) != 0) {
            return i2c0_fail(bus);
        }
        got += chunk;
    }
    return 0;
}

int board_i2c_write_read(board_i2c_t *bus, uint8_t addr7,
    const uint8_t *w, size_t wn, uint8_t *r, size_t rn)
{
    if (board_i2c_write(bus, addr7, w, wn) != 0) {
        return -1;
    }
    return board_i2c_read(bus, addr7, r, rn);
}
