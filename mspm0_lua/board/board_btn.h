#ifndef BOARD_BTN_H
#define BOARD_BTN_H

#include <stdint.h>

/*
 * Polled buttons with debounce + long-press (active-low, internal pull-up).
 * Call board_btn_scan() periodically (e.g. every 5..20 ms from tmr/task).
 */
#define BOARD_BTN_MAX 4

enum {
    BOARD_BTN_NONE = 0,
    BOARD_BTN_PRESS = 1,
    BOARD_BTN_RELEASE = 2,
    BOARD_BTN_LONG = 3,
};

int board_btn_open(const char *pin, uint32_t debounce_ms, uint32_t long_ms);
void board_btn_close(int id);
/* Scan all; returns event count this call. Events queued per-id. */
int board_btn_scan(void);
/* Pop one event: 0 none, or PRESS/RELEASE/LONG. */
int board_btn_event(int id);
int board_btn_down(int id); /* current level, 1=pressed */
/* ms held while pressed (0 if up); does not consume long event */
uint32_t board_btn_held_ms(int id);

#endif
