#include "native_module.h"

#include "board_pins.h"
#include "board_resource.h"
#include "g3507_pin_routes.h"
#include <ti/driverlib/driverlib.h>

#define UART_MAX_READ 256u
#define UART_WAIT_SPINS 200000u

extern const native_module_header_t g_native_module_header;

static int same_string(const char *a, const char *b)
{
    while (*a && *a == *b) { a++; b++; }
    return *a == *b;
}

static unsigned uart_slot(void)
{
    uintptr_t address = (uintptr_t)&g_native_module_header;
    if (address < NATIVE_MODULE_SLOT_ADDR ||
            address >= NATIVE_MODULE_SLOT_ADDR +
                NATIVE_MODULE_SLOT_COUNT * NATIVE_MODULE_SLOT_SIZE) {
        return NATIVE_MODULE_SLOT_COUNT;
    }
    return (unsigned)((address - NATIVE_MODULE_SLOT_ADDR) /
        NATIVE_MODULE_SLOT_SIZE);
}

typedef struct {
    uint32_t baud;
    uint8_t tx_id;
    uint8_t rx_id;
    uint8_t format;
    uint8_t active;
} uart_port_state_t;

typedef struct {
    uart_port_state_t port[4];
} uart_state_t;

static UART_Regs *uart_regs(unsigned id)
{
    switch (id) {
        case 0: return UART0;
        case 1: return UART1;
        case 2: return UART2;
        default: return UART3;
    }
}

static uint8_t uart_owner(unsigned id)
{
    return id ? (uint8_t)(PIN_OWN_UART1 + id - 1u) : PIN_OWN_UART_APP0;
}

static uint8_t uart_resource(unsigned id)
{
    return (uint8_t)(BOARD_RES_UART1 + id - 1u);
}

static int route_pf(unsigned instance, const native_pin_t *pin,
    unsigned role, unsigned *pf)
{
    unsigned i;
    for (i = 0; i < sizeof(g3507_uart_routes) /
            sizeof(g3507_uart_routes[0]); i++) {
        const g3507_route_t *route = &g3507_uart_routes[i];
        if (route->instance == instance && route->iomux == pin->iomux &&
                route->role == role) {
            *pf = route->pf;
            return 0;
        }
    }
    return -1;
}

static int native_pin_id(const native_pin_t *pin)
{
    unsigned bit;
    for (bit = 0; bit < 32u; bit++) {
        if (pin->pin == (1u << bit)) {
            if (pin->port == (uintptr_t)GPIOA) return (int)bit;
            if (pin->port == (uintptr_t)GPIOB && bit < 28u) {
                return (int)(32u + bit);
            }
            break;
        }
    }
    return -1;
}

static void pin_name(uint8_t id, char name[5])
{
    unsigned number = id;
    name[0] = 'P';
    name[1] = 'A';
    if (number >= 32u) {
        name[1] = 'B';
        number -= 32u;
    }
    if (number >= 10u) {
        name[2] = (char)('0' + number / 10u);
        name[3] = (char)('0' + number % 10u);
        name[4] = 0;
    } else {
        name[2] = (char)('0' + number);
        name[3] = 0;
        name[4] = 0;
    }
}

static DL_UART_WORD_LENGTH word_length(unsigned bits)
{
    switch (bits) {
        case 5: return DL_UART_WORD_LENGTH_5_BITS;
        case 6: return DL_UART_WORD_LENGTH_6_BITS;
        case 7: return DL_UART_WORD_LENGTH_7_BITS;
        default: return DL_UART_WORD_LENGTH_8_BITS;
    }
}

static DL_UART_PARITY parity_mode(unsigned parity)
{
    if (parity == 1u) return DL_UART_PARITY_EVEN;
    if (parity == 2u) return DL_UART_PARITY_ODD;
    return DL_UART_PARITY_NONE;
}

