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
#ifndef ti_msp_dl_config_h
#define ti_msp_dl_config_h

#define CONFIG_MSPM0G3507
#define SYSCONFIG_WEAK __attribute__((weak))

#include <ti/devices/msp/msp.h>
#include <ti/driverlib/driverlib.h>
#include <ti/driverlib/m0p/dl_core.h>
#include <stdint.h>
#include <stdbool.h>

#ifdef __cplusplus
extern "C" {
#endif

#define POWER_STARTUP_DELAY (16)

/*
 * Target clock (after successful HFXT+PLL):
 *   HFXT 40 MHz → SYSPLL → MCLK 80 MHz, ULPCLK 40 MHz
 * Fallback: SYSOSC 32 MHz (MCLK=ULPCLK=32 MHz)
 */
#define CPUCLK_FREQ_HFXT 80000000u
#define CPUCLK_FREQ_SYSOSC 32000000u
#define UART_BUSCLK_FREQ_HFXT 40000000u
#define UART_BUSCLK_FREQ_SYSOSC 32000000u

/* Default compile-time for delay_cycles sizing (actual: g_cpuclk_hz) */
#define CPUCLK_FREQ CPUCLK_FREQ_HFXT

/* HFXT pins PA5/PA6 */
#define GPIO_HFXIN_IOMUX (IOMUX_PINCM10)
#define GPIO_HFXOUT_IOMUX (IOMUX_PINCM11)

/* UART0 on PA10(TX)/PA11(RX) via CH340 */
#define UART_0_INST UART0
#define UART_0_INST_IRQHandler UART0_IRQHandler
#define UART_0_INST_INT_IRQN UART0_INT_IRQn
#define GPIO_UART_0_RX_PORT GPIOA
#define GPIO_UART_0_TX_PORT GPIOA
#define GPIO_UART_0_RX_PIN DL_GPIO_PIN_11
#define GPIO_UART_0_TX_PIN DL_GPIO_PIN_10
#define GPIO_UART_0_IOMUX_RX (IOMUX_PINCM22)
#define GPIO_UART_0_IOMUX_TX (IOMUX_PINCM21)
#define GPIO_UART_0_IOMUX_RX_FUNC IOMUX_PINCM22_PF_UART0_RX
#define GPIO_UART_0_IOMUX_TX_FUNC IOMUX_PINCM21_PF_UART0_TX
#define UART_0_BAUD_RATE 115200u
#define UART_0_IBRD_40_MHZ_115200_BAUD 21
#define UART_0_FBRD_40_MHZ_115200_BAUD 45
#define UART_0_IBRD_32_MHZ_115200_BAUD 17
#define UART_0_FBRD_32_MHZ_115200_BAUD 23

/* User LED on PA14 */
#define GPIO_LEDS_PORT GPIOA
#define GPIO_LEDS_USER_LED_PIN DL_GPIO_PIN_14
#define GPIO_LEDS_USER_LED_IOMUX (IOMUX_PINCM36)

/* External SPI Flash W25Q32: PB14=POCI, PB15=PICO, PB16=CLK, PB17=CS */
#define SPI_FLASH_INST SPI1
#define GPIO_SPI_FLASH_POCI_PORT GPIOB
#define GPIO_SPI_FLASH_PICO_PORT GPIOB
#define GPIO_SPI_FLASH_CLK_PORT GPIOB
#define GPIO_SPI_FLASH_CS_PORT GPIOB
#define GPIO_SPI_FLASH_POCI_PIN DL_GPIO_PIN_14
#define GPIO_SPI_FLASH_PICO_PIN DL_GPIO_PIN_15
#define GPIO_SPI_FLASH_CLK_PIN DL_GPIO_PIN_16
#define GPIO_SPI_FLASH_CS_PIN DL_GPIO_PIN_17
#define GPIO_SPI_FLASH_IOMUX_POCI (IOMUX_PINCM31)
#define GPIO_SPI_FLASH_IOMUX_PICO (IOMUX_PINCM32)
#define GPIO_SPI_FLASH_IOMUX_CLK (IOMUX_PINCM33)
#define GPIO_SPI_FLASH_IOMUX_CS (IOMUX_PINCM43)
#define GPIO_SPI_FLASH_IOMUX_POCI_FUNC IOMUX_PINCM31_PF_SPI1_POCI
#define GPIO_SPI_FLASH_IOMUX_PICO_FUNC IOMUX_PINCM32_PF_SPI1_PICO
#define GPIO_SPI_FLASH_IOMUX_CLK_FUNC IOMUX_PINCM33_PF_SPI1_SCLK

extern volatile uint32_t g_cpuclk_hz;
extern volatile uint32_t g_uart_busclk_hz;
extern volatile uint8_t g_hfxt_ok;

void SYSCFG_DL_init(void);
void SYSCFG_DL_initPower(void);
void SYSCFG_DL_GPIO_init(void);
void SYSCFG_DL_SYSCTL_init(void);
void SYSCFG_DL_UART_0_init(void);
void SYSCFG_DL_SPI_FLASH_init(void);
bool board_clock_hfxt_ok(void);

#ifdef __cplusplus
}
#endif
#endif
