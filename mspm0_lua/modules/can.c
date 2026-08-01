#include "native_module.h"

#include "board_pins.h"
#include "board_resource.h"
#include "g3507_pin_routes.h"
#include <ti/driverlib/driverlib.h>

#define CAN_FIELD(value, field) \
    ((((uint32_t)(value)) << field##_OFS) & field##_MASK)
#define CAN_GET(value, field) \
    ((((uint32_t)(value)) & field##_MASK) >> field##_OFS)

extern const native_module_header_t g_native_module_header;

static unsigned can_slot(void)
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
    uint8_t active;
    uint8_t tx_id;
    uint8_t rx_id;
    uint8_t reserved;
} can_state_t;

static int route_pf(const native_pin_t *pin, unsigned role, unsigned *pf)
{
    unsigned i;
    for (i = 0; i < sizeof(g3507_can_routes) /
            sizeof(g3507_can_routes[0]); i++) {
        const g3507_route_t *route = &g3507_can_routes[i];
        if (route->iomux == pin->iomux && route->role == role) {
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
        unsigned tens = NATIVE_CORE_API->udiv32(number, 10u);
        name[2] = (char)('0' + tens);
        name[3] = (char)('0' + number - tens * 10u);
        name[4] = 0;
    } else {
        name[2] = (char)('0' + number);
        name[3] = 0;
        name[4] = 0;
    }
}

static void can_disconnect_pins(const char *tx, const char *rx)
{
    (void)NATIVE_CORE_API->pin_af(tx, 0, 0);
    (void)NATIVE_CORE_API->pin_af(rx, 0, 0);
}

static int wait_mode(uint32_t mode)
{
    uint32_t spins = 800000u;
    while (CAN_GET(CANFD0->MCANSS.MCAN.MCAN_CCCR, MCAN_CCCR_INIT) != mode) {
        if (!--spins) return -1;
    }
    return 0;
}

static void can_hardware_close(void)
{
    CANFD0->MCANSS.MCAN.MCAN_CCCR |= MCAN_CCCR_INIT_MASK;
    (void)wait_mode(DL_MCAN_OPERATION_MODE_SW_INIT);
    DL_MCAN_disableModuleClock(CANFD0);
    DL_MCAN_disablePower(CANFD0);
}

static void can_close_state(can_state_t *state)
{
    char tx[5];
    char rx[5];
    if (!state || !state->active) return;
    can_hardware_close();
    pin_name(state->tx_id, tx);
    pin_name(state->rx_id, rx);
    can_disconnect_pins(tx, rx);
    NATIVE_CORE_API->pin_release(tx, PIN_OWN_CAN);
    NATIVE_CORE_API->pin_release(rx, PIN_OWN_CAN);
    NATIVE_CORE_API->resource_release(BOARD_RES_CAN, PIN_OWN_CAN);
    state->active = 0;
}

static int can_config_valid(uint32_t bitrate)
{
    return NATIVE_CORE_API->bus_clock_hz() == 40000000u &&
        (bitrate == 125000u || bitrate == 250000u ||
         bitrate == 500000u || bitrate == 1000000u);
}

static int can_hardware_open(uint32_t bitrate, int loopback)
{
    uint32_t ready_wait = 1600000u;
    uint32_t mem_wait = 1600000u;
    uint32_t clock_wait = 160000u;
    uint32_t prescaler;

    prescaler = NATIVE_CORE_API->udiv32(1000000u, bitrate) - 1u;
    DL_MCAN_disableModuleClock(CANFD0);
    DL_MCAN_disablePower(CANFD0);
    delay_cycles(80000u);
    DL_MCAN_reset(CANFD0);
    DL_MCAN_enablePower(CANFD0);
    delay_cycles(80000u);
    DL_MCAN_enableModuleClock(CANFD0);
    SYSCTL->SOCLOCK.GENCLKCFG =
        (SYSCTL->SOCLOCK.GENCLKCFG & ~SYSCTL_GENCLKCFG_CANCLKSRC_MASK) |
        DL_MCAN_FCLK_HFCLK;
    CANFD0->MCANSS.TI_WRAPPER.MSP.MCANSS_CLKDIV = DL_MCAN_FCLK_DIV_1;
    DL_MCAN_disableClockStopGateRequest(CANFD0);
    CANFD0->MCANSS.MCAN.MCAN_CCCR &= ~MCAN_CCCR_CSR_MASK;
    while (!DL_MCAN_isModuleClockEnabled(CANFD0) ||
            !DL_MCAN_getControllerClockRequestStatus(CANFD0)) {
        DL_MCAN_enableModuleClock(CANFD0);
        if (!--clock_wait) return -2;
    }
    while ((SYSCTL->SOCLOCK.SYSSTATUS & DL_MCAN_INSTANCE_0) == 0u) {
        if (!--ready_wait) return -3;
    }
    while ((CANFD0->MCANSS.TI_WRAPPER.PROCESSORS.MCANSS_REGS.MCANSS_STAT &
            MCAN_TI_WRAPPER_PROCESSORS_REGS_STAT_MEM_INIT_DONE_MASK) == 0u) {
        if (!--mem_wait) return -4;
    }
    CANFD0->MCANSS.MCAN.MCAN_CCCR |= MCAN_CCCR_INIT_MASK;
    if (wait_mode(DL_MCAN_OPERATION_MODE_SW_INIT)) return -5;
    CANFD0->MCANSS.MCAN.MCAN_CCCR |= MCAN_CCCR_CCE_MASK;
    CANFD0->MCANSS.TI_WRAPPER.PROCESSORS.MCANSS_REGS.MCANSS_CTRL =
        MCAN_TI_WRAPPER_REGS_CTRL_DBGSUSP_FREE_MASK;
    CANFD0->MCANSS.MCAN.MCAN_RWD = 255u;
    CANFD0->MCANSS.MCAN.MCAN_TDCR = 0u;
    CANFD0->MCANSS.MCAN.MCAN_DBTP =
        CAN_FIELD(4, MCAN_DBTP_DSJW) |
        CAN_FIELD(4, MCAN_DBTP_DTSEG2) |
        CAN_FIELD(13, MCAN_DBTP_DTSEG1) |
        CAN_FIELD(1, MCAN_DBTP_DBRP);
    CANFD0->MCANSS.MCAN.MCAN_NBTP =
        CAN_FIELD(4, MCAN_NBTP_NSJW) |
        CAN_FIELD(4, MCAN_NBTP_NTSEG2) |
        CAN_FIELD(33, MCAN_NBTP_NTSEG1) |
        CAN_FIELD(prescaler, MCAN_NBTP_NBRP);
    CANFD0->MCANSS.MCAN.MCAN_GFC =
        MCAN_GFC_RRFE_MASK | MCAN_GFC_RRFS_MASK;
    CANFD0->MCANSS.MCAN.MCAN_SIDFC = 0u;
    CANFD0->MCANSS.MCAN.MCAN_XIDFC = 0u;
    CANFD0->MCANSS.MCAN.MCAN_XIDAM = 0x1fffffffu;
    CANFD0->MCANSS.MCAN.MCAN_RXF0C =
        CAN_FIELD(172u >> 2u, MCAN_RXF0C_F0SA) |
        CAN_FIELD(3, MCAN_RXF0C_F0S);
    CANFD0->MCANSS.MCAN.MCAN_RXF1C = 0u;
    CANFD0->MCANSS.MCAN.MCAN_RXBC =
        CAN_FIELD(208u >> 2u, MCAN_RXBC_RBSA);
    CANFD0->MCANSS.MCAN.MCAN_RXESC = 0u;
    CANFD0->MCANSS.MCAN.MCAN_TXBC =
        CAN_FIELD(148u >> 2u, MCAN_TXBC_TBSA) |
        CAN_FIELD(1, MCAN_TXBC_NDTB);
    CANFD0->MCANSS.MCAN.MCAN_TXESC = 0u;
    CANFD0->MCANSS.MCAN.MCAN_TXEFC = 0u;
    if (loopback) {
        CANFD0->MCANSS.MCAN.MCAN_CCCR |=
            MCAN_CCCR_TEST_MASK | MCAN_CCCR_MON_MASK;
        CANFD0->MCANSS.MCAN.MCAN_TEST = MCAN_TEST_LBCK_MASK;
    }
    CANFD0->MCANSS.MCAN.MCAN_CCCR &= ~MCAN_CCCR_CCE_MASK;
    CANFD0->MCANSS.MCAN.MCAN_CCCR &= ~MCAN_CCCR_INIT_MASK;
    return wait_mode(DL_MCAN_OPERATION_MODE_NORMAL);
}

static int can_open_on(lua_State *L, const char *tx, const char *rx,
    uint32_t bitrate, int loopback)
{
    can_state_t *state = (can_state_t *)NATIVE_CORE_API->module_state(
        can_slot(), sizeof(can_state_t));
    native_pin_t tx_pin;
    native_pin_t rx_pin;
    unsigned tx_pf;
    unsigned rx_pf;
    int tx_id;
    int rx_id;
    if (!state || !can_config_valid(bitrate)) {
        return NATIVE_CORE_API->raise_error(L, "can:config");
    }
    if (NATIVE_CORE_API->pin_resolve(tx, &tx_pin) != 0 ||
            NATIVE_CORE_API->pin_resolve(rx, &rx_pin) != 0 ||
            route_pf(&tx_pin, G3507_CAN_TX, &tx_pf) != 0 ||
            route_pf(&rx_pin, G3507_CAN_RX, &rx_pf) != 0 ||
            tx_pin.iomux == rx_pin.iomux) {
        return NATIVE_CORE_API->raise_error(L, "can:route");
    }
    tx_id = native_pin_id(&tx_pin);
    rx_id = native_pin_id(&rx_pin);
    if (tx_id < 0 || rx_id < 0) return NATIVE_CORE_API->raise_error(L, "can:pin");
    can_close_state(state);
    if (NATIVE_CORE_API->resource_claim(BOARD_RES_CAN, PIN_OWN_CAN) != 0 ||
            NATIVE_CORE_API->pin_claim(tx, PIN_OWN_CAN) != 0 ||
            NATIVE_CORE_API->pin_claim(rx, PIN_OWN_CAN) != 0) {
        NATIVE_CORE_API->pin_release(tx, PIN_OWN_CAN);
        NATIVE_CORE_API->pin_release(rx, PIN_OWN_CAN);
        NATIVE_CORE_API->resource_release(BOARD_RES_CAN, PIN_OWN_CAN);
        return NATIVE_CORE_API->raise_error(L, "can:busy");
    }
    if (NATIVE_CORE_API->pin_af(tx, tx_pf, 0) != 0 ||
            NATIVE_CORE_API->pin_af(rx, rx_pf, 1) != 0) {
        can_disconnect_pins(tx, rx);
        NATIVE_CORE_API->pin_release(tx, PIN_OWN_CAN);
        NATIVE_CORE_API->pin_release(rx, PIN_OWN_CAN);
        NATIVE_CORE_API->resource_release(BOARD_RES_CAN, PIN_OWN_CAN);
        return NATIVE_CORE_API->raise_error(L, "can:open");
    }
    if (can_hardware_open(bitrate, loopback) != 0) {
        can_hardware_close();
        can_disconnect_pins(tx, rx);
        NATIVE_CORE_API->pin_release(tx, PIN_OWN_CAN);
        NATIVE_CORE_API->pin_release(rx, PIN_OWN_CAN);
        NATIVE_CORE_API->resource_release(BOARD_RES_CAN, PIN_OWN_CAN);
        return NATIVE_CORE_API->raise_error(L, "can:open");
    }
    state->tx_id = (uint8_t)tx_id;
    state->rx_id = (uint8_t)rx_id;
    state->active = 1;
    return 0;
}

static int l_can_open(lua_State *L)
{
    uint32_t bitrate = (uint32_t)NATIVE_CORE_API->opt_integer(L, 1, 500000);
    int loopback = NATIVE_CORE_API->to_boolean(L, 2);
    const char *tx = NATIVE_CORE_API->opt_string(L, 3, "PA26");
    const char *rx = NATIVE_CORE_API->opt_string(L, 4, "PA27");
    return can_open_on(L, tx, rx, bitrate, loopback);
}

static int l_can_open_on(lua_State *L)
{
    return can_open_on(L, NATIVE_CORE_API->check_string(L, 1),
        NATIVE_CORE_API->check_string(L, 2),
        (uint32_t)NATIVE_CORE_API->opt_integer(L, 3, 500000),
        NATIVE_CORE_API->to_boolean(L, 4));
}

static int l_can_close(lua_State *L)
{
    can_state_t *state = (can_state_t *)NATIVE_CORE_API->module_state(
        can_slot(), sizeof(can_state_t));
    (void)L;
    can_close_state(state);
    return 0;
}

static int l_can_send(lua_State *L)
{
    can_state_t *state = (can_state_t *)NATIVE_CORE_API->module_state(
        can_slot(), sizeof(can_state_t));
    volatile uint32_t *message = (volatile uint32_t *)
        ((uintptr_t)CANFD0 + 148u);
    uint32_t id = (uint32_t)NATIVE_CORE_API->check_integer(L, 1);
    size_t size;
    const uint8_t *data = (const uint8_t *)NATIVE_CORE_API->check_lstring(
        L, 2, &size);
    int32_t timeout_value = NATIVE_CORE_API->opt_integer(L, 3, 100);
    uint32_t timeout = (uint32_t)timeout_value;
    int extended = NATIVE_CORE_API->to_boolean(L, 4);
    uint32_t started;
    size_t i;
    int ok;
    if (!state || !state->active || timeout_value < 0 || size > 8u ||
            id > (extended ? 0x1fffffffu : 0x7ffu) ||
            (CANFD0->MCANSS.MCAN.MCAN_TXBRP & 1u)) {
        NATIVE_CORE_API->push_boolean(L, 0);
        return 1;
    }
    message[0] = extended ? id | (1u << 30u) : id << 18u;
    message[1] = (uint32_t)size << 16u;
    for (i = 0; i < 2u; i++) {
        size_t offset = i * 4u;
        uint32_t word = 0;
        unsigned j;
        for (j = 0; j < 4u && offset + j < size; j++) {
            word |= (uint32_t)data[offset + j] << (j * 8u);
        }
        message[2u + i] = word;
    }
    CANFD0->MCANSS.MCAN.MCAN_TXBAR = 1u;
    started = NATIVE_CORE_API->millis();
    while (CANFD0->MCANSS.MCAN.MCAN_TXBRP & 1u) {
        if ((uint32_t)(NATIVE_CORE_API->millis() - started) >= timeout) {
            CANFD0->MCANSS.MCAN.MCAN_TXBCR = 1u;
            NATIVE_CORE_API->push_boolean(L, 0);
            return 1;
        }
    }
    ok = (CANFD0->MCANSS.MCAN.MCAN_TXBTO & 1u) != 0;
    NATIVE_CORE_API->push_boolean(L, ok);
    return 1;
}

static int l_can_recv(lua_State *L)
{
    can_state_t *state = (can_state_t *)NATIVE_CORE_API->module_state(
        can_slot(), sizeof(can_state_t));
    volatile uint32_t *message;
    uint8_t data[8];
    int32_t timeout_value = NATIVE_CORE_API->opt_integer(L, 1, 0);
    uint32_t timeout = (uint32_t)timeout_value;
    uint32_t started = NATIVE_CORE_API->millis();
    uint32_t status;
    uint32_t header0;
    uint32_t header1;
    uint32_t index;
    size_t size;
    size_t i;
    if (!state || !state->active) return NATIVE_CORE_API->raise_error(L, "can:closed");
    if (timeout_value < 0) return NATIVE_CORE_API->raise_error(L, "can:timeout");
    do {
        status = CANFD0->MCANSS.MCAN.MCAN_RXF0S;
        if (CAN_GET(status, MCAN_RXF0S_F0FL)) break;
    } while ((uint32_t)(NATIVE_CORE_API->millis() - started) < timeout);
    if (!CAN_GET(status, MCAN_RXF0S_F0FL)) return 0;
    index = CAN_GET(status, MCAN_RXF0S_F0GI);
    message = (volatile uint32_t *)((uintptr_t)CANFD0 + 172u + index * 16u);
    header0 = message[0];
    header1 = message[1];
    size = (header1 >> 16u) & 15u;
    if (size > 8u) size = 8u;
    for (i = 0; i < size; i++) {
        data[i] = (uint8_t)(message[2u + i / 4u] >> ((i & 3u) * 8u));
    }
    CANFD0->MCANSS.MCAN.MCAN_RXF0A = index;
    if ((header0 & (1u << 29u)) || (header1 & (1u << 21u))) {
        return NATIVE_CORE_API->raise_error(L, "can:frame");
    }
    NATIVE_CORE_API->push_integer(L,
        (int32_t)((header0 & (1u << 30u)) ? header0 & 0x1fffffffu
                                         : (header0 >> 18u) & 0x7ffu));
    NATIVE_CORE_API->push_lstring(L, (const char *)data, size);
    NATIVE_CORE_API->push_boolean(L, (header0 & (1u << 30u)) != 0);
    return 3;
}

static int l_can_valid(lua_State *L)
{
    native_pin_t tx;
    native_pin_t rx;
    unsigned pf;
    int ok = NATIVE_CORE_API->pin_resolve(
            NATIVE_CORE_API->check_string(L, 1), &tx) == 0 &&
        NATIVE_CORE_API->pin_resolve(
            NATIVE_CORE_API->check_string(L, 2), &rx) == 0 &&
        route_pf(&tx, G3507_CAN_TX, &pf) == 0 &&
        route_pf(&rx, G3507_CAN_RX, &pf) == 0 && tx.iomux != rx.iomux;
    NATIVE_CORE_API->push_boolean(L, ok);
    return 1;
}

static const native_lua_reg_t k_can_functions[] = {
    {"open", l_can_open}, {"open_on", l_can_open_on},
    {"close", l_can_close}, {"send", l_can_send},
    {"recv", l_can_recv}, {"valid", l_can_valid}, {0, 0},
};

static int can_init(lua_State *L, const native_core_api_t *api)
{
    can_state_t *state;
    if (!api || api->magic != NATIVE_CORE_API_MAGIC ||
            api->abi_version != NATIVE_CORE_ABI_VERSION ||
            api->struct_size < sizeof(native_core_api_t)) return -1;
    state = (can_state_t *)api->module_state(can_slot(), sizeof(*state));
    if (!state) return -1;
    state->active = 0;
    return api->register_lua_module(L, "can", k_can_functions);
}

static void can_deinit(void)
{
    can_close_state((can_state_t *)NATIVE_CORE_API->module_state(
        can_slot(), sizeof(can_state_t)));
}

const native_module_header_t g_native_module_header
    __attribute__((section(".module_header"), used, aligned(4))) = {
        NATIVE_MODULE_MAGIC, NATIVE_MODULE_FORMAT, NATIVE_CORE_ABI_VERSION,
        0, 0, sizeof(native_module_header_t), can_init, can_deinit, "can",
    };
