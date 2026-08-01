#ifndef BOARD_UART_H
#define BOARD_UART_H

#include <stddef.h>
#include <stdint.h>

void board_uart_init(void);
/* Re-arm pins/IRQ without wiping RX ring (safe after Lua). */
void board_uart_rearm(void);
int board_uart_set_baud(uint32_t baud);
uint32_t board_uart_get_baud(void);
void board_uart_write(const char *s, size_t n);
void board_uart_putc(char c);
int board_uart_getc_nonblock(void);
/* Next RX byte without consume; <0 if empty. */
int board_uart_peek_nonblock(void);
void board_uart_puts(const char *s);
int board_uart_loopback_ok(void);
int board_uart0_app_acquire(void);
void board_uart0_app_release(void);

#endif
