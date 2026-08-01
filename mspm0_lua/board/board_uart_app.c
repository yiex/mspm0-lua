#include "board_uart_app.h"

#include <string.h>
#include <ti/driverlib/driverlib.h>

#include "board_irq.h"
#include "board_pins.h"
#include "ti_msp_dl_config.h"

#define APP_UART_RING 384
#define APP_UART_N 3

static uint8_t s_ring[APP_UART_N][APP_UART_RING];
static volatile uint16_t s_rh[APP_UART_N];
static volatile uint16_t s_rt[APP_UART_N];
static volatile uint32_t s_drop[APP_UART_N];
static UART_Regs *s_regs[APP_UART_N];

static UART_Regs *uart_regs(unsigned id)
{
    return id == 1u ? UART1 : id == 2u ? UART2 : id == 3u ? UART3 : 0;
}

static IRQn_Type uart_irqn(unsigned id)
{
    return id == 1u ? UART1_INT_IRQn : id == 2u ? UART2_INT_IRQn : UART3_INT_IRQn;
}

/*
 * App UART pinmux (MSPM0G3507 PINCM PF). Header-first; Flash/console excluded.
 * UART1 TX: PA17, PB6, PA8 | RX: PA18, PA9, PB7
 * UART2 TX: PA23, PA21, PB17 | RX: PA24, PA22, PB18
 * UART3 TX: PA26, PA14(pf4), PB2 | RX: PA25, PB3, PA13(pf4)
 */
static int uart_pf(unsigned id, const char *pin, int tx)
{
    if (!pin) {
        return -1;
    }
    if (id == 1u) {
        if (tx && (!strcmp(pin, "PA17") || !strcmp(pin, "PB6") ||
                !strcmp(pin, "PA8"))) {
            return 2;
        }
        if (!tx && (!strcmp(pin, "PA18") || !strcmp(pin, "PA9") ||
                !strcmp(pin, "PB7"))) {
            return 2;
        }
    } else if (id == 2u) {
        if (tx && (!strcmp(pin, "PA23") || !strcmp(pin, "PA21") ||
                !strcmp(pin, "PB17"))) {
            return 2;
        }
        if (!tx && (!strcmp(pin, "PA24") || !strcmp(pin, "PA22") ||
                !strcmp(pin, "PB18"))) {
            return 2;
        }
    } else if (id == 3u) {
        if (tx && (!strcmp(pin, "PA26") || !strcmp(pin, "PB2"))) {
            return 2;
        }
        if (tx && !strcmp(pin, "PA14")) {
            return 4;
        }
        if (!tx && (!strcmp(pin, "PA25") || !strcmp(pin, "PB3"))) {
            return 2;
        }
        if (!tx && !strcmp(pin, "PA13")) {
            return 4;
        }
    }
    return -1;
}

static void ring_push(unsigned idx, uint8_t b)
{
    uint16_t h = s_rh[idx];
    uint16_t n = (uint16_t)((h + 1u) % APP_UART_RING);
    if (n == s_rt[idx]) {
        /* drop oldest byte so continuous streams still land */
        s_rt[idx] = (uint16_t)((s_rt[idx] + 1u) % APP_UART_RING);
        s_drop[idx]++;
    }
    s_ring[idx][h] = b;
    s_rh[idx] = n;
}

static int ring_pop(unsigned idx)
{
    uint16_t t = s_rt[idx];
    uint8_t b;
    if (t == s_rh[idx]) {
        return -1;
    }
    b = s_ring[idx][t];
    s_rt[idx] = (uint16_t)((t + 1u) % APP_UART_RING);
    return b;
}

static void uart_rx_isr(unsigned idx)
{
    UART_Regs *regs = s_regs[idx];
    if (!regs) {
        return;
    }
    /* Always drain RX FIFO; clear whatever IRQ woke us. */
    (void)DL_UART_Main_getPendingInterrupt(regs);
    while (!DL_UART_Main_isRXFIFOEmpty(regs)) {
        ring_push(idx, DL_UART_Main_receiveData(regs));
    }
}

void UART1_IRQHandler(void) { uart_rx_isr(0); }
void UART2_IRQHandler(void) { uart_rx_isr(1); }
void UART3_IRQHandler(void) { uart_rx_isr(2); }

