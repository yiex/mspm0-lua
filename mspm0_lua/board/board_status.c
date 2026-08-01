#include "board_status.h"

/*
 * Fixed low-SRAM address (after vtable region), never near stack top.
 * Section placed by ld script absolute address.
 */
volatile uint32_t g_board_status[4]
    __attribute__((section(".status_mbox"), used));
