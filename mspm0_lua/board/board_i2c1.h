#ifndef BOARD_I2C1_H
#define BOARD_I2C1_H

#include <stddef.h>
#include <stdint.h>

typedef struct {
    uint32_t generation;
    uint8_t open;
} board_i2c1_t;

int board_i2c1_open(board_i2c1_t *bus, const char *scl, const char *sda,
    uint32_t hz);
void board_i2c1_close(board_i2c1_t *bus);
int board_i2c1_ready(const board_i2c1_t *bus);
int board_i2c1_write(board_i2c1_t *bus, uint8_t addr7,
    const uint8_t *data, size_t n);
int board_i2c1_read(board_i2c1_t *bus, uint8_t addr7, uint8_t *data, size_t n);
int board_i2c1_write_read(board_i2c1_t *bus, uint8_t addr7,
    const uint8_t *w, size_t wn, uint8_t *r, size_t rn);

#endif
