#ifndef BOARD_CAP_H
#define BOARD_CAP_H

#include <stdint.h>

/*
 * Single-channel input capture on TIMG6.
 * CCP0: PA21/PB6/PB2; CCP1: PA22/PB7/PB3. Period between edges.
 */
int board_cap_open(const char *pin, int edge); /* edge 0 rise 1 fall 2 both */
void board_cap_close(void);
/* Last period in timer ticks (busclk / prescale); 0 if none yet. */
uint32_t board_cap_period(void);
/* Approximate Hz ×10 (0 if period 0). */
uint32_t board_cap_hz_x10(void);
int board_cap_ready(void); /* 1 if ≥1 period captured since open */
uint32_t board_cap_hits(void); /* ISR edge count (diag) */

#endif
