#ifndef BOARD_CAN_H
#define BOARD_CAN_H

#include <stddef.h>
#include <stdint.h>

int board_can_open(uint32_t bitrate, int loopback);
void board_can_close(void);
int board_can_send(uint16_t id, const uint8_t *data, size_t n,
    uint32_t timeout_ms);
int board_can_recv(uint16_t *id, uint8_t *data, size_t *n,
    uint32_t timeout_ms);

#endif
