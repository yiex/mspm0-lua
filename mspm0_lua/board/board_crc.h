#ifndef BOARD_CRC_H
#define BOARD_CRC_H

#include <stdint.h>
#include <stddef.h>

/* Compact software CRC for contest protocols (no hardware CRC peripheral). */
uint8_t board_crc8(const uint8_t *data, size_t n, uint8_t init);
/* CRC-16/MODBUS poly 0xA001, init 0xFFFF */
uint16_t board_crc16_modbus(const uint8_t *data, size_t n);

#endif
