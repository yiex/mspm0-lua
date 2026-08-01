/*****************************************************************************

  Copyright (C) 2026 Texas Instruments Incorporated - http://www.ti.com/

  Redistribution and use in source and binary forms, with or without
  modification, are permitted provided that the following conditions
  are met:

   Redistributions of source code must retain the above copyright
   notice, this list of conditions and the following disclaimer.

   Redistributions in binary form must reproduce the above copyright
   notice, this list of conditions and the following disclaimer in the
   documentation and/or other materials provided with the
   distribution.

   Neither the name of Texas Instruments Incorporated nor the names of
   its contributors may be used to endorse or promote products derived
   from this software without specific prior written permission.

  THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
  "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
  LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
  A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT
  OWNER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
  SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT
  LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
  DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
  THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
  (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
  OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

*****************************************************************************/
#include "ti_msp_dl_config.h"

volatile uint32_t g_cpuclk_hz = CPUCLK_FREQ_SYSOSC;
volatile uint32_t g_uart_busclk_hz = UART_BUSCLK_FREQ_SYSOSC;
volatile uint8_t g_hfxt_ok = 0;

SYSCONFIG_WEAK void SYSCFG_DL_init(void)
{
    SYSCFG_DL_initPower();
    SYSCFG_DL_GPIO_init();
    SYSCFG_DL_SYSCTL_init();
    SYSCFG_DL_UART_0_init();
}

SYSCONFIG_WEAK void SYSCFG_DL_initPower(void)
{
    DL_GPIO_reset(GPIOA);
    DL_GPIO_reset(GPIOB);
    DL_UART_Main_reset(UART_0_INST);

    DL_GPIO_enablePower(GPIOA);
    DL_GPIO_enablePower(GPIOB);
    DL_UART_Main_enablePower(UART_0_INST);
    delay_cycles(POWER_STARTUP_DELAY);
}

SYSCONFIG_WEAK void SYSCFG_DL_GPIO_init(void)
{
    DL_GPIO_initPeripheralAnalogFunction(GPIO_HFXIN_IOMUX);
    DL_GPIO_initPeripheralAnalogFunction(GPIO_HFXOUT_IOMUX);

    DL_GPIO_initPeripheralOutputFunction(
        GPIO_UART_0_IOMUX_TX, GPIO_UART_0_IOMUX_TX_FUNC);
    DL_GPIO_initPeripheralInputFunction(
        GPIO_UART_0_IOMUX_RX, GPIO_UART_0_IOMUX_RX_FUNC);

    DL_GPIO_initDigitalOutput(GPIO_LEDS_USER_LED_IOMUX);
    DL_GPIO_clearPins(GPIO_LEDS_PORT, GPIO_LEDS_USER_LED_PIN);
    DL_GPIO_enableOutput(GPIO_LEDS_PORT, GPIO_LEDS_USER_LED_PIN);
}

/* Same as IMU: HFXT 40 MHz → PLL → CLK2x = 80 MHz MCLK */
static const DL_SYSCTL_SYSPLLConfig gSYSPLLConfig = {
    .inputFreq = DL_SYSCTL_SYSPLL_INPUT_FREQ_32_48_MHZ,
    .rDivClk2x = 3,
    .rDivClk1 = 1,
    .rDivClk0 = 0,
    .enableCLK2x = DL_SYSCTL_SYSPLL_CLK2X_ENABLE,
    .enableCLK1 = DL_SYSCTL_SYSPLL_CLK1_ENABLE,
    .enableCLK0 = DL_SYSCTL_SYSPLL_CLK0_DISABLE,
    .sysPLLMCLK = DL_SYSCTL_SYSPLL_MCLK_CLK2X,
    .sysPLLRef = DL_SYSCTL_SYSPLL_REF_HFCLK,
    .qDiv = 3,
    .pDiv = DL_SYSCTL_SYSPLL_PDIV_1,
};

