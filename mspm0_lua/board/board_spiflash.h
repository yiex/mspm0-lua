#ifndef BOARD_SPIFLASH_H
#define BOARD_SPIFLASH_H

#include <stddef.h>
#include <stdint.h>
#include <stdbool.h>

/* W25Q32: 4MB, 4KB sector, 256B page */
#define SPIFLASH_TOTAL_SIZE (4u * 1024u * 1024u)
#define SPIFLASH_SECTOR_SIZE 4096u
#define SPIFLASH_PAGE_SIZE 256u

void board_spiflash_init(void);
void board_spiflash_cs(bool select);
uint8_t board_spiflash_xfer(uint8_t v);
bool board_spiflash_read_jedec(uint8_t id[3]);
uint32_t board_spiflash_capacity_bytes(void);
bool board_spiflash_read(uint32_t addr, uint8_t *buf, size_t len);
bool board_spiflash_write_enable(void);
bool board_spiflash_wait_ready(uint32_t timeout_ms);
bool board_spiflash_erase_sector(uint32_t addr);
bool board_spiflash_program_page(uint32_t addr, const uint8_t *buf, size_t len);
bool board_spiflash_program(uint32_t addr, const uint8_t *buf, size_t len);

#endif
