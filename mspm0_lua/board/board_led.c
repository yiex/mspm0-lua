#include "board_led.h"

#include "board_pwm.h"
#include "board_pins.h"
#include "board_reg.h"
#include "ti_msp_dl_config.h"

static int s_pwm_id = -1;

static int gpio_drive(int on)
{
    if (s_pwm_id >= 0) {
        board_pwm_close(s_pwm_id);
        s_pwm_id = -1;
    }
    if (board_pin_claim("PA14", PIN_OWN_LED, 0) != 0) {
        return -1;
    }
    board_reg_iomux_gpio_out(IOMUX_PINCM36);
    board_reg_gpio_enable_out(GPIOA, DL_GPIO_PIN_14);
    if (on) {
        board_reg_gpio_set(GPIOA, DL_GPIO_PIN_14);
    } else {
        board_reg_gpio_clr(GPIOA, DL_GPIO_PIN_14);
        board_pin_release_owned("PA14", PIN_OWN_LED);
    }
    return 0;
}

int board_led_on(void) { return gpio_drive(1); }

int board_led_off(void) { return gpio_drive(0); }

int board_led_toggle(void)
{
    if (s_pwm_id >= 0) {
        board_pwm_close(s_pwm_id);
        s_pwm_id = -1;
        return gpio_drive(1);
    }
    if (board_pin_claim("PA14", PIN_OWN_LED, 0) != 0) {
        return -1;
    }
    board_reg_iomux_gpio_out(IOMUX_PINCM36);
    board_reg_gpio_enable_out(GPIOA, DL_GPIO_PIN_14);
    board_reg_gpio_tog(GPIOA, DL_GPIO_PIN_14);
    return 0;
}

int board_led_pwm(uint8_t duty)
{
    if (duty == 0) {
        gpio_drive(0);
        return 0;
    }
    if (s_pwm_id < 0) {
        board_pin_release_owned("PA14", PIN_OWN_LED);
        s_pwm_id = board_pwm_open("PA14", 1000);
        if (s_pwm_id < 0) {
            return -1;
        }
    }
    if (duty > 100) {
        duty = 100;
    }
    board_pwm_set_duty(s_pwm_id, duty);
    return 0;
}