/* ~timeout loops while still on SYSOSC 32 MHz */
static int wait_clk_bit(uint32_t mask, uint32_t want, uint32_t spins)
{
    while (spins--) {
        if ((DL_SYSCTL_getClockStatus() & mask) == want) {
            return 1;
        }
    }
    return 0;
}

/* configSYSPLL with timeout (DriverLib version can hang forever) */
static int config_syspll_timeout(const DL_SYSCTL_SYSPLLConfig *config)
{
    DL_SYSCTL_disableSYSPLL();
    if (!wait_clk_bit(DL_SYSCTL_CLK_STATUS_SYSPLL_OFF,
            DL_SYSCTL_CLK_STATUS_SYSPLL_OFF, 1600000u)) {
        return 0;
    }

    DL_Common_updateReg(&SYSCTL->SOCLOCK.SYSPLLCFG0,
        ((uint32_t)config->sysPLLRef), SYSCTL_SYSPLLCFG0_SYSPLLREF_MASK);
    DL_Common_updateReg(&SYSCTL->SOCLOCK.SYSPLLCFG1, ((uint32_t)config->pDiv),
        SYSCTL_SYSPLLCFG1_PDIV_MASK);

    {
        uint32_t ctlTemp = DL_CORE_getInstructionConfig();
        DL_CORE_configInstruction(DL_CORE_PREFETCH_ENABLED, DL_CORE_CACHE_DISABLED,
            DL_CORE_LITERAL_CACHE_ENABLED);
        SYSCTL->SOCLOCK.SYSPLLPARAM0 =
            *(volatile uint32_t *)((uint32_t)config->inputFreq);
        SYSCTL->SOCLOCK.SYSPLLPARAM1 =
            *(volatile uint32_t *)((uint32_t)config->inputFreq + 4u);
        CPUSS->CTL = ctlTemp;
    }

    DL_Common_updateReg(&SYSCTL->SOCLOCK.SYSPLLCFG1,
        ((config->qDiv << SYSCTL_SYSPLLCFG1_QDIV_OFS) &
            SYSCTL_SYSPLLCFG1_QDIV_MASK),
        SYSCTL_SYSPLLCFG1_QDIV_MASK);
    DL_Common_updateReg(&SYSCTL->SOCLOCK.SYSPLLCFG0,
        (((config->rDivClk2x << SYSCTL_SYSPLLCFG0_RDIVCLK2X_OFS) &
             SYSCTL_SYSPLLCFG0_RDIVCLK2X_MASK) |
            ((config->rDivClk1 << SYSCTL_SYSPLLCFG0_RDIVCLK1_OFS) &
                SYSCTL_SYSPLLCFG0_RDIVCLK1_MASK) |
            ((config->rDivClk0 << SYSCTL_SYSPLLCFG0_RDIVCLK0_OFS) &
                SYSCTL_SYSPLLCFG0_RDIVCLK0_MASK) |
            config->enableCLK2x | config->enableCLK1 | config->enableCLK0 |
            (uint32_t)config->sysPLLMCLK),
        (SYSCTL_SYSPLLCFG0_RDIVCLK2X_MASK | SYSCTL_SYSPLLCFG0_RDIVCLK1_MASK |
            SYSCTL_SYSPLLCFG0_RDIVCLK0_MASK |
            SYSCTL_SYSPLLCFG0_ENABLECLK2X_MASK |
            SYSCTL_SYSPLLCFG0_ENABLECLK1_MASK |
            SYSCTL_SYSPLLCFG0_ENABLECLK0_MASK |
            SYSCTL_SYSPLLCFG0_MCLK2XVCO_MASK));

    DL_SYSCTL_enableSYSPLL();
    return wait_clk_bit(SYSCTL_CLKSTATUS_SYSPLLGOOD_MASK,
        DL_SYSCTL_CLK_STATUS_SYSPLL_GOOD, 1600000u);
}

/*
 * Try HFXT+SYSPLL. On failure leave SYSOSC 32 MHz running.
 * Never infinite-loop on crystal/PLL lock.
 */
