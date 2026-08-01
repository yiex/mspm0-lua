#include "board_spi.h"
#include "board_dma.h"
#include "board_pins.h"
#include "board_reg.h"
#include "ti_msp_dl_config.h"

/*
 * App SPI: hardware SPI1 on flash pins (PB16/15/14 + CS).
 * Shares bus with W25Q via board_spi1_lock.
 */

int board_spi_open(board_spi_t *bus,
    const char *sck, const char *mosi, const char *miso, const char *cs, int hz)
{
    board_pin_t pcs;
    if (!bus) {
        return -1;
    }
    (void)sck;
    (void)mosi;
    (void)miso;
    /* PB17 is reserved for the W25Q; default app chip-select is PA18. */
    if (board_pin_resolve(cs ? cs : "PA18", &pcs) != 0) {
        return -1;
    }
    bus->cs_port = pcs.port;
    bus->cs_pin = pcs.pin;
    bus->cs_iomux = pcs.iomux;
    bus->hz = hz > 0 ? hz : 1000000;
    bus->open = 1;
    bus->hw = 1;
    board_spi1_hw_ensure();
    board_spi1_set_hz((uint32_t)bus->hz);
    board_reg_pin_out(pcs.port, pcs.pin, pcs.iomux);
    board_reg_gpio_set(pcs.port, pcs.pin);
    return 0;
}

void board_spi_close(board_spi_t *bus)
{
    if (bus) {
        bus->open = 0;
    }
}

void board_spi_cs(board_spi_t *bus, bool select)
{
    if (!bus || !bus->open) {
        return;
    }
    if (select) {
        board_reg_gpio_clr((GPIO_Regs *)bus->cs_port, bus->cs_pin);
    } else {
        board_reg_gpio_set((GPIO_Regs *)bus->cs_port, bus->cs_pin);
    }
}

int board_spi_xfer(board_spi_t *bus, const uint8_t *tx, uint8_t *rx, size_t n)
{
    int st;
    if (!bus || !bus->open) {
        return -1;
    }
    if (board_spi1_lock(100) != 0) {
        return -1;
    }
    board_spi1_set_hz((uint32_t)bus->hz);
    if (n >= 16) {
        st = board_spi1_xfer_dma(tx, rx, n, 200);
    } else {
        st = 0;
        for (size_t i = 0; i < n; i++) {
            uint8_t r = board_spi1_xfer_byte(tx ? tx[i] : 0xFFu);
            if (rx) {
                rx[i] = r;
            }
        }
        st = (int)n;
    }
    board_spi1_unlock();
    /* restore flash-friendly rate */
    board_spi1_set_hz(1000000u);
    return st < 0 ? -1 : (int)n;
}
