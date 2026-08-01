#include "board_dma.h"
#include "board_delay.h"
#include "board_irq.h"
#include "ti_msp_dl_config.h"

static volatile uint8_t s_spi1_locked;
static uint8_t s_spi1_ready;

void board_spi1_hw_ensure(void)
{
    if (s_spi1_ready) {
        return;
    }
    SYSCFG_DL_SPI_FLASH_init();
    DL_SPI_setFIFOThreshold(SPI_FLASH_INST, DL_SPI_RX_FIFO_LEVEL_ONE_FRAME,
        DL_SPI_TX_FIFO_LEVEL_ONE_FRAME);
    s_spi1_ready = 1;
}

void board_spi1_set_hz(uint32_t hz)
{
    uint32_t bus = g_uart_busclk_hz ? g_uart_busclk_hz : 32000000u;
    uint32_t scr;
    board_spi1_hw_ensure();
    if (hz < 1000u) {
        hz = 1000u;
    }
    scr = bus / (2u * hz);
    if (scr > 0u) {
        scr -= 1u;
    }
    if (scr > 1023u) {
        scr = 1023u;
    }
    DL_SPI_setBitRateSerialClockDivider(SPI_FLASH_INST, scr);
}

void board_dma_init(void)
{
    s_spi1_locked = 0;
    s_spi1_ready = 0;
    board_spi1_hw_ensure();
}

int board_spi1_lock(uint32_t timeout_ms)
{
    uint32_t t0 = board_millis();
    while (s_spi1_locked) {
        if ((uint32_t)(board_millis() - t0) > timeout_ms) {
            return -1;
        }
    }
    s_spi1_locked = 1;
    return 0;
}

void board_spi1_unlock(void)
{
    s_spi1_locked = 0;
}

int board_spi1_app_acquire(uint32_t timeout_ms)
{
    if (board_spi1_lock(timeout_ms) != 0) {
        return -1;
    }
    s_spi1_ready = 0;
    return 0;
}

void board_spi1_app_release(void)
{
    s_spi1_ready = 0;
    board_spi1_hw_ensure();
    board_spi1_unlock();
}

uint8_t board_spi1_xfer_byte(uint8_t v)
{
    uint32_t spins = 200000u;
    board_spi1_hw_ensure();
    while (DL_SPI_isTXFIFOFull(SPI_FLASH_INST)) {
        if (spins-- == 0u) {
            return 0xFFu;
        }
    }
    DL_SPI_transmitData8(SPI_FLASH_INST, v);
    spins = 200000u;
    while (DL_SPI_isRXFIFOEmpty(SPI_FLASH_INST)) {
        if (spins-- == 0u) {
            return 0xFFu;
        }
    }
    return DL_SPI_receiveData8(SPI_FLASH_INST);
}

int board_spi1_xfer_dma(const uint8_t *tx, uint8_t *rx, size_t n, uint32_t timeout_ms)
{
    size_t i;
    (void)timeout_ms;
    if (n == 0u) {
        return 0;
    }
    board_spi1_hw_ensure();
    while (!DL_SPI_isRXFIFOEmpty(SPI_FLASH_INST)) {
        (void)DL_SPI_receiveData8(SPI_FLASH_INST);
    }
    for (i = 0; i < n; i++) {
        uint8_t r = board_spi1_xfer_byte(tx ? tx[i] : 0xFFu);
        if (rx) {
            rx[i] = r;
        }
    }
    return (int)n;
}
