#include "native_module.h"

#include <stddef.h>

#include "board_crc.h"
#ifdef MSPM0_MODULAR_CORE
#include "board_delay.h"
#include "board_dma.h"
#include "board_irq.h"
#include "board_pins.h"
#include "board_resource.h"
#endif
#include "board_status.h"
#include "board_uart.h"
#include "ti_msp_dl_config.h"
#include "lua.h"
#include "lauxlib.h"

static uint8_t s_loaded_slots;

#ifdef MSPM0_MODULAR_CORE
extern uint32_t __udivsi3(uint32_t numerator, uint32_t denominator);

static uint32_t s_module_state[NATIVE_MODULE_SLOT_COUNT]
    [NATIVE_MODULE_STATE_SIZE / sizeof(uint32_t)];

static void *core_module_state(unsigned slot, size_t size)
{
    if (slot >= NATIVE_MODULE_SLOT_COUNT || size > NATIVE_MODULE_STATE_SIZE) {
        return 0;
    }
    return s_module_state[slot];
}
#endif

static int core_register_lua_module(lua_State *L, const char *name,
    const native_lua_reg_t *functions)
{
    const native_lua_reg_t *item;
    if (!L || !name || !name[0] || !functions) {
        return -1;
    }
    lua_createtable(L, 0, 4);
    for (item = functions; item->name && item->function; item++) {
        lua_pushcclosure(L, item->function, 0);
        lua_setfield(L, -2, item->name);
    }
    lua_setglobal(L, name);
    return 0;
}

static void core_push_integer(lua_State *L, int32_t value)
{
    lua_pushinteger(L, (lua_Integer)value);
}

#ifdef MSPM0_MODULAR_CORE
static int32_t core_check_integer(lua_State *L, int index)
{
    return (int32_t)luaL_checkinteger(L, index);
}

static int32_t core_opt_integer(lua_State *L, int index, int32_t fallback)
{
    return (int32_t)luaL_optinteger(L, index, fallback);
}

static const char *core_check_string(lua_State *L, int index)
{
    return luaL_checkstring(L, index);
}

static const char *core_opt_string(lua_State *L, int index,
    const char *fallback)
{
    return luaL_optstring(L, index, fallback);
}

static void core_push_boolean(lua_State *L, int value)
{
    lua_pushboolean(L, value);
}

static int core_to_boolean(lua_State *L, int index)
{
    return lua_toboolean(L, index);
}

static const char *core_check_lstring(lua_State *L, int index, size_t *length)
{
    return luaL_checklstring(L, index, length);
}

static void core_push_lstring(lua_State *L, const char *data, size_t length)
{
    lua_pushlstring(L, data, length);
}

static uint32_t core_bus_clock_hz(void)
{
    return g_uart_busclk_hz ? g_uart_busclk_hz : 32000000u;
}

static int core_raise_error(lua_State *L, const char *message)
{
    return luaL_error(L, "%s", message ? message : "module");
}

static int core_pin_resolve(const char *name, native_pin_t *pin)
{
    board_pin_t resolved;
    if (!pin || board_pin_resolve(name, &resolved) != 0) {
        return -1;
    }
    pin->port = (uintptr_t)resolved.port;
    pin->pin = resolved.pin;
    pin->iomux = resolved.iomux;
    return 0;
}

static int core_pin_claim(const char *name, uint8_t owner)
{
    return board_pin_claim(name, owner, 0);
}

static void core_pin_release(const char *name, uint8_t owner)
{
    board_pin_release_owned(name, owner);
}

static int core_resource_claim(uint8_t resource, uint8_t owner)
{
    return board_resource_claim((board_resource_t)resource, owner);
}

static void core_resource_release(uint8_t resource, uint8_t owner)
{
    board_resource_release((board_resource_t)resource, owner);
}
#endif

