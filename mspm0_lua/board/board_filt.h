#ifndef BOARD_FILT_H
#define BOARD_FILT_H

#include <stdint.h>

/* Lightweight filters for ADC/control (C hot path). */
#define BOARD_FILT_MAX 4
#define BOARD_FILT_MA_MAX 16

enum {
    BOARD_FILT_LP = 0, /* 1st-order IIR: y += a*(x-y), a in 1..256 (256≈1.0) */
    BOARD_FILT_MA = 1, /* moving average, window 2..BOARD_FILT_MA_MAX */
};

int board_filt_open(int kind);
void board_filt_close(int id);
void board_filt_reset(int id);

/* LP: alpha 1..256 (y += alpha*(x-y)/256). MA: window 2..16. */
void board_filt_config(int id, int param);
int32_t board_filt_update(int id, int32_t x);
int32_t board_filt_get(int id); /* last y; 0 if unused */

#endif
