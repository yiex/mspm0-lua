#ifndef BOARD_LED_H
#define BOARD_LED_H

#include <stdint.h>

/* Board user LED PA14. C owns pinmux; Lua only calls high-level ops. */
int board_led_on(void);
int board_led_off(void);
int board_led_toggle(void);
/* 0..100 duty via TIMG12; 0 releases PWM and drives GPIO low. */
int board_led_pwm(uint8_t duty);

#endif
