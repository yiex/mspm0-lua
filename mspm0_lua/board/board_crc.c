#include "board_crc.h"

uint8_t board_crc8(const uint8_t *data, size_t n, uint8_t init)
{
    uint8_t crc = init;
    size_t i;
    int b;
    if (!data) {
        return crc;
    }
    for (i = 0; i < n; i++) {
        crc ^= data[i];
        for (b = 0; b < 8; b++) {
            if (crc & 0x80u) {
                crc = (uint8_t)((crc << 1) ^ 0x07u);
            } else {
                crc <<= 1;
            }
        }
    }
    return crc;
}

uint16_t board_crc16_modbus(const uint8_t *data, size_t n)
{
    uint16_t crc = 0xFFFFu;
    size_t i;
    int b;
    if (!data) {
        return crc;
    }
    for (i = 0; i < n; i++) {
        crc ^= data[i];
        for (b = 0; b < 8; b++) {
            if (crc & 1u) {
                crc = (uint16_t)((crc >> 1) ^ 0xA001u);
            } else {
                crc >>= 1;
            }
        }
    }
    return crc;
}