static int try_hfxt_pll(void)
{
    DL_SYSCTL_setFlashWaitState(DL_SYSCTL_FLASH_WAIT_STATE_2);
    DL_SYSCTL_disableHFXT();
    DL_SYSCTL_disableSYSPLL();

    DL_SYSCTL_setHFXTFrequencyRange(DL_SYSCTL_HFXT_RANGE_32_48_MHZ);
    /* startupTime in ~64 us steps; 20 ≈ 1.28 ms (same as IMU) */
    DL_SYSCTL_setHFXTStartupTime(20);
    SYSCTL->SOCLOCK.HSCLKEN |= SYSCTL_HSCLKEN_HFXTEN_ENABLE;
    DL_SYSCTL_enableHFCLKStartupMonitor();

    /* ~50 ms @ 32 MHz busy poll */
    if (!wait_clk_bit(SYSCTL_CLKSTATUS_HFCLKGOOD_MASK,
            DL_SYSCTL_CLK_STATUS_HFCLK_GOOD, 1600000u)) {
        DL_SYSCTL_disableHFXT();
        return 0;
    }

    if (!config_syspll_timeout(&gSYSPLLConfig)) {
        DL_SYSCTL_disableSYSPLL();
        DL_SYSCTL_disableHFXT();
        return 0;
    }

    DL_SYSCTL_setULPCLKDivider(DL_SYSCTL_ULPCLK_DIV_2);
    DL_SYSCTL_setMCLKSource(SYSOSC, HSCLK, DL_SYSCTL_HSCLK_SOURCE_SYSPLL);
    return 1;
}

SYSCONFIG_WEAK void SYSCFG_DL_SYSCTL_init(void)
{
    DL_SYSCTL_setBORThreshold(DL_SYSCTL_BOR_THRESHOLD_LEVEL_0);
    DL_SYSCTL_setSYSOSCFreq(DL_SYSCTL_SYSOSC_FREQ_BASE);
    DL_SYSCTL_disableHFXT();
    DL_SYSCTL_disableSYSPLL();
    DL_SYSCTL_setULPCLKDivider(DL_SYSCTL_ULPCLK_DIV_1);

    g_cpuclk_hz = CPUCLK_FREQ_SYSOSC;
    g_uart_busclk_hz = UART_BUSCLK_FREQ_SYSOSC;
    g_hfxt_ok = 0;

    if (try_hfxt_pll()) {
        g_cpuclk_hz = CPUCLK_FREQ_HFXT;
        g_uart_busclk_hz = UART_BUSCLK_FREQ_HFXT;
        g_hfxt_ok = 1;
    } else {
        /* stay on SYSOSC 32 MHz */
        DL_SYSCTL_setFlashWaitState(DL_SYSCTL_FLASH_WAIT_STATE_0);
        DL_SYSCTL_setULPCLKDivider(DL_SYSCTL_ULPCLK_DIV_1);
        g_cpuclk_hz = CPUCLK_FREQ_SYSOSC;
        g_uart_busclk_hz = UART_BUSCLK_FREQ_SYSOSC;
        g_hfxt_ok = 0;
    }
}

bool board_clock_hfxt_ok(void)
{
    return g_hfxt_ok != 0;
}

static const DL_UART_Main_ClockConfig gUART_0ClockConfig = {
    .clockSel = DL_UART_MAIN_CLOCK_BUSCLK,
    .divideRatio = DL_UART_MAIN_CLOCK_DIVIDE_RATIO_1,
};

static const DL_UART_Main_Config gUART_0Config = {
    .mode = DL_UART_MAIN_MODE_NORMAL,
    .direction = DL_UART_MAIN_DIRECTION_TX_RX,
    .flowControl = DL_UART_MAIN_FLOW_CONTROL_NONE,
    .parity = DL_UART_MAIN_PARITY_NONE,
    .wordLength = DL_UART_MAIN_WORD_LENGTH_8_BITS,
    .stopBits = DL_UART_MAIN_STOP_BITS_ONE,
};