static int uart_configure(unsigned id, const uart_port_state_t *port)
{
    char tx[5];
    char rx[5];
    native_pin_t tx_pin;
    native_pin_t rx_pin;
    unsigned tx_pf;
    unsigned rx_pf;
    unsigned bits = 5u + (port->format & 3u);
    unsigned parity = (port->format >> 3u) & 3u;
    UART_Regs *regs = uart_regs(id);
    DL_UART_ClockConfig clock = {
        .clockSel = DL_UART_CLOCK_BUSCLK,
        .divideRatio = DL_UART_CLOCK_DIVIDE_RATIO_1,
    };
    DL_UART_Config config = {
        .mode = DL_UART_MODE_NORMAL,
        .direction = DL_UART_DIRECTION_TX_RX,
        .flowControl = DL_UART_FLOW_CONTROL_NONE,
        .parity = parity_mode(parity),
        .wordLength = word_length(bits),
        .stopBits = (port->format & 4u) ? DL_UART_STOP_BITS_TWO
                                       : DL_UART_STOP_BITS_ONE,
    };

    pin_name(port->tx_id, tx);
    pin_name(port->rx_id, rx);
    if (NATIVE_CORE_API->pin_resolve(tx, &tx_pin) != 0 ||
            NATIVE_CORE_API->pin_resolve(rx, &rx_pin) != 0 ||
            route_pf(id, &tx_pin, G3507_UART_TX, &tx_pf) != 0 ||
            route_pf(id, &rx_pin, G3507_UART_RX, &rx_pf) != 0 ||
            NATIVE_CORE_API->pin_af(tx, tx_pf, 0) != 0 ||
            NATIVE_CORE_API->pin_af(rx, rx_pf, 1) != 0) return -1;

    DL_UART_reset(regs);
    DL_UART_enablePower(regs);
    delay_cycles(16);
    DL_UART_setClockConfig(regs, &clock);
    DL_UART_init(regs, &config);
    DL_UART_configBaudRate(regs, NATIVE_CORE_API->bus_clock_hz(), port->baud);
    DL_UART_enableFIFOs(regs);
    DL_UART_setRXFIFOThreshold(regs, DL_UART_RX_FIFO_LEVEL_ONE_ENTRY);
    DL_UART_enable(regs);
    return 0;
}

static void uart_disconnect_pins(const uart_port_state_t *port)
{
    char tx[5];
    char rx[5];
    pin_name(port->tx_id, tx);
    pin_name(port->rx_id, rx);
    (void)NATIVE_CORE_API->pin_af(tx, 0, 0);
    (void)NATIVE_CORE_API->pin_af(rx, 0, 0);
}

static int uart_write_bytes(UART_Regs *regs, const uint8_t *data, size_t size)
{
    size_t i;
    for (i = 0; i < size; i++) {
        uint32_t spins = UART_WAIT_SPINS;
        while (DL_UART_isTXFIFOFull(regs)) {
            if (!--spins) return -1;
        }
        DL_UART_transmitData(regs, data[i]);
    }
    {
        uint32_t spins = UART_WAIT_SPINS;
        while (DL_UART_isBusy(regs)) {
            if (!--spins) return -1;
        }
    }
    return 0;
}

static void uart0_restore_console(const uart_port_state_t *port)
{
    NATIVE_CORE_API->uart0_release();
    /* MSPM0 permits multiple pins to select one peripheral input. Leaving the
     * temporary RX route connected makes it contend with console PA11. */
    uart_disconnect_pins(port);
}

static void uart_close_port(uart_state_t *state, unsigned id)
{
    uart_port_state_t *port;
    char tx[5];
    char rx[5];
    uint8_t owner;
    if (id > 3u || !state->port[id].active) return;
    port = &state->port[id];
    owner = uart_owner(id);
    if (id) {
        DL_UART_disable(uart_regs(id));
        DL_UART_reset(uart_regs(id));
        DL_UART_disablePower(uart_regs(id));
        NATIVE_CORE_API->resource_release(uart_resource(id), owner);
    }
    pin_name(port->tx_id, tx);
    pin_name(port->rx_id, rx);
    uart_disconnect_pins(port);
    NATIVE_CORE_API->pin_release(tx, owner);
    NATIVE_CORE_API->pin_release(rx, owner);
    port->active = 0;
}

