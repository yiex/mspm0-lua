#ifndef BOARD_UTIL_H
#define BOARD_UTIL_H

#include <stdint.h>

/* Small C helpers for contest control loops (no slots). */

int32_t board_clamp(int32_t v, int32_t lo, int32_t hi);
int32_t board_deadzone(int32_t v, int32_t dz); /* |v|<dz → 0 */
int32_t board_map(int32_t x, int32_t in_lo, int32_t in_hi,
    int32_t out_lo, int32_t out_hi);
int32_t board_med3(int32_t a, int32_t b, int32_t c);
/* Average n samples from ptr; n 1..32. */
int32_t board_avg_n(const int32_t *x, int n);
/* Rate limit: step cur toward tgt by at most rate. */
int32_t board_slew(int32_t cur, int32_t tgt, int32_t rate);
int32_t board_sign(int32_t v); /* -1/0/+1 */

#endif
