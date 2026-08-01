#ifndef BOARD_I2C_H
#define BOARD_I2C_H

#include <stddef.h>
#include <stdint.h>
#include <stdbool.h>

/* Hardware I2C0 master: PA0=SDA, PA1=SCL. */

typedef struct {
    int hz;
    int open;
    int hw;
} board_i2c_t;

int board_i2c_open(board_i2c_t *bus, const char *scl, const char *sda, int hz);
void board_i2c_close(board_i2c_t *bus);

int board_i2c_write(board_i2c_t *bus, uint8_t addr7, const uint8_t *data, size_t n);
int board_i2c_read(board_i2c_t *bus, uint8_t addr7, uint8_t *data, size_t n);
int board_i2c_write_read(board_i2c_t *bus, uint8_t addr7,
    const uint8_t *w, size_t wn, uint8_t *r, size_t rn);

#endif
