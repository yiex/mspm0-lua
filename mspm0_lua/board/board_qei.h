#ifndef BOARD_QEI_H
#define BOARD_QEI_H

#include <stdint.h>

/*
 * Hardware QEI (TIMG8, 2-input): PHA=PA26, PHB=PA27.
 * X4 counts in 16-bit timer; pos is extended int32 with wrap tracking.
 */
int board_qei_open(void); /* fixed PA26/PA27; → 0 ok */
void board_qei_close(void);
void board_qei_set(int32_t pos);
int32_t board_qei_pos(void);
int32_t board_qei_delta(void);
int board_qei_dir(void); /* +1 up, -1 down, 0 unknown/idle */
int board_qei_active(void);

/* Soft quadrature stim on PA14/PA25 (wire to PA26/PA27). steps = edge pairs. */
int board_qgen_run(int steps, int dir, uint32_t half_us);

#endif
