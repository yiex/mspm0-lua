#include "board_spi0.h"

#include <string.h>
#include <ti/driverlib/driverlib.h>

#include "board_pins.h"
#include "board_reg.h"
#include "ti_msp_dl_config.h"

int board_spi0_open(board_spi0_t *bus, const char *sck, const char *pico,
    const char *poci, const char *cs, uint32_t hz)
{
    DL_SPI_Config cfg = {
        .mode = DL_SPI_MODE_CONTROLLER,
        .frameFormat = DL_SPI_FRAME_FORMAT_MOTO4_POL0_PHA0,
        .parity = DL_SPI_PARITY_NONE,
        .dataSize = DL_SPI_DATA_SIZE_8,
        .bitOrder = DL_SPI_BIT_ORDER_MSB_FIRST,
        .chipSelectPin = DL_SPI_CHIP_SELECT_NONE,
    };
    DL_SPI_ClockConfig clk = {
        .clockSel = DL_SPI_CLOCK_BUSCLK,
        .divideRatio = DL_SPI_CLOCK_DIVIDE_RATIO_1,
    };
    board_pin_t pcs;
    uint32_t div;
    if (!bus || strcmp(sck, "PA12") || strcmp(pico, "PA14") ||
            strcmp(poci, "PA13") || hz < 1000u || hz > 20000000u) return -1;
    if (board_pin_resolve(cs, &pcs) != 0 || !strcmp(cs, "PA10") || !strcmp(cs, "PA11") ||
            !strcmp(cs, "PA19") || !strcmp(cs, "PA20") ||
            !strcmp(cs, "PB14") || !strcmp(cs, "PB15") ||
            !strcmp(cs, "PB16") || !strcmp(cs, "PB17")) return -1;
    bus->cs_port = pcs.port;
    bus->cs_pin = pcs.pin;
    if (board_pin_af(sck, 3, 0) || board_pin_af(pico, 3, 0) ||
            board_pin_af(poci, 3, 1)) return -1;
    board_reg_pin_out(pcs.port, pcs.pin, pcs.iomux);
    board_reg_gpio_set(pcs.port, pcs.pin);
    DL_SPI_reset(SPI0);
    DL_SPI_enablePower(SPI0);
    delay_cycles(POWER_STARTUP_DELAY);
    DL_SPI_setClockConfig(SPI0, &clk);
    DL_SPI_init(SPI0, &cfg);
    div = (g_uart_busclk_hz + hz - 1u) / hz;
    if (div < 1u) div = 1u;
    if (div > 1024u) div = 1024u;
    DL_SPI_setBitRateSerialClockDivider(SPI0, div - 1u);
    DL_SPI_enable(SPI0);
    bus->open = 1;
    return 0;
}

void board_spi0_close(board_spi0_t *bus)
{
    if (bus && bus->open) {
        DL_SPI_disable(SPI0);
        board_spi0_cs(bus, false);
        bus->open = 0;
    }
}

void board_spi0_cs(board_spi0_t *bus, bool select)
{
    if (!bus || !bus->open) return;
    if (select) board_reg_gpio_clr(bus->cs_port, bus->cs_pin);
    else board_reg_gpio_set(bus->cs_port, bus->cs_pin);
}

int board_spi0_xfer(board_spi0_t *bus, const uint8_t *tx, uint8_t *rx, size_t n)
{
    if (!bus || !bus->open) return -1;
    for (size_t i = 0; i < n; i++) {
        DL_SPI_transmitDataBlocking8(SPI0, tx ? tx[i] : 0xFFu);
        if (rx) rx[i] = DL_SPI_receiveDataBlocking8(SPI0);
        else (void)DL_SPI_receiveDataBlocking8(SPI0);
    }
    return (int)n;
}
