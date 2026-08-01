#ifndef BOARD_SPI_H
#define BOARD_SPI_H

#include <stddef.h>
#include <stdint.h>
#include <stdbool.h>

/* App SPI on hardware SPI1 (shared with W25Q; uses board_spi1_lock). */

typedef struct {
    uint32_t cs_iomux;
    void *cs_port;
    uint32_t cs_pin;
    int hz;
    int open;
    int hw;
} board_spi_t;

int board_spi_open(board_spi_t *bus,
    const char *sck, const char *mosi, const char *miso, const char *cs, int hz);
void board_spi_close(board_spi_t *bus);
void board_spi_cs(board_spi_t *bus, bool select);
int board_spi_xfer(board_spi_t *bus, const uint8_t *tx, uint8_t *rx, size_t n);

#endif
