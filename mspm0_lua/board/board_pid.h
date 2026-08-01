#ifndef BOARD_PID_H
#define BOARD_PID_H

#include <stdint.h>

/*
 * Compact control helpers for Lua orchestration.
 * Gains: Q16.16 (same as iq). Process / output: int32 application units.
 * Modes: positional, incremental. Cascade = outer then inner step.
 */
#define BOARD_PID_MAX 4

enum {
    BOARD_PID_POS = 0,
    BOARD_PID_INC = 1,
};

int board_pid_open(int mode); /* → id 0..MAX-1 or -1 */
void board_pid_close(int id);
void board_pid_reset(int id);

/* kp/ki/kd as IQ16 (1.0 = 65536). ki is per-second; scaled by dt_ms inside. */
void board_pid_tune(int id, int32_t kp, int32_t ki, int32_t kd);
void board_pid_out_limit(int id, int32_t umin, int32_t umax);
/* |integral| cap in same units as error*time (IQ path); 0 = no extra i-limit */
void board_pid_i_limit(int id, int32_t imax_abs);

/*
 * One sample. dt_ms < 1 treated as 1.
 * err = sp - fb for step(); or pass err directly to step_err().
 */
int32_t board_pid_step(int id, int32_t sp, int32_t fb, uint32_t dt_ms);
int32_t board_pid_step_err(int id, int32_t err, uint32_t dt_ms);

/* outer(sp, fb_out) → inner setpoint; inner(sp_in, fb_in) → plant u */
int32_t board_pid_cascade(int outer_id, int inner_id,
    int32_t sp, int32_t fb_out, int32_t fb_in, uint32_t dt_ms);

#endif