static int l_uart_open(lua_State *L)
{
    static const char *const defaults[8] = {
        "PA0", "PA1", "PA17", "PA18",
        "PA23", "PA24", "PA26", "PA25",
    };
    uart_state_t *state = (uart_state_t *)NATIVE_CORE_API->module_state(
        uart_slot(), sizeof(uart_state_t));
    unsigned id = (unsigned)NATIVE_CORE_API->check_integer(L, 1);
    const char *tx;
    const char *rx;
    const char *parity_name;
    native_pin_t tx_pin;
    native_pin_t rx_pin;
    uart_port_state_t candidate;
    uint32_t baud;
    unsigned bits;
    unsigned stop;
    unsigned pf;
    unsigned parity;
    uint8_t owner;
    int tx_id;
    int rx_id;
    int acquired0 = 0;

    if (!state || id > 3u) return NATIVE_CORE_API->raise_error(L, "uart:id");
    tx = NATIVE_CORE_API->opt_string(L, 2, defaults[id * 2u]);
    rx = NATIVE_CORE_API->opt_string(L, 3, defaults[id * 2u + 1u]);
    baud = (uint32_t)NATIVE_CORE_API->opt_integer(L, 4, 115200);
    bits = (unsigned)NATIVE_CORE_API->opt_integer(L, 5, 8);
    parity_name = NATIVE_CORE_API->opt_string(L, 6, "none");
    stop = (unsigned)NATIVE_CORE_API->opt_integer(L, 7, 1);
    if (same_string(parity_name, "none")) parity = 0u;
    else if (same_string(parity_name, "even")) parity = 1u;
    else if (same_string(parity_name, "odd")) parity = 2u;
    else return NATIVE_CORE_API->raise_error(L, "uart:config");
    if (baud < 1200u || baud > 3000000u || bits < 5u || bits > 8u ||
            stop < 1u || stop > 2u ||
            NATIVE_CORE_API->pin_resolve(tx, &tx_pin) != 0 ||
            NATIVE_CORE_API->pin_resolve(rx, &rx_pin) != 0 ||
            route_pf(id, &tx_pin, G3507_UART_TX, &pf) != 0 ||
            route_pf(id, &rx_pin, G3507_UART_RX, &pf) != 0 ||
            tx_pin.iomux == rx_pin.iomux ||
            (id == 0u && ((NATIVE_CORE_API->pin_policy(tx) |
                          NATIVE_CORE_API->pin_policy(rx)) & PIN_POL_CONSOLE))) {
        return NATIVE_CORE_API->raise_error(L, "uart:config");
    }
    tx_id = native_pin_id(&tx_pin);
    rx_id = native_pin_id(&rx_pin);
    if (tx_id < 0 || rx_id < 0) return NATIVE_CORE_API->raise_error(L, "uart:pin");
    uart_close_port(state, id);
    owner = uart_owner(id);
    if (id && NATIVE_CORE_API->resource_claim(uart_resource(id), owner) != 0) {
        return NATIVE_CORE_API->raise_error(L, "uart:busy");
    }
    if (NATIVE_CORE_API->pin_claim(tx, owner) != 0 ||
            NATIVE_CORE_API->pin_claim(rx, owner) != 0) {
        NATIVE_CORE_API->pin_release(tx, owner);
        NATIVE_CORE_API->pin_release(rx, owner);
        if (id) NATIVE_CORE_API->resource_release(uart_resource(id), owner);
        return NATIVE_CORE_API->raise_error(L, "uart:busy");
    }
    candidate.baud = baud;
    candidate.tx_id = (uint8_t)tx_id;
    candidate.rx_id = (uint8_t)rx_id;
    candidate.format = (uint8_t)((bits - 5u) | ((stop - 1u) << 2u) |
        (parity << 3u));
    candidate.active = 1;
    if (id == 0u) {
        if (NATIVE_CORE_API->uart0_acquire() != 0) goto fail;
        acquired0 = 1;
    }
    if (uart_configure(id, &candidate) != 0) goto fail;
    state->port[id] = candidate;
    if (acquired0) uart0_restore_console(&candidate);
    return 0;

fail:
    if (acquired0) uart0_restore_console(&candidate);
    else uart_disconnect_pins(&candidate);
    NATIVE_CORE_API->pin_release(tx, owner);
    NATIVE_CORE_API->pin_release(rx, owner);
    if (id) NATIVE_CORE_API->resource_release(uart_resource(id), owner);
    return NATIVE_CORE_API->raise_error(L, "uart:open");
}

static int l_uart_close(lua_State *L)
{
    uart_state_t *state = (uart_state_t *)NATIVE_CORE_API->module_state(
        uart_slot(), sizeof(uart_state_t));
    unsigned id = (unsigned)NATIVE_CORE_API->check_integer(L, 1);
    if (!state || id > 3u) return NATIVE_CORE_API->raise_error(L, "uart:id");
    uart_close_port(state, id);
    return 0;
}

static int l_uart_tx(lua_State *L)
{
    uart_state_t *state = (uart_state_t *)NATIVE_CORE_API->module_state(
        uart_slot(), sizeof(uart_state_t));
    unsigned id = (unsigned)NATIVE_CORE_API->check_integer(L, 1);
    size_t size;
    const uint8_t *data = (const uint8_t *)NATIVE_CORE_API->check_lstring(
        L, 2, &size);
    UART_Regs *regs;
    int acquired0 = 0;
    int ok;
    if (!state || id > 3u || !state->port[id].active) {
        return NATIVE_CORE_API->raise_error(L, "uart:closed");
    }
    regs = uart_regs(id);
    if (id == 0u) {
        if (NATIVE_CORE_API->uart0_acquire() != 0 ||
                uart_configure(0, &state->port[0]) != 0) {
            NATIVE_CORE_API->uart0_release();
            uart_disconnect_pins(&state->port[0]);
            return NATIVE_CORE_API->raise_error(L, "uart0:acquire");
        }
        acquired0 = 1;
    }
    ok = uart_write_bytes(regs, data, size) == 0;
    if (acquired0) uart0_restore_console(&state->port[0]);
    if (!ok) return NATIVE_CORE_API->raise_error(L, "uart:timeout");
    return 0;
}

