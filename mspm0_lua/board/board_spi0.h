#ifndef BOARD_SPI0_H
#define BOARD_SPI0_H

#include <stddef.h>
#include <stdint.h>
#include <stdbool.h>
#include <ti/devices/msp/msp.h>

typedef struct {
    GPIO_Regs *cs_port;
    uint32_t cs_pin;
    uint8_t open;
} board_spi0_t;

int board_spi0_open(board_spi0_t *bus, const char *sck, const char *pico,
    const char *poci, const char *cs, uint32_t hz);
void board_spi0_close(board_spi0_t *bus);
void board_spi0_cs(board_spi0_t *bus, bool select);
int board_spi0_xfer(board_spi0_t *bus, const uint8_t *tx, uint8_t *rx, size_t n);

#endif
