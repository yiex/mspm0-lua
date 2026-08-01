/* Temporary UART-only probe firmware: polled RX echo on PA10/PA11 @ 115200. */
#include "board_delay.h"
#include "board_status.h"
#include "board_uart.h"
#include "ti_msp_dl_config.h"

int main(void)
{
    board_status_set(ST_BOOT);
    SYSCFG_DL_init();
    board_uart_init();
    board_status_or(ST_UART_OK);

    if (board_uart_loopback_ok()) {
        board_status_or(ST_UART_LB_OK);
    } else {
        board_status_or(ST_UART_LB_FAIL);
    }

    board_uart_puts("\n\nUARTONLY RX READY 115200 PA10/PA11\n");
    board_status_or(ST_PUTS_OK);

    for (uint32_t idle = 0;; idle++) {
        int c = board_uart_getc_nonblock();
        if (c >= 0) {
            board_uart_puts("RX ");
            board_uart_putc((char)c);
            board_uart_puts("\n");
            idle = 0;
        }
        if ((idle & 0x1FFFFu) == 0u) {
            DL_GPIO_togglePins(GPIO_LEDS_PORT, GPIO_LEDS_USER_LED_PIN);
        }
        board_status_or(ST_POST_LED);
    }
}
