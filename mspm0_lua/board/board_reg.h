#ifndef BOARD_REG_H
#define BOARD_REG_H

/*
 * Thin register HAL for hot paths (GPIO/IOMUX/delay).
 * Needs CMSIS device headers only (no DriverLib calls).
 */
#include <stdint.h>
#include <stdbool.h>
#include <ti/devices/msp/msp.h>

#ifndef IOMUX_PINCM_PC_CONNECTED
#define IOMUX_PINCM_PC_CONNECTED ((uint32_t)0x00000080u)
#endif
#ifndef IOMUX_PINCM_PIPU_ENABLE
#define IOMUX_PINCM_PIPU_ENABLE ((uint32_t)0x00020000u)
#endif
#ifndef IOMUX_PINCM_INENA_ENABLE
#define IOMUX_PINCM_INENA_ENABLE ((uint32_t)0x00040000u)
#endif

/* Busy-wait N CPU cycles (approx; used when no DriverLib delay_cycles). */
static inline void board_reg_delay_cycles(uint32_t cycles)
{
    while (cycles--) {
        __asm volatile("nop");
    }
}

static inline void board_reg_iomux_gpio_out(uint32_t pincm)
{
    IOMUX->SECCFG.PINCM[pincm] =
        IOMUX_PINCM_PC_CONNECTED | ((uint32_t)0x00000001u);
}

static inline void board_reg_iomux_gpio_in_pu(uint32_t pincm)
{
    IOMUX->SECCFG.PINCM[pincm] = IOMUX_PINCM_PC_CONNECTED |
        ((uint32_t)0x00000001u) | IOMUX_PINCM_INENA_ENABLE |
        IOMUX_PINCM_PIPU_ENABLE;
}

static inline void board_reg_gpio_enable_out(GPIO_Regs *port, uint32_t pin_mask)
{
    port->DOE31_0 |= pin_mask;
}

static inline void board_reg_gpio_set(GPIO_Regs *port, uint32_t pin_mask)
{
    port->DOUTSET31_0 = pin_mask;
}

static inline void board_reg_gpio_clr(GPIO_Regs *port, uint32_t pin_mask)
{
    port->DOUTCLR31_0 = pin_mask;
}

static inline void board_reg_gpio_tog(GPIO_Regs *port, uint32_t pin_mask)
{
    /* Single posted write (hardware toggle), no RMW. */
    port->DOUTTGL31_0 = pin_mask;
}

/* Fast path after gpio.mode out: no IOMUX traffic. */
static inline void board_reg_gpio_write(GPIO_Regs *port, uint32_t pin_mask,
    int high)
{
    if (high) {
        port->DOUTSET31_0 = pin_mask;
    } else {
        port->DOUTCLR31_0 = pin_mask;
    }
}

static inline uint32_t board_reg_gpio_read(GPIO_Regs *port, uint32_t pin_mask)
{
    return port->DIN31_0 & pin_mask;
}

static inline void board_reg_pin_out(GPIO_Regs *port, uint32_t pin_mask,
    uint32_t pincm)
{
    board_reg_iomux_gpio_out(pincm);
    board_reg_gpio_clr(port, pin_mask);
    board_reg_gpio_enable_out(port, pin_mask);
}

#endif
