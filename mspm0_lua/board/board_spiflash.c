#include "board_spiflash.h"
#include "board_delay.h"
#include "board_dma.h"
#include "board_reg.h"
#include "ti_msp_dl_config.h"

static void cs(bool select)
{
    if (select) {
        board_reg_gpio_clr(GPIO_SPI_FLASH_CS_PORT, GPIO_SPI_FLASH_CS_PIN);
    } else {
        board_reg_gpio_set(GPIO_SPI_FLASH_CS_PORT, GPIO_SPI_FLASH_CS_PIN);
    }
}

void board_spiflash_cs(bool select)
{
    cs(select);
}

uint8_t board_spiflash_xfer(uint8_t v)
{
    return board_spi1_xfer_byte(v);
}

void board_spiflash_init(void)
{
    board_spi1_hw_ensure();
    board_spi1_set_hz(1000000u);
    cs(false);
}

bool board_spiflash_read_jedec(uint8_t id[3])
{
    if (board_spi1_lock(50) != 0) {
        return false;
    }
    cs(true);
    (void)board_spi1_xfer_byte(0x9F);
    id[0] = board_spi1_xfer_byte(0xFF);
    id[1] = board_spi1_xfer_byte(0xFF);
    id[2] = board_spi1_xfer_byte(0xFF);
    cs(false);
    board_spi1_unlock();
    if ((id[0] == 0x00 && id[1] == 0x00 && id[2] == 0x00) ||
        (id[0] == 0xFF && id[1] == 0xFF && id[2] == 0xFF)) {
        return false;
    }
    return true;
}

uint32_t board_spiflash_capacity_bytes(void)
{
    uint8_t id[3];
    if (board_spiflash_read_jedec(id) && id[2] >= 17u && id[2] <= 24u) {
        return (uint32_t)1u << id[2];
    }
    return SPIFLASH_TOTAL_SIZE;
}

bool board_spiflash_read(uint32_t addr, uint8_t *buf, size_t len)
{
    uint8_t cmd[4];
    if (!buf) {
        return false;
    }
    if (board_spi1_lock(100) != 0) {
        return false;
    }
    cmd[0] = 0x03;
    cmd[1] = (uint8_t)(addr >> 16);
    cmd[2] = (uint8_t)(addr >> 8);
    cmd[3] = (uint8_t)addr;
    cs(true);
    for (int i = 0; i < 4; i++) {
        (void)board_spi1_xfer_byte(cmd[i]);
    }
    if (len >= 16) {
        if (board_spi1_xfer_dma(NULL, buf, len, 200) < 0) {
            /* fallback poll */
            for (size_t i = 0; i < len; i++) {
                buf[i] = board_spi1_xfer_byte(0xFF);
            }
        }
    } else {
        for (size_t i = 0; i < len; i++) {
            buf[i] = board_spi1_xfer_byte(0xFF);
        }
    }
    cs(false);
    board_spi1_unlock();
    return true;
}

bool board_spiflash_write_enable(void)
{
    if (board_spi1_lock(50) != 0) {
        return false;
    }
    cs(true);
    (void)board_spi1_xfer_byte(0x06);
    cs(false);
    board_spi1_unlock();
    return true;
}

bool board_spiflash_wait_ready(uint32_t timeout_ms)
{
    while (timeout_ms--) {
        if (board_spi1_lock(20) != 0) {
            return false;
        }
        cs(true);
        (void)board_spi1_xfer_byte(0x05);
        uint8_t sr = board_spi1_xfer_byte(0xFF);
        cs(false);
        board_spi1_unlock();
        if ((sr & 0x01u) == 0) {
            return true;
        }
        board_delay_ms(1);
    }
    return false;
}

bool board_spiflash_erase_sector(uint32_t addr)
{
    addr &= ~(SPIFLASH_SECTOR_SIZE - 1u);
    if (!board_spiflash_write_enable()) {
        return false;
    }
    if (board_spi1_lock(50) != 0) {
        return false;
    }
    cs(true);
    (void)board_spi1_xfer_byte(0x20);
    (void)board_spi1_xfer_byte((uint8_t)(addr >> 16));
    (void)board_spi1_xfer_byte((uint8_t)(addr >> 8));
    (void)board_spi1_xfer_byte((uint8_t)addr);
    cs(false);
    board_spi1_unlock();
    return board_spiflash_wait_ready(500);
}

bool board_spiflash_program_page(uint32_t addr, const uint8_t *buf, size_t len)
{
    if (len == 0 || len > SPIFLASH_PAGE_SIZE || !buf) {
        return false;
    }
    if ((addr & (SPIFLASH_PAGE_SIZE - 1u)) + len > SPIFLASH_PAGE_SIZE) {
        return false;
    }
    if (!board_spiflash_write_enable()) {
        return false;
    }
    if (board_spi1_lock(100) != 0) {
        return false;
    }
    cs(true);
    (void)board_spi1_xfer_byte(0x02);
    (void)board_spi1_xfer_byte((uint8_t)(addr >> 16));
    (void)board_spi1_xfer_byte((uint8_t)(addr >> 8));
    (void)board_spi1_xfer_byte((uint8_t)addr);
    if (len >= 16) {
        if (board_spi1_xfer_dma(buf, NULL, len, 200) < 0) {
            for (size_t i = 0; i < len; i++) {
                (void)board_spi1_xfer_byte(buf[i]);
            }
        }
    } else {
        for (size_t i = 0; i < len; i++) {
            (void)board_spi1_xfer_byte(buf[i]);
        }
    }
    cs(false);
    board_spi1_unlock();
    return board_spiflash_wait_ready(50);
}

bool board_spiflash_program(uint32_t addr, const uint8_t *buf, size_t len)
{
    while (len > 0) {
        uint32_t page_off = addr & (SPIFLASH_PAGE_SIZE - 1u);
        size_t chunk = SPIFLASH_PAGE_SIZE - page_off;
        if (chunk > len) {
            chunk = len;
        }
        if (!board_spiflash_program_page(addr, buf, chunk)) {
            return false;
        }
        addr += (uint32_t)chunk;
        buf += chunk;
        len -= chunk;
    }
    return true;
}
