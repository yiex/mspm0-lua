#ifndef BOARD_RESOURCE_H
#define BOARD_RESOURCE_H

#include <stdint.h>

typedef enum {
    BOARD_RES_TIMG0 = 0,
    BOARD_RES_TIMG6,
    BOARD_RES_TIMG7,
    BOARD_RES_TIMG8,
    BOARD_RES_TIMG12,
    BOARD_RES_TIMA0,
    BOARD_RES_TIMA1,
    BOARD_RES_ADC0,
    BOARD_RES_DMA0,
    BOARD_RES_I2C0,
    BOARD_RES_I2C1,
    BOARD_RES_SPI0,
    BOARD_RES_UART1,
    BOARD_RES_UART2,
    BOARD_RES_UART3,
    BOARD_RES_WWDT1,
    BOARD_RES_CAN,
    BOARD_RES_DAC0,
    BOARD_RES_COMP0,
    BOARD_RES_COMP1,
    BOARD_RES_COMP2,
    BOARD_RES_RTC,
    BOARD_RES_OPA0,
    BOARD_RES_OPA1,
    BOARD_RES_ADC1,
    BOARD_RES_COUNT
} board_resource_t;

void board_resource_init(void);
int board_resource_claim(board_resource_t resource, uint8_t owner);
void board_resource_release(board_resource_t resource, uint8_t owner);
void board_resource_release_owner(uint8_t owner);
void board_resource_reset_app(void);
uint8_t board_resource_owner(board_resource_t resource);

#endif
