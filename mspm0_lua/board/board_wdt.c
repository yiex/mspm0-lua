#include "board_wdt.h"
#include "board_pins.h"
#include "board_resource.h"
#include "ti_msp_dl_config.h"

static uint8_t s_on;

int board_wdt_start(uint32_t period_ms)
{
    DL_WWDT_TIMER_PERIOD per;
    DL_WWDT_CLOCK_DIVIDE div = DL_WWDT_CLOCK_DIVIDE_4;

    if (s_on) {
        return 0;
    }
    if (board_resource_claim(BOARD_RES_WWDT1, PIN_OWN_SYS) != 0) {
        return -1;
    }
    if (period_ms == 0u) {
        period_ms = 500u;
    }
    /*
     * t ≈ div * 2^bits / 32768
     * div=4: 2^12→500ms, 2^15→4s, 2^10→125ms, 2^8→31ms, 2^18→8s
     */
    if (period_ms <= 40u) {
        per = DL_WWDT_TIMER_PERIOD_8_BITS;
    } else if (period_ms <= 150u) {
        per = DL_WWDT_TIMER_PERIOD_10_BITS;
    } else if (period_ms <= 700u) {
        per = DL_WWDT_TIMER_PERIOD_12_BITS;
    } else if (period_ms <= 5000u) {
        per = DL_WWDT_TIMER_PERIOD_15_BITS;
    } else {
        per = DL_WWDT_TIMER_PERIOD_18_BITS;
    }

    DL_WWDT_reset(WWDT1);
    DL_WWDT_enablePower(WWDT1);
    delay_cycles(POWER_STARTUP_DELAY);
    /* 0% closed window → feed anytime in period (product-friendly). */
    DL_WWDT_initWatchdogMode(WWDT1, div, per, DL_WWDT_RUN_IN_SLEEP,
        DL_WWDT_WINDOW_PERIOD_0, DL_WWDT_WINDOW_PERIOD_0);
    DL_WWDT_setActiveWindow(WWDT1, DL_WWDT_WINDOW0);
    s_on = 1;
    return 0;
}

void board_wdt_feed(void)
{
    if (s_on) {
        DL_WWDT_restart(WWDT1);
    }
}

int board_wdt_active(void)
{
    return s_on ? 1 : 0;
}