SYSCONFIG_WEAK void SYSCFG_DL_UART_0_init(void)
{
    uint32_t ibrd;
    uint32_t fbrd;

    DL_UART_Main_setClockConfig(
        UART_0_INST, (DL_UART_Main_ClockConfig *)&gUART_0ClockConfig);
    DL_UART_Main_init(UART_0_INST, (DL_UART_Main_Config *)&gUART_0Config);
    DL_UART_Main_setOversampling(UART_0_INST, DL_UART_OVERSAMPLING_RATE_16X);

    if (g_hfxt_ok) {
        ibrd = UART_0_IBRD_40_MHZ_115200_BAUD;
        fbrd = UART_0_FBRD_40_MHZ_115200_BAUD;
    } else {
        ibrd = UART_0_IBRD_32_MHZ_115200_BAUD;
        fbrd = UART_0_FBRD_32_MHZ_115200_BAUD;
    }
    DL_UART_Main_setBaudRateDivisor(UART_0_INST, ibrd, fbrd);
    DL_UART_Main_enableFIFOs(UART_0_INST);
    DL_UART_Main_setRXFIFOThreshold(UART_0_INST, DL_UART_RX_FIFO_LEVEL_1_2_FULL);
    DL_UART_Main_setTXFIFOThreshold(UART_0_INST, DL_UART_TX_FIFO_LEVEL_1_2_EMPTY);
    DL_UART_Main_enable(UART_0_INST);
}

SYSCONFIG_WEAK void SYSCFG_DL_SPI_FLASH_init(void)
{
    DL_SPI_reset(SPI_FLASH_INST);
    DL_SPI_enablePower(SPI_FLASH_INST);
    delay_cycles(POWER_STARTUP_DELAY);

    DL_GPIO_initPeripheralInputFunction(
        GPIO_SPI_FLASH_IOMUX_POCI, GPIO_SPI_FLASH_IOMUX_POCI_FUNC);
    DL_GPIO_initPeripheralOutputFunction(
        GPIO_SPI_FLASH_IOMUX_PICO, GPIO_SPI_FLASH_IOMUX_PICO_FUNC);
    DL_GPIO_initPeripheralOutputFunction(
        GPIO_SPI_FLASH_IOMUX_CLK, GPIO_SPI_FLASH_IOMUX_CLK_FUNC);

    DL_GPIO_initDigitalOutput(GPIO_SPI_FLASH_IOMUX_CS);
    DL_GPIO_setPins(GPIO_SPI_FLASH_CS_PORT, GPIO_SPI_FLASH_CS_PIN);
    DL_GPIO_enableOutput(GPIO_SPI_FLASH_CS_PORT, GPIO_SPI_FLASH_CS_PIN);

    DL_SPI_Config cfg = {
        .mode = DL_SPI_MODE_CONTROLLER,
        .frameFormat = DL_SPI_FRAME_FORMAT_MOTO4_POL0_PHA0,
        .parity = DL_SPI_PARITY_NONE,
        .dataSize = DL_SPI_DATA_SIZE_8,
        .bitOrder = DL_SPI_BIT_ORDER_MSB_FIRST,
        .chipSelectPin = DL_SPI_CHIP_SELECT_NONE,
    };
    DL_SPI_ClockConfig clk = {
        .clockSel = DL_SPI_CLOCK_BUSCLK,
        .divideRatio = DL_SPI_CLOCK_DIVIDE_RATIO_1,
    };
    DL_SPI_setClockConfig(SPI_FLASH_INST, &clk);
    DL_SPI_init(SPI_FLASH_INST, &cfg);
    /* ~1 MHz: divider depends on bus clk */
    if (g_hfxt_ok) {
        DL_SPI_setBitRateSerialClockDivider(SPI_FLASH_INST, 39);
    } else {
        DL_SPI_setBitRateSerialClockDivider(SPI_FLASH_INST, 31);
    }
    DL_SPI_enable(SPI_FLASH_INST);
}