const native_core_api_t g_native_core_api
    __attribute__((section(".core_api"), used, aligned(4))) = {
        NATIVE_CORE_API_MAGIC,
        NATIVE_CORE_ABI_VERSION,
        sizeof(native_core_api_t),
        core_register_lua_module,
        core_push_integer,
        board_uart_puts,
#ifdef MSPM0_MODULAR_CORE
        core_check_integer,
        core_opt_integer,
        core_check_string,
        core_opt_string,
        core_push_boolean,
        core_raise_error,
        core_pin_resolve,
        core_pin_claim,
        core_pin_release,
        board_millis,
        board_soft_timer_start,
        board_soft_timer_stop,
        board_soft_timer_take,
        board_delay_ms,
        core_check_lstring,
        core_push_lstring,
        core_bus_clock_hz,
        board_pin_af,
        core_resource_claim,
        core_resource_release,
        board_spi1_app_acquire,
        board_spi1_app_release,
        board_pin_owner,
        board_pin_policy,
        core_module_state,
        board_uart0_app_acquire,
        board_uart0_app_release,
        core_to_boolean,
        __udivsi3,
#endif
    };

static int header_name_valid(const native_module_header_t *header)
{
    unsigned i;
    for (i = 0; i < sizeof(header->name); i++) {
        if (header->name[i] == 0) {
            return i != 0;
        }
    }
    return 0;
}

static int native_module_register_slot(lua_State *L, unsigned slot)
{
    uintptr_t slot_addr = NATIVE_MODULE_SLOT_ADDR +
        (uintptr_t)slot * NATIVE_MODULE_SLOT_SIZE;
    const native_module_header_t *header =
        (const native_module_header_t *)slot_addr;
    uintptr_t entry;
    uintptr_t deinit;
    uint16_t crc;

    if (header->magic == 0xFFFFFFFFu) {
        return 0;
    }
    entry = (uintptr_t)header->init;
    deinit = (uintptr_t)header->deinit;
    if (header->magic != NATIVE_MODULE_MAGIC ||
            header->format_version != NATIVE_MODULE_FORMAT ||
            header->abi_version != NATIVE_CORE_ABI_VERSION ||
            header->header_size != sizeof(native_module_header_t) ||
            header->image_size <= sizeof(native_module_header_t) ||
            header->image_size > NATIVE_MODULE_SLOT_SIZE ||
            !header_name_valid(header) || (entry & 1u) == 0 ||
            (entry & ~(uintptr_t)1u) <
                slot_addr + sizeof(native_module_header_t) ||
            (entry & ~(uintptr_t)1u) >=
                slot_addr + header->image_size ||
            (deinit != 0u && ((deinit & 1u) == 0u ||
                (deinit & ~(uintptr_t)1u) <
                    slot_addr + sizeof(native_module_header_t) ||
                (deinit & ~(uintptr_t)1u) >=
                    slot_addr + header->image_size))) {
        board_uart_puts("MOD bad\n");
        return -1;
    }
    crc = board_crc16_modbus(
        (const uint8_t *)(slot_addr +
            sizeof(native_module_header_t)),
        header->image_size - sizeof(native_module_header_t));
    if (crc != header->payload_crc16) {
        board_uart_puts("MOD crc\n");
        return -1;
    }
    if (header->init(L, &g_native_core_api) != 0) {
        board_uart_puts("MOD init\n");
        return -1;
    }
    board_uart_puts("MOD ");
    board_uart_puts(header->name);
    board_uart_puts("\n");
    board_status_or(ST_NATIVE_MODULE_OK);
    board_status_or_raw(1u << slot);
    s_loaded_slots |= (uint8_t)(1u << slot);
    return 1;
}

int native_module_register(lua_State *L)
{
    int loaded = 0;
    unsigned slot;
    s_loaded_slots = 0;
    for (slot = 0; slot < NATIVE_MODULE_SLOT_COUNT; slot++) {
        int status = native_module_register_slot(L, slot);
        if (status > 0) {
            loaded++;
        }
    }
    return loaded;
}

void native_module_deinit(void)
{
    unsigned slot = NATIVE_MODULE_SLOT_COUNT;
    while (slot-- != 0u) {
        if (s_loaded_slots & (uint8_t)(1u << slot)) {
            const native_module_header_t *header =
                (const native_module_header_t *)(NATIVE_MODULE_SLOT_ADDR +
                    (uintptr_t)slot * NATIVE_MODULE_SLOT_SIZE);
            if (header->deinit) header->deinit();
        }
    }
    s_loaded_slots = 0;
}
