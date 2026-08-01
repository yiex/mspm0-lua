#include "board_uart.h"
#include "ti_msp_dl_config.h"
#include "board_status.h"

#define RX_RING_SIZE 256
static volatile uint8_t s_rx_ring[RX_RING_SIZE];
static volatile uint16_t s_rx_head;
static volatile uint16_t s_rx_tail;
static uint32_t s_uart_baud = UART_0_BAUD_RATE;

static void ring_push(uint8_t b)
{
    uint16_t next = (uint16_t)((s_rx_head + 1u) % RX_RING_SIZE);
    if (next == s_rx_tail) {
        return;
    }
    s_rx_ring[s_rx_head] = b;
    s_rx_head = next;
    board_status_or(ST_UART_RX_HIT);
}

static int ring_pop(void)
{
    if (s_rx_tail == s_rx_head) {
        return -1;
    }
    uint8_t b = s_rx_ring[s_rx_tail];
    s_rx_tail = (uint16_t)((s_rx_tail + 1u) % RX_RING_SIZE);
    return (int)b;
}

static void drain_hw_fifo_to_ring(void)
{
    /* Critical: IRQ + poll must not both read the same FIFO byte. */
#if defined(BOARD_UART_IRQ) && (BOARD_UART_IRQ)
    NVIC_DisableIRQ(UART_0_INST_INT_IRQN);
#endif
    while (!DL_UART_Main_isRXFIFOEmpty(UART_0_INST)) {
        uint8_t b = DL_UART_Main_receiveData(UART_0_INST);
        ring_push(b);
    }
#if defined(BOARD_UART_IRQ) && (BOARD_UART_IRQ)
    NVIC_EnableIRQ(UART_0_INST_INT_IRQN);
#endif
}

/* Optional IRQ path; enabled only if BOARD_UART_IRQ=1 */
#if defined(BOARD_UART_IRQ) && (BOARD_UART_IRQ)
void UART0_IRQHandler(void)
{
    switch (DL_UART_Main_getPendingInterrupt(UART_0_INST)) {
    case DL_UART_IIDX_RX:
        drain_hw_fifo_to_ring();
        break;
    default:
        break;
    }
}
#endif

static void uart_pins_and_irq(int clear_ring)
{
    if (clear_ring) {
        s_rx_head = 0;
        s_rx_tail = 0;
    }
    /* Restore console pins (scripts/gpio.af may steal PA10/PA11). */
    DL_GPIO_initPeripheralOutputFunction(
        GPIO_UART_0_IOMUX_TX, GPIO_UART_0_IOMUX_TX_FUNC);
    DL_GPIO_initPeripheralInputFunction(
        GPIO_UART_0_IOMUX_RX, GPIO_UART_0_IOMUX_RX_FUNC);
    DL_UART_Main_enable(UART_0_INST);
#if defined(BOARD_UART_IRQ) && (BOARD_UART_IRQ)
    DL_UART_Main_setRXFIFOThreshold(UART_0_INST, DL_UART_RX_FIFO_LEVEL_ONE_ENTRY);
    DL_UART_Main_enableInterrupt(UART_0_INST, DL_UART_INTERRUPT_RX);
    NVIC_ClearPendingIRQ(UART_0_INST_INT_IRQN);
    NVIC_EnableIRQ(UART_0_INST_INT_IRQN);
#else
    DL_UART_Main_disableInterrupt(UART_0_INST, DL_UART_INTERRUPT_RX);
    NVIC_DisableIRQ(UART_0_INST_INT_IRQN);
#endif
    drain_hw_fifo_to_ring();
}

void board_uart_init(void)
{
    s_uart_baud = UART_0_BAUD_RATE;
    uart_pins_and_irq(1);
}

void board_uart_rearm(void)
{
    uart_pins_and_irq(0);
}

static int wait_tx_room(uint32_t spins)
{
    while (DL_UART_Main_isTXFIFOFull(UART_0_INST)) {
        if (spins-- == 0) {
            return -1;
        }
    }
    return 0;
}

static int wait_not_busy(uint32_t spins)
{
    while (DL_UART_Main_isBusy(UART_0_INST)) {
        if (spins-- == 0) {
            return -1;
        }
    }
    return 0;
}

