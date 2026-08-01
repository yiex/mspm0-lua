#ifndef BOARD_PINS_H
#define BOARD_PINS_H

#include <stdint.h>
#include <ti/devices/msp/msp.h>

#define BOARD_PIN_MASK(n) ((uint32_t)(1u << (n)))

/* Owners for pin conflict tracking (one owner per pin). */
enum {
    PIN_OWN_FREE = 0,
    PIN_OWN_GPIO,
    PIN_OWN_IRQ,
    PIN_OWN_UART0,
    PIN_OWN_UART1,
    PIN_OWN_UART2,
    PIN_OWN_UART3,
    PIN_OWN_I2C0,
    PIN_OWN_I2C1,
    PIN_OWN_SPI0,
    PIN_OWN_SPI1,
    PIN_OWN_PWM,
    PIN_OWN_PWM2,
    PIN_OWN_PWMCOMP,
    PIN_OWN_PWMCOMP2,
    PIN_OWN_ADC,
    PIN_OWN_OLED,
    PIN_OWN_BTN,
    PIN_OWN_ENC,
    PIN_OWN_CAP,
    PIN_OWN_QEI,
    PIN_OWN_FLASH,
    PIN_OWN_SYS,
    PIN_OWN_UART_APP0,
    PIN_OWN_CAN,
    PIN_OWN_DAC,
    PIN_OWN_COMP0,
    PIN_OWN_COMP1,
    PIN_OWN_COMP2,
    PIN_OWN_RTC,
    PIN_OWN_OPA0,
    PIN_OWN_OPA1,
};

/* Policy bits (board schematic / product rules). */
#define PIN_POL_SYS      (1u << 0) /* SWD / crystal / ROSC — refuse app use */
#define PIN_POL_CONSOLE  (1u << 1) /* UART0 console PA10/11 */
#define PIN_POL_FLASH    (1u << 2) /* W25Q SPI1 PB14..17 */
#define PIN_POL_BSL      (1u << 3) /* PA18 BSL risk */
#define PIN_POL_LED      (1u << 4) /* PA14 LED load */
#define PIN_POL_HEADER   (1u << 5) /* on expansion header */

typedef struct {
    GPIO_Regs *port;
    uint32_t pin;
    uint32_t iomux;
} board_pin_t;

void board_pin_init(void);

/* Parse "PAx"/"PBx" into caller-owned storage. */
int board_pin_resolve(const char *name, board_pin_t *out);

/* Compact id: PA0..31 => 0..31, PB0..27 => 32..59; -1 if unbonded. */
int board_pin_id(const char *name);

/* IOMUX pin function 0..9; PF0 disconnects digital for analog. */
int board_pin_af(const char *name, unsigned pf, int input_enable);

/*
 * Claim pin for owner. force!=0 steals from non-SYS owners.
 * 0 ok, -1 unknown pin, -2 policy/SYS lock, -3 busy (other owner).
 */
int board_pin_claim(const char *name, uint8_t owner, int force);
int board_pin_claim_id(int id, uint8_t owner, int force);
void board_pin_release(const char *name);
void board_pin_release_owned(const char *name, uint8_t owner);
void board_pin_release_owner(uint8_t owner);
/* Release every VM-owned pin, preserving SYS, console UART and SPI Flash. */
void board_pin_reset_app_owners(void);
int board_pin_owner(const char *name); /* PIN_OWN_* or -1 */
const char *board_pin_owner_str(uint8_t owner);
unsigned board_pin_policy(const char *name);

/* Header-oriented helpers */
int board_adc_pin_channel(const char *name); /* ADC0 ch 0..7 or -1 */
/*
 * Simple PWM route: *timer_out 0=TIMG12 1=TIMG7; *ccp_out 0/1.
 * Returns 0 if pin can host independent PWM.
 */
int board_pwm_route(const char *pin, unsigned *pf_out, unsigned *ccp_out,
    int *timer_out);
/*
 * Complementary pair: *tima_out 0=TIMA0 1=TIMA1 (C0 + C0N + dead-band).
 */
int board_pwm_comp_route(const char *hi, const char *lo,
    unsigned *pf_hi, unsigned *pf_lo, int *tima_out);

/* Short error tag for Lua: "pin","locked","busy","sys","flash","console" */
const char *board_pin_errstr(int claim_rc);

#endif
