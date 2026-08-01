#ifndef BOARD_OLED_H
#define BOARD_OLED_H

#include <stddef.h>
#include <stdint.h>

/*
 * SSD1306 128x64 on I2C1.
 * - Built-in 6x8: ASCII 32..90 + a-z→A-Z (oled.print / oled.num)
 * - Optional 6x8 bank: oled.glyph / oled.font → _run.fnt (F6)
 * - Optional 16x16 bank: oled.text / oled.font16 → _run.f16 (FN)
 *   BMP: 16 rows × 2 bytes, MSB = left pixel (row-major).
 */
int board_oled_open(const char *scl, const char *sda, uint8_t addr7, uint32_t hz);
void board_oled_close(void);
int board_oled_ready(void);

int board_oled_clear(void);
/* Fill GDDRAM with byte v (0x00 clear, 0xFF solid white). */
int board_oled_fill(uint8_t v);
int board_oled_cursor(uint8_t x, uint8_t page);
int board_oled_putc(char c);
int board_oled_puts(const char *s);
int board_oled_num(uint8_t x, uint8_t page, int32_t value, uint8_t dec);

int board_oled_glyph_set(uint8_t code, const uint8_t data6[6]);
void board_oled_glyph_clear(void);
int board_oled_font_load(const char *path);
int board_oled_has_glyph(uint8_t code);

/* 16x16 dynamic bank (Unicode BMP codepoint). */
#define BOARD_OLED_CJK_MAX 16
#define BOARD_OLED_CJK_BYTES 32

int board_oled_cjk_set(uint16_t code, const uint8_t bmp32[32]);
void board_oled_cjk_clear(void);
/* Load FN pack from LittleFS. Returns count or <0. Default path _run.f16 */
int board_oled_font16_load(const char *path);
int board_oled_has_cjk(uint16_t code);
/*
 * Draw UTF-8 string with 16x16 glyphs (2 pages tall).
 * x: 0..112, page: top page 0..6. Advances by 16 px per glyph.
 * Missing glyph → hollow box. Returns 0 or -1.
 */
int board_oled_text(uint8_t x, uint8_t page, const char *utf8);

/*
 * Draw scope trace on pages 0..6 (56 px tall). n samples of 12-bit codes.
 * Fixed 0..4095 scale, robust rising trigger and isolated-spike rejection.
 * Page 7 is reserved for frequency.
 */
int board_oled_wave(const uint16_t *s, size_t n);

#endif
