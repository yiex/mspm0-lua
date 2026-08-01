#ifndef BOARD_IRQ_H
#define BOARD_IRQ_H

#include <stdint.h>
#include <stdbool.h>

#define BOARD_SOFT_TIMER_MAX 4
#define BOARD_GPIO_IRQ_MAX 4

/*
 * Interrupt layer:
 * - ISR only updates counters / soft-timer pending (never runs Lua)
 * - 1 ms tick + soft timers: Cortex-M0+ SysTick
 * - GPIO edges: GROUP1 (GPIOA/GPIOB)
 */

void board_irq_init(void);

uint32_t board_millis(void);

/* ISR-driven soft timers; expirations accumulate until take(). */
int board_soft_timer_start(unsigned id, uint32_t period_ms);
void board_soft_timer_stop(unsigned id);
uint32_t board_soft_timer_take(unsigned id);

void board_delay_ms_irq(uint32_t ms);

/* edge: 0=fall, 1=rise, 2=both. Returns 0 or -1. */
int board_gpio_irq_enable(const char *pin_name, int edge);
int board_gpio_irq_set_debounce(const char *pin_name, uint32_t debounce_ms);
int board_gpio_irq_disable(const char *pin_name);
uint32_t board_gpio_irq_count(const char *pin_name); /* read-clear */
void board_gpio_irq_reset(void);

#endif