int board_uart_set_baud(uint32_t baud)
{
    if (baud == s_uart_baud) {
        return 0;
    }
    if (baud < 9600u || baud > 1000000u || wait_not_busy(200000u) != 0) {
        return -1;
    }
#if defined(BOARD_UART_IRQ) && (BOARD_UART_IRQ)
    NVIC_DisableIRQ(UART_0_INST_INT_IRQN);
#endif
    DL_UART_Main_disable(UART_0_INST);
    DL_UART_Main_disableFIFOs(UART_0_INST);
    DL_UART_Main_configBaudRate(UART_0_INST, g_uart_busclk_hz, baud);
    DL_UART_Main_enableFIFOs(UART_0_INST);
    DL_UART_Main_setRXFIFOThreshold(UART_0_INST, DL_UART_RX_FIFO_LEVEL_ONE_ENTRY);
    DL_UART_Main_setTXFIFOThreshold(UART_0_INST, DL_UART_TX_FIFO_LEVEL_1_2_EMPTY);
    DL_UART_Main_enable(UART_0_INST);
    s_uart_baud = baud;
#if defined(BOARD_UART_IRQ) && (BOARD_UART_IRQ)
    DL_UART_Main_enableInterrupt(UART_0_INST, DL_UART_INTERRUPT_RX);
    NVIC_ClearPendingIRQ(UART_0_INST_INT_IRQN);
    NVIC_EnableIRQ(UART_0_INST_INT_IRQN);
#endif
    return 0;
}

uint32_t board_uart_get_baud(void)
{
    return s_uart_baud;
}

static int uart_write_byte(char c)
{
#if !defined(BOARD_UART_IRQ) || !(BOARD_UART_IRQ)
    drain_hw_fifo_to_ring();
#endif
    if (wait_tx_room(200000u) != 0) {
        return -1;
    }
    DL_UART_Main_transmitData(UART_0_INST, (uint8_t)c);
#if !defined(BOARD_UART_IRQ) || !(BOARD_UART_IRQ)
    drain_hw_fifo_to_ring();
#endif
    return 0;
}

void board_uart_putc(char c)
{
    if (uart_write_byte(c) != 0) return;
    (void)wait_not_busy(200000u);
}

void board_uart_write(const char *s, size_t n)
{
    for (size_t i = 0; i < n; i++) {
        if (s[i] == '\n') {
            if (uart_write_byte('\r') != 0) return;
        }
        if (uart_write_byte(s[i]) != 0) return;
    }
    (void)wait_not_busy(200000u);
}

void board_uart_puts(const char *s)
{
    while (*s) {
        if (*s == '\n') {
            if (uart_write_byte('\r') != 0) return;
        }
        if (uart_write_byte(*s++) != 0) return;
    }
    (void)wait_not_busy(200000u);
}

int board_uart_getc_nonblock(void)
{
    /* Always drain HW FIFO: if RX IRQ was lost (e.g. after long Lua),
     * polling still recovers console commands for upload/ls. */
    drain_hw_fifo_to_ring();
    return ring_pop();
}

int board_uart_peek_nonblock(void)
{
    drain_hw_fifo_to_ring();
    if (s_rx_tail == s_rx_head) {
        return -1;
    }
    return (int)s_rx_ring[s_rx_tail];
}

int board_uart_loopback_ok(void)
{
    int ok = 0;
#if defined(BOARD_UART_IRQ) && (BOARD_UART_IRQ)
    DL_UART_Main_disableInterrupt(UART_0_INST, DL_UART_INTERRUPT_RX);
#endif
    DL_UART_Main_enableLoopbackMode(UART_0_INST);
    while (!DL_UART_Main_isRXFIFOEmpty(UART_0_INST)) {
        (void)DL_UART_Main_receiveData(UART_0_INST);
    }
    s_rx_head = s_rx_tail = 0;
    if (wait_tx_room(50000u) == 0) {
        DL_UART_Main_transmitData(UART_0_INST, 0xA5);
        (void)wait_not_busy(50000u);
        for (volatile int i = 0; i < 20000; i++) {
            if (!DL_UART_Main_isRXFIFOEmpty(UART_0_INST)) {
                uint8_t b = DL_UART_Main_receiveData(UART_0_INST);
                ok = (b == 0xA5) ? 1 : 0;
                break;
            }
        }
    }
    DL_UART_Main_disableLoopbackMode(UART_0_INST);
#if defined(BOARD_UART_IRQ) && (BOARD_UART_IRQ)
    DL_UART_Main_enableInterrupt(UART_0_INST, DL_UART_INTERRUPT_RX);
#endif
    return ok;
}

int board_uart0_app_acquire(void)
{
    uint32_t spins = 200000u;
    drain_hw_fifo_to_ring();
    while (DL_UART_Main_isBusy(UART_0_INST)) {
        if (spins-- == 0u) return -1;
    }
    DL_UART_Main_disable(UART_0_INST);
    return 0;
}

void board_uart0_app_release(void)
{
    uint32_t baud = s_uart_baud;
    DL_UART_Main_reset(UART_0_INST);
    DL_UART_Main_enablePower(UART_0_INST);
    delay_cycles(POWER_STARTUP_DELAY);
    SYSCFG_DL_UART_0_init();
    s_uart_baud = UART_0_BAUD_RATE;
    (void)board_uart_set_baud(baud);
    board_uart_rearm();
}
