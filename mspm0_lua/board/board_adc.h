#ifndef BOARD_ADC_H
#define BOARD_ADC_H

#include <stddef.h>
#include <stdint.h>

/* ADC12 single-shot and high-speed DMA burst (ADC0 + DMA CH0). */

#define BOARD_ADC_DMA_MAX 256

void board_adc_init(void);
void board_adc_close(void);
int board_adc_read(uint8_t ch);

/* Resolve header pin name to ch 0..7 (claims as ADC analog, PF=0). Or -1. */
int board_adc_claim_pin(const char *name);

/*
 * Burst capture: n samples (1..BOARD_ADC_DMA_MAX) into 12-bit codes.
 * ch: ADC input channel 0..7. timeout_ms: 0 => default 200.
 * rate: 0=fast (~2 MSPS), 1=normal (~96 kSPS), 2=slow (~7.8 kSPS).
 * Returns sample count, or -1 on error.
 */
int board_adc_capture(uint8_t ch, uint16_t *buf, size_t n,
    uint32_t timeout_ms, uint8_t rate);

/* After capture: sample period estimate in ns (0 if unknown). */
uint32_t board_adc_capture_period_ns(void);

#endif
