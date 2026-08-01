#ifndef BOARD_STATUS_H
#define BOARD_STATUS_H

#include <stdint.h>

/*
 * Status mailbox in .noinit (low SRAM), NOT near stack top.
 * Stack grows down from 0x20208000; addresses like 0x20206000 get smashed.
 */
extern volatile uint32_t g_board_status[4];

#define BOARD_STATUS_MAGIC 0x4C554131u /* 'LUA1' */

#define ST_BOOT 0x00000001u
#define ST_UART_OK 0x00000002u
#define ST_LUA_OK 0x00000004u
#define ST_LUA_RUN 0x00000008u
#define ST_SPI_OK 0x00000010u
#define ST_SPI_FAIL 0x00000020u
#define ST_DEMO_DONE 0x00000040u
#define ST_LFS_OK 0x00000080u
#define ST_SCRIPT_EXT 0x00000100u
#define ST_UART_LB_OK 0x00000200u
#define ST_UART_LB_FAIL 0x00000400u
#define ST_UART_RX_HIT 0x00000800u
#define ST_PUTS_OK 0x00001000u
#define ST_POST_LED 0x00002000u
#define ST_HFXT_OK 0x00004000u
#define ST_HFXT_FAIL 0x00008000u
#define ST_NATIVE_MODULE_OK 0x00010000u

static inline void board_status_set(uint32_t flags)
{
    g_board_status[0] = BOARD_STATUS_MAGIC;
    g_board_status[1] = flags;
    g_board_status[2] = 0;
    g_board_status[3] = 0;
}

static inline void board_status_or(uint32_t flags)
{
    g_board_status[0] = BOARD_STATUS_MAGIC;
    g_board_status[1] |= flags;
}

static inline void board_status_set_jedec(uint32_t jedec)
{
    g_board_status[0] = BOARD_STATUS_MAGIC;
    g_board_status[2] = jedec;
}

static inline void board_status_set_raw(uint32_t raw)
{
    g_board_status[3] = raw;
}

static inline void board_status_or_raw(uint32_t raw)
{
    g_board_status[3] |= raw;
}

#endif
