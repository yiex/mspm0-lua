#ifndef BOARD_DMA_H
#define BOARD_DMA_H

#include <stddef.h>
#include <stdint.h>
#include <stdbool.h>

/* SPI1 bus helpers (poll path; DMA removed to shrink flash / simplify). */

void board_dma_init(void);

/* Exclusive bus lock for SPI1 (flash + app). Returns 0 if acquired. */
int board_spi1_lock(uint32_t timeout_ms);
void board_spi1_unlock(void);

/* Temporarily lend SPI1 to a module and restore the LittleFS flash bus. */
int board_spi1_app_acquire(uint32_t timeout_ms);
void board_spi1_app_release(void);

/*
 * Full-duplex transfer (poll). NULL tx => 0xFF fill; NULL rx => discard.
 * Kept name for call-site compatibility.
 */
int board_spi1_xfer_dma(const uint8_t *tx, uint8_t *rx, size_t n, uint32_t timeout_ms);

uint8_t board_spi1_xfer_byte(uint8_t v);

void board_spi1_hw_ensure(void);
void board_spi1_set_hz(uint32_t hz);

#endif
