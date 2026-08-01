#include "board_delay.h"
#include "board_reg.h"
#include "ti_msp_dl_config.h"

void board_delay_us(uint32_t us)
{
    uint32_t mhz = g_cpuclk_hz / 1000000u;
    if (mhz == 0u) {
        mhz = 32u;
    }
    /* ~1 cycle per loop iteration (nop + branch); scale ~4 for M0+ */
    board_reg_delay_cycles(us * mhz / 4u + 1u);
}

void board_delay_ms(uint32_t ms)
{
    while (ms--) {
        board_delay_us(1000);
    }
}
