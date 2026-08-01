#ifndef BOARD_PWM_H
#define BOARD_PWM_H

#include <stdint.h>

/*
 * Independent PWM: up to 2 concurrent (TIMG12 id=0 family, TIMG7 id=1 family).
 * open returns channel id 0 or 1. freq 100..50000, duty 0..100.
 */
int board_pwm_open(const char *pin, uint32_t freq_hz); /* → id or <0 */
void board_pwm_set_duty(int id, uint8_t duty);
void board_pwm_close(int id);
void board_pwm_close_all(void);

/*
 * Complementary PWM with dead-band: TIMA0 and/or TIMA1 (2 concurrent max).
 * open returns comp id 0 or 1. freq 100..100000; duty 0..100; dead_ns clamped.
 */
int board_pwm_comp_open(const char *hi, const char *lo,
    uint32_t freq_hz, uint8_t duty_pct, uint32_t dead_ns);
void board_pwm_comp_set_duty(int id, uint8_t duty_pct);
void board_pwm_comp_close(int id);
void board_pwm_comp_close_all(void);

#endif
