#ifndef BOARD_UART_APP_H
#define BOARD_UART_APP_H

#include <stddef.h>
#include <stdint.h>
#include <ti/devices/msp/msp.h>

typedef struct {
    UART_Regs *regs;
    uint8_t open;
    uint8_t id; /* 1..3 */
} board_uart_app_t;

int board_uart_app_open(board_uart_app_t *port, unsigned id,
    const char *tx, const char *rx, uint32_t baud);
void board_uart_app_close(board_uart_app_t *port);
int board_uart_app_write(board_uart_app_t *port, const uint8_t *data, size_t n);
/* IRQ-backed ring + short poll; timeout_ms waits for first byte only. */
size_t board_uart_app_read(board_uart_app_t *port, uint8_t *data, size_t n,
    uint32_t timeout_ms);
/* Bytes currently queued in RX ring (id 1..3). */
size_t board_uart_app_rx_avail(unsigned id);
/* Drain HW RX FIFOs into rings (safe from main while I2C busy-waits). */
void board_uart_app_poll(void);
uint32_t board_uart_app_drops(unsigned id);

#endif
