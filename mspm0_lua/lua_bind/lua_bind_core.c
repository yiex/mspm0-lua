#include "lua_bind.h"

#include "board_iq.h"
#include "board_irq.h"
#include "board_pins.h"
#include "board_resource.h"
#include "board_uart.h"
#include "board_wdt.h"

#include "lauxlib.h"

static volatile int s_stop;

void lua_bind_request_stop(void) { s_stop = 1; }
void lua_bind_clear_stop(void) { s_stop = 0; }
int lua_bind_stop_requested(void) { return s_stop; }

static void poll_stop_uart(void)
{
    for (;;) {
        int c = board_uart_peek_nonblock();
        if (c != '!') {
            break;
        }
        (void)board_uart_getc_nonblock();
        s_stop = 1;
    }
}

static void stop_hook(lua_State *L, lua_Debug *ar)
{
    (void)ar;
    poll_stop_uart();
    if (s_stop) {
        luaL_error(L, "STOP");
    }
}

static int l_delay_ms(lua_State *L)
{
    uint32_t ms = (uint32_t)luaL_checkinteger(L, 1);
    uint32_t start = board_millis();
    while ((uint32_t)(board_millis() - start) < ms) {
        board_wdt_feed();
        poll_stop_uart();
        if (s_stop) {
            return luaL_error(L, "STOP");
        }
    }
    return 0;
}

static int l_millis(lua_State *L)
{
    lua_pushinteger(L, (lua_Integer)board_millis());
    return 1;
}

static int l_yield(lua_State *L)
{
    board_wdt_feed();
    poll_stop_uart();
    if (s_stop) {
        return luaL_error(L, "STOP");
    }
    __WFI();
    return 0;
}

static int l_stopped(lua_State *L)
{
    poll_stop_uart();
    lua_pushboolean(L, s_stop != 0);
    return 1;
}

static int l_byte(lua_State *L)
{
    size_t length;
    const unsigned char *value =
        (const unsigned char *)luaL_checklstring(L, 1, &length);
    lua_Integer index = luaL_optinteger(L, 2, 1);
    if (index < 1 || (size_t)index > length) {
        lua_pushnil(L);
    } else {
        lua_pushinteger(L, value[index - 1]);
    }
    return 1;
}

/* IQ16 is part of the resident Core API, not a replaceable native module. */
static int l_iq_from(lua_State *L)
{
    lua_pushinteger(L, iq16_from_i((int32_t)luaL_checkinteger(L, 1)));
    return 1;
}

static int l_iq_from_x10(lua_State *L)
{
    lua_pushinteger(L, iq16_from_x10((int32_t)luaL_checkinteger(L, 1)));
    return 1;
}

static int l_iq_from_x100(lua_State *L)
{
    lua_pushinteger(L, iq16_from_x100((int32_t)luaL_checkinteger(L, 1)));
    return 1;
}

static int l_iq_to_x10(lua_State *L)
{
    lua_pushinteger(L, iq16_to_x10((iq16_t)luaL_checkinteger(L, 1)));
    return 1;
}

static int l_iq_to_x100(lua_State *L)
{
    lua_pushinteger(L, iq16_to_x100((iq16_t)luaL_checkinteger(L, 1)));
    return 1;
}

static int l_iq_to_x1000(lua_State *L)
{
    lua_pushinteger(L, iq16_to_x1000((iq16_t)luaL_checkinteger(L, 1)));
    return 1;
}

static int l_iq_mul(lua_State *L)
{
    lua_pushinteger(L, iq16_mul((iq16_t)luaL_checkinteger(L, 1),
        (iq16_t)luaL_checkinteger(L, 2)));
    return 1;
}

static int l_iq_div(lua_State *L)
{
    lua_pushinteger(L, iq16_div((iq16_t)luaL_checkinteger(L, 1),
        (iq16_t)luaL_checkinteger(L, 2)));
    return 1;
}

static int l_iq_sin_deg(lua_State *L)
{
    lua_pushinteger(L, iq16_sin_deg_x10((int32_t)luaL_checkinteger(L, 1)));
    return 1;
}

static int l_iq_cos_deg(lua_State *L)
{
    lua_pushinteger(L, iq16_cos_deg_x10((int32_t)luaL_checkinteger(L, 1)));
    return 1;
}

static int l_iq_atan2_deg(lua_State *L)
{
    lua_pushinteger(L, iq16_atan2_deg_x10((iq16_t)luaL_checkinteger(L, 1),
        (iq16_t)luaL_checkinteger(L, 2)));
    return 1;
}

static const luaL_Reg k_iq_functions[] = {
    {"from", l_iq_from},
    {"from_x10", l_iq_from_x10},
    {"from_x100", l_iq_from_x100},
    {"to_x10", l_iq_to_x10},
    {"to_x100", l_iq_to_x100},
    {"to_x1000", l_iq_to_x1000},
    {"mul", l_iq_mul},
    {"div", l_iq_div},
    {"sin_deg", l_iq_sin_deg},
    {"cos_deg", l_iq_cos_deg},
    {"atan2_deg", l_iq_atan2_deg},
    {NULL, NULL},
};

static const luaL_Reg k_core_globals[] = {
    {"delay_ms", l_delay_ms},
    {"millis", l_millis},
    {"yield", l_yield},
    {"stopped", l_stopped},
    {"byte", l_byte},
    {NULL, NULL},
};

void lua_bind_reset_runtime(lua_State *L)
{
    unsigned i;
    for (i = 0; i < BOARD_SOFT_TIMER_MAX; i++) {
        board_soft_timer_stop(i);
    }
    board_gpio_irq_reset();
    board_pin_reset_app_owners();
    board_resource_reset_app();
    s_stop = 0;
}

void lua_bind_register(lua_State *L)
{
    lua_sethook(L, stop_hook, LUA_MASKCOUNT, 1024);
    lua_pushglobaltable(L);
    luaL_setfuncs(L, k_core_globals, 0);
    lua_pop(L, 1);
    luaL_newlib(L, k_iq_functions);
    lua_setglobal(L, "iq");
}
