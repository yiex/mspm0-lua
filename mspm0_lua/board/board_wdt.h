#ifndef BOARD_WDT_H
#define BOARD_WDT_H

#include <stdint.h>

/*
 * WWDT1 window watchdog (LFCLK). Once armed, must feed in the open window
 * or device SYSRST. Cannot stop after start (hardware).
 *
 * period_ms is quantized to WWDT bit-period buckets (~2..1000 ms typical).
 */
int board_wdt_start(uint32_t period_ms); /* 0 = ~500 ms default; → 0 ok, -1 fail */
void board_wdt_feed(void);
int board_wdt_active(void);

#endif