int board_uart_app_open(board_uart_app_t *port, unsigned id,
    const char *tx, const char *rx, uint32_t baud)
{
    static const DL_UART_Main_ClockConfig clk = {
        .clockSel = DL_UART_MAIN_CLOCK_BUSCLK,
        .divideRatio = DL_UART_MAIN_CLOCK_DIVIDE_RATIO_1,
    };
    static const DL_UART_Main_Config cfg = {
        .mode = DL_UART_MAIN_MODE_NORMAL,
        .direction = DL_UART_MAIN_DIRECTION_TX_RX,
        .flowControl = DL_UART_MAIN_FLOW_CONTROL_NONE,
        .parity = DL_UART_MAIN_PARITY_NONE,
        .wordLength = DL_UART_MAIN_WORD_LENGTH_8_BITS,
        .stopBits = DL_UART_MAIN_STOP_BITS_ONE,
    };
    UART_Regs *regs = uart_regs(id);
    int txpf = uart_pf(id, tx, 1);
    int rxpf = uart_pf(id, rx, 0);
    uint32_t div64;
    unsigned idx;
    if (!port || !regs || id < 1u || id > 3u || txpf < 0 || rxpf < 0 ||
            baud < 1200u || baud > 2000000u) {
        return -1;
    }
    idx = id - 1u;
    if (board_pin_af(tx, (unsigned)txpf, 0) != 0 ||
            board_pin_af(rx, (unsigned)rxpf, 1) != 0) {
        return -1;
    }
    s_rh[idx] = s_rt[idx] = 0;
    s_regs[idx] = regs;

    DL_UART_Main_reset(regs);
    DL_UART_Main_enablePower(regs);
    delay_cycles(POWER_STARTUP_DELAY);
    DL_UART_Main_setClockConfig(regs, (DL_UART_Main_ClockConfig *)&clk);
    DL_UART_Main_init(regs, (DL_UART_Main_Config *)&cfg);
    DL_UART_Main_setOversampling(regs, DL_UART_OVERSAMPLING_RATE_16X);
    div64 = (g_uart_busclk_hz * 4u + baud / 2u) / baud;
    DL_UART_Main_setBaudRateDivisor(regs, div64 / 64u, div64 % 64u);
    DL_UART_Main_enableFIFOs(regs);
    DL_UART_Main_setRXFIFOThreshold(regs, DL_UART_RX_FIFO_LEVEL_ONE_ENTRY);
    DL_UART_Main_enableInterrupt(regs, DL_UART_INTERRUPT_RX);
    NVIC_ClearPendingIRQ(uart_irqn(id));
    NVIC_EnableIRQ(uart_irqn(id));
    DL_UART_Main_enable(regs);

    port->regs = regs;
    port->open = 1;
    port->id = (uint8_t)id;
    return 0;
}

void board_uart_app_close(board_uart_app_t *port)
{
    if (port && port->open) {
        unsigned idx = port->id ? (unsigned)port->id - 1u : 0u;
        DL_UART_Main_disableInterrupt(port->regs, DL_UART_INTERRUPT_RX);
        NVIC_DisableIRQ(uart_irqn(port->id));
        DL_UART_Main_disable(port->regs);
        s_regs[idx] = 0;
        port->open = 0;
    }
}

int board_uart_app_write(board_uart_app_t *port, const uint8_t *data, size_t n)
{
    if (!port || !port->open || (!data && n)) return -1;
    while (n--) DL_UART_Main_transmitDataBlocking(port->regs, *data++);
    return 0;
}

size_t board_uart_app_rx_avail(unsigned id)
{
    unsigned idx;
    uint16_t h, t;
    if (id < 1u || id > 3u) return 0;
    idx = id - 1u;
    h = s_rh[idx];
    t = s_rt[idx];
    if (h >= t) return (size_t)(h - t);
    return (size_t)(APP_UART_RING - t + h);
}

size_t board_uart_app_read(board_uart_app_t *port, uint8_t *data, size_t n,
    uint32_t timeout_ms)
{
    size_t got = 0;
    uint32_t start = board_millis();
    unsigned idx;
    if (!port || !port->open || !data || !n) return 0;
    idx = (unsigned)port->id - 1u;
    while (got < n) {
        int b = ring_pop(idx);
        if (b >= 0) {
            data[got++] = (uint8_t)b;
            continue;
        }
        /* drain any residual FIFO without IRQ race */
        if (!DL_UART_Main_isRXFIFOEmpty(port->regs)) {
            data[got++] = DL_UART_Main_receiveData(port->regs);
            continue;
        }
        if (got || (uint32_t)(board_millis() - start) >= timeout_ms) {
            break;
        }
    }
    return got;
}

void board_uart_app_poll(void)
{
    unsigned idx;
    for (idx = 0; idx < APP_UART_N; idx++) {
        UART_Regs *regs = s_regs[idx];
        if (!regs) {
            continue;
        }
        while (!DL_UART_Main_isRXFIFOEmpty(regs)) {
            ring_push(idx, DL_UART_Main_receiveData(regs));
        }
    }
}

uint32_t board_uart_app_drops(unsigned id)
{
    if (id < 1u || id > 3u) {
        return 0;
    }
    return s_drop[id - 1u];
}
