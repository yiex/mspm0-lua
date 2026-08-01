#ifndef BOARD_RAMP_H
#define BOARD_RAMP_H

#include <stdint.h>

/* Soft setpoint ramps for motor/PWM (C, max 4). */
#define BOARD_RAMP_MAX 4

int board_ramp_open(void);
void board_ramp_close(int id);
void board_ramp_config(int id, int32_t rate); /* max |Δ| per step call */
void board_ramp_set(int id, int32_t target);
void board_ramp_jump(int id, int32_t value); /* set current=target=value */
int32_t board_ramp_step(int id);             /* advance one step → current */
int32_t board_ramp_get(int id);
int board_ramp_done(int id); /* 1 if current==target */

#endif
