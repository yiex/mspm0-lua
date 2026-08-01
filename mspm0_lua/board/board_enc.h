#ifndef BOARD_ENC_H
#define BOARD_ENC_H

#include <stdint.h>

/*
 * Software quadrature decoder (X4). Uses two GPIO IRQ slots (both edges).
 * Pins: active with internal pull-up; wire encoder outputs + GND.
 */
#define BOARD_ENC_MAX 2

int board_enc_open(const char *pin_a, const char *pin_b);
void board_enc_close(int id);
void board_enc_set(int id, int32_t pos);
int32_t board_enc_pos(int id);
/* Counts since last call (signed, X4 edges); also advances internal mark. */
int32_t board_enc_delta(int id);
/*
 * Rate in counts per second over last interval:
 * call periodically; uses mark + board_millis. First call after open/set → 0.
 */
int32_t board_enc_cps(int id);
/* Called from GROUP1 GPIO ISR — do not call from Lua. */
void board_enc_isr_tick(void);
/* Optional main-context sample (same decode as ISR). */
void board_enc_poll(void);

#endif