static int l_uart_rx(lua_State *L)
{
    uint8_t data[UART_MAX_READ];
    uart_state_t *state = (uart_state_t *)NATIVE_CORE_API->module_state(
        uart_slot(), sizeof(uart_state_t));
    unsigned id = (unsigned)NATIVE_CORE_API->check_integer(L, 1);
    int32_t timeout_value = NATIVE_CORE_API->opt_integer(L, 2, 0);
    uint32_t timeout = (uint32_t)timeout_value;
    int32_t requested = NATIVE_CORE_API->opt_integer(L, 3, 64);
    uint32_t started = NATIVE_CORE_API->millis();
    UART_Regs *regs;
    size_t size = 0;
    int acquired0 = 0;
    if (!state || id > 3u || !state->port[id].active || timeout_value < 0 ||
            requested < 1 ||
            requested > (int32_t)UART_MAX_READ) {
        return NATIVE_CORE_API->raise_error(L, "uart:read");
    }
    regs = uart_regs(id);
    if (id == 0u) {
        if (NATIVE_CORE_API->uart0_acquire() != 0 ||
                uart_configure(0, &state->port[0]) != 0) {
            NATIVE_CORE_API->uart0_release();
            uart_disconnect_pins(&state->port[0]);
            return NATIVE_CORE_API->raise_error(L, "uart0:acquire");
        }
        acquired0 = 1;
    }
    while (size < (size_t)requested) {
        if (!DL_UART_isRXFIFOEmpty(regs)) {
            data[size++] = DL_UART_receiveData(regs);
            continue;
        }
        if (size || (uint32_t)(NATIVE_CORE_API->millis() - started) >= timeout) {
            break;
        }
    }
    if (acquired0) uart0_restore_console(&state->port[0]);
    if (!size) return 0;
    NATIVE_CORE_API->push_lstring(L, (const char *)data, size);
    return 1;
}

static int l_uart_valid(lua_State *L)
{
    native_pin_t tx;
    native_pin_t rx;
    unsigned pf;
    unsigned id = (unsigned)NATIVE_CORE_API->check_integer(L, 1);
    const char *tx_name = NATIVE_CORE_API->check_string(L, 2);
    const char *rx_name = NATIVE_CORE_API->check_string(L, 3);
    int ok = id <= 3u &&
        NATIVE_CORE_API->pin_resolve(tx_name, &tx) == 0 &&
        NATIVE_CORE_API->pin_resolve(rx_name, &rx) == 0 &&
        tx.iomux != rx.iomux &&
        route_pf(id, &tx, G3507_UART_TX, &pf) == 0 &&
        route_pf(id, &rx, G3507_UART_RX, &pf) == 0 &&
        (id != 0u || ((NATIVE_CORE_API->pin_policy(tx_name) |
                      NATIVE_CORE_API->pin_policy(rx_name)) &
                     PIN_POL_CONSOLE) == 0u);
    NATIVE_CORE_API->push_boolean(L, ok);
    return 1;
}

static const native_lua_reg_t k_uart_functions[] = {
    {"open", l_uart_open}, {"close", l_uart_close},
    {"tx", l_uart_tx}, {"rx", l_uart_rx},
    {"valid", l_uart_valid}, {0, 0},
};

static int uart_init(lua_State *L, const native_core_api_t *api)
{
    uart_state_t *state;
    unsigned i;
    if (!api || api->magic != NATIVE_CORE_API_MAGIC ||
            api->abi_version != NATIVE_CORE_ABI_VERSION ||
            api->struct_size < sizeof(native_core_api_t)) return -1;
    state = (uart_state_t *)api->module_state(uart_slot(), sizeof(*state));
    if (!state) return -1;
    for (i = 0; i < 4u; i++) state->port[i].active = 0;
    return api->register_lua_module(L, "uart", k_uart_functions);
}

static void uart_deinit(void)
{
    uart_state_t *state = (uart_state_t *)NATIVE_CORE_API->module_state(
        uart_slot(), sizeof(uart_state_t));
    unsigned id;
    if (!state) return;
    for (id = 0; id < 4u; id++) uart_close_port(state, id);
}

const native_module_header_t g_native_module_header
    __attribute__((section(".module_header"), used, aligned(4))) = {
        NATIVE_MODULE_MAGIC, NATIVE_MODULE_FORMAT, NATIVE_CORE_ABI_VERSION,
        0, 0, sizeof(native_module_header_t), uart_init, uart_deinit, "uart",
    };
