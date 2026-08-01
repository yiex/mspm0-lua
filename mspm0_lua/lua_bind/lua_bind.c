#include "lua_bind.h"
#include "lua_runtime.h"

#include <string.h>

#include "board_adc.h"
#include "board_delay.h"
#include "board_i2c.h"
#include "board_iq.h"
#include "board_irq.h"
#include "board_pid.h"
#include "board_filt.h"
#include "board_btn.h"
#include "board_enc.h"
#include "board_wdt.h"
#include "board_ramp.h"
#include "board_util.h"
#include "board_crc.h"
#include "board_cap.h"
#include "board_qei.h"
#include "board_lfs.h"
#include "board_pins.h"
#include "board_pwm.h"
#include "board_reg.h"
#include "board_resource.h"
#include "board_spi.h"
#include "board_uart.h"
#ifdef LUA_BINARY_ONLY
#include "board_i2c1.h"
#include "board_oled.h"
#include "board_spi0.h"
#include "board_uart_app.h"
#endif
#include "ti_msp_dl_config.h"

#include "lauxlib.h"

#define N_PWM 2
#define N_COMP 2
#define N_TMR BOARD_SOFT_TIMER_MAX
#define N_IRQ_CB BOARD_GPIO_IRQ_MAX
#define N_TASK 4

enum {
    TASK_FREE = 0,
    TASK_READY,
    TASK_RUNNING,
    TASK_SLEEPING,
};

typedef struct {
    int used;
    int freq;
    int duty;
    char pin[8];
} pwm_slot_t;

typedef struct {
    int used;
    char hi[8];
    char lo[8];
} comp_slot_t;

static pwm_slot_t s_pwm[N_PWM];
static comp_slot_t s_comp[N_COMP];
static board_i2c_t s_i2c;
static board_spi_t s_spi;
#ifdef LUA_BINARY_ONLY
static board_i2c1_t s_i2c1;
static board_spi0_t s_spi0;
static board_uart_app_t s_uart_app[3];
#endif
static volatile int s_stop;
static struct {
    int used;
    int cb_ref;
} s_tmr[N_TMR];
static struct {
    int used;
    int cb_ref;
    char pin[8];
} s_irq_cb[N_IRQ_CB];
static struct {
    int state;
    int ref;
    lua_State *co;
    uint32_t wake;
} s_task[N_TASK];
static int s_event_stop;

void lua_bind_request_stop(void) { s_stop = 1; }
void lua_bind_clear_stop(void) { s_stop = 0; }
int lua_bind_stop_requested(void) { return s_stop; }

static void cpy8(char *d, const char *s)
{
    size_t n = 0;
    while (s[n] && n < 7) {
        d[n] = s[n];
        n++;
    }
    d[n] = 0;
}

/* Only consume '!'; other bytes stay in UART ring for the console parser. */
static void poll_stop_uart(void)
{
    for (;;) {
        int c = board_uart_peek_nonblock();
        if (c < 0) {
            break;
        }
        if (c == '!') {
            (void)board_uart_getc_nonblock();
            s_stop = 1;
            /* keep scanning for more '!' but do not drop other bytes */
            continue;
        }
        break;
    }
}

/*
 * Check the console periodically even when Lua code never calls yield(),
 * delay_ms(), or stopped().  This turns '!' into a VM-level interrupt and
 * keeps an accidental tight loop from permanently owning the CPU.
 */
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
    uint32_t t0 = board_millis();
    while ((uint32_t)(board_millis() - t0) < ms) {
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
    poll_stop_uart();
    if (s_stop) {
        return luaL_error(L, "STOP");
    }
    return 0;
}

static int l_stopped(lua_State *L)
{
    poll_stop_uart();
    lua_pushboolean(L, s_stop != 0);
    return 1;
}

static int l_tmr_every(lua_State *L)
{
    int ms = (int)luaL_checkinteger(L, 1);
    int cb_ref = LUA_NOREF;
    int i;
    if (ms < 1) {
        ms = 1;
    }
    if (!lua_isnoneornil(L, 2)) {
        luaL_checktype(L, 2, LUA_TFUNCTION);
        lua_pushvalue(L, 2);
        cb_ref = luaL_ref(L, LUA_REGISTRYINDEX);
    }
    for (i = 0; i < N_TMR; i++) {
        if (!s_tmr[i].used) {
            s_tmr[i].used = 1;
            s_tmr[i].cb_ref = cb_ref;
            if (board_soft_timer_start((unsigned)i, (uint32_t)ms) != 0) {
                s_tmr[i].used = 0;
                luaL_unref(L, LUA_REGISTRYINDEX, cb_ref);
                return luaL_error(L, "tmr start");
            }
            lua_pushinteger(L, i);
            return 1;
        }
    }
    luaL_unref(L, LUA_REGISTRYINDEX, cb_ref);
    return luaL_error(L, "no tmr");
}

static int l_tmr_ready(lua_State *L)
{
    int id = (int)luaL_checkinteger(L, 1);
    if (id < 0 || id >= N_TMR || !s_tmr[id].used) {
        lua_pushboolean(L, 0);
        return 1;
    }
    lua_pushboolean(L, board_soft_timer_take((unsigned)id) != 0u);
    return 1;
}

static int l_tmr_stop(lua_State *L)
{
    int id = (int)luaL_checkinteger(L, 1);
    if (id >= 0 && id < N_TMR) {
        board_soft_timer_stop((unsigned)id);
        luaL_unref(L, LUA_REGISTRYINDEX, s_tmr[id].cb_ref);
        s_tmr[id].used = 0;
        s_tmr[id].cb_ref = LUA_NOREF;
    }
    return 0;
}

static int l_gpio_mode(lua_State *L)
{
    const char *name = luaL_checkstring(L, 1);
    const char *mode = luaL_optstring(L, 2, "out");
    board_pin_t p;
    int st;
    if (board_pin_resolve(name, &p) != 0) {
        return luaL_error(L, "pin");
    }
    st = board_pin_claim(name, PIN_OWN_GPIO, 0);
    if (st != 0) {
        return luaL_error(L, "%s:%s", name, board_pin_errstr(st));
    }
    if (p.port == GPIOA && p.pin == BOARD_PIN_MASK(14)) {
        int i;
        for (i = 0; i < N_PWM; i++) {
            if (s_pwm[i].used && s_pwm[i].pin[0] == 'P' &&
                    s_pwm[i].pin[1] == 'A' && s_pwm[i].pin[2] == '1' &&
                    s_pwm[i].pin[3] == '4' && !s_pwm[i].pin[4]) {
                board_pwm_close(i);
                s_pwm[i].used = 0;
            }
        }
    }
    if (mode[0] == 'o') {
        board_reg_pin_out(p.port, p.pin, p.iomux);
    } else {
        p.port->DOECLR31_0 = p.pin;
        board_reg_iomux_gpio_in_pu(p.iomux);
    }
    return 0;
}

/* Configure as GPIO out only if DOE not already set (mode() or first use). */
static int gpio_ensure_out(const char *name, const board_pin_t *p)
{
    int st = board_pin_claim(name, PIN_OWN_GPIO, 0);
    if (st != 0) {
        return st;
    }
    if ((p->port->DOE31_0 & p->pin) == 0u) {
        if (p->port == GPIOA && p->pin == BOARD_PIN_MASK(14)) {
            int i;
            for (i = 0; i < N_PWM; i++) {
                if (s_pwm[i].used && !strcmp(s_pwm[i].pin, "PA14")) {
                    board_pwm_close(i);
                    s_pwm[i].used = 0;
                }
            }
        }
        board_reg_pin_out(p->port, p->pin, p->iomux);
    }
    return 0;
}

static int l_gpio_set(lua_State *L)
{
    const char *name = luaL_checkstring(L, 1);
    int val = (int)luaL_checkinteger(L, 2);
    board_pin_t p;
    int st;
    if (board_pin_resolve(name, &p) != 0) {
        return luaL_error(L, "pin");
    }
    st = gpio_ensure_out(name, &p);
    if (st != 0) {
        return luaL_error(L, "%s:%s", name, board_pin_errstr(st));
    }
    board_reg_gpio_write(p.port, p.pin, val);
    return 0;
}

static int l_gpio_get(lua_State *L)
{
    const char *name = luaL_checkstring(L, 1);
    board_pin_t p;
    if (board_pin_resolve(name, &p) != 0) {
        return luaL_error(L, "pin");
    }
    lua_pushinteger(L, board_reg_gpio_read(p.port, p.pin) ? 1 : 0);
    return 1;
}

static int l_gpio_toggle(lua_State *L)
{
    const char *name = luaL_checkstring(L, 1);
    board_pin_t p;
    int st;
    if (board_pin_resolve(name, &p) != 0) {
        return luaL_error(L, "pin");
    }
    st = gpio_ensure_out(name, &p);
    if (st != 0) {
        return luaL_error(L, "%s:%s", name, board_pin_errstr(st));
    }
    board_reg_gpio_tog(p.port, p.pin);
    return 0;
}

/* Generic MSPM0 IOMUX route. PF values are the numbers in the data sheet. */
static int l_gpio_af(lua_State *L)
{
    const char *pin = luaL_checkstring(L, 1);
    int pf = (int)luaL_checkinteger(L, 2);
    int st;
    if (pf < 0 || pf > 9) {
        return luaL_error(L, "pf");
    }
    st = board_pin_claim(pin, PIN_OWN_GPIO, 0);
    if (st != 0) {
        return luaL_error(L, "%s:%s", pin, board_pin_errstr(st));
    }
    st = board_pin_af(pin, (unsigned)pf, luaL_optinteger(L, 3, 1) != 0);
    if (st != 0) {
        return luaL_error(L, "%s:%s", pin, board_pin_errstr(st));
    }
    return 0;
}

static int l_gpio_owner(lua_State *L)
{
    const char *pin = luaL_checkstring(L, 1);
    int o = board_pin_owner(pin);
    if (o < 0) {
        lua_pushnil(L);
        return 1;
    }
    lua_pushstring(L, board_pin_owner_str((uint8_t)o));
    return 1;
}

static int l_gpio_release(lua_State *L)
{
    const char *pin = luaL_checkstring(L, 1);
    board_pin_release_owned(pin, PIN_OWN_GPIO);
    return 0;
}

static int l_irq_on(lua_State *L)
{
    const char *pin = luaL_checkstring(L, 1);
    const char *edge_s = luaL_optstring(L, 2, "fall");
    int cb_slot = -1;
    int cb_ref = LUA_NOREF;
    uint32_t debounce = 0;
    int edge = (edge_s[0] == 'r') ? 1 : (edge_s[0] == 'b') ? 2 : 0;
    if (lua_isfunction(L, 3)) {
        debounce = (uint32_t)luaL_optinteger(L, 4, 0);
        for (int i = 0; i < N_IRQ_CB; i++) {
            if (s_irq_cb[i].used && strcmp(s_irq_cb[i].pin, pin) == 0) {
                cb_slot = i;
                break;
            }
            if (!s_irq_cb[i].used && cb_slot < 0) cb_slot = i;
        }
        if (cb_slot < 0) return luaL_error(L, "no irq callback");
        lua_pushvalue(L, 3);
        cb_ref = luaL_ref(L, LUA_REGISTRYINDEX);
    } else {
        debounce = (uint32_t)luaL_optinteger(L, 3, 0);
    }
    int st = board_pin_claim(pin, PIN_OWN_IRQ, 0);
    if (st != 0) {
        luaL_unref(L, LUA_REGISTRYINDEX, cb_ref);
        return luaL_error(L, "%s:%s", pin, board_pin_errstr(st));
    }
    /* Drop previous callback for this pin before enable (atomic-ish vs dispatch). */
    for (int i = 0; i < N_IRQ_CB; i++) {
        if (s_irq_cb[i].used && strcmp(s_irq_cb[i].pin, pin) == 0) {
            luaL_unref(L, LUA_REGISTRYINDEX, s_irq_cb[i].cb_ref);
            s_irq_cb[i].used = 0;
            s_irq_cb[i].cb_ref = LUA_NOREF;
        }
    }
    if (board_gpio_irq_enable(pin, edge) != 0) {
        luaL_unref(L, LUA_REGISTRYINDEX, cb_ref);
        board_pin_release_owned(pin, PIN_OWN_IRQ);
        return luaL_error(L, "irq");
    }
    (void)board_gpio_irq_set_debounce(pin, debounce);
    if (cb_slot >= 0) {
        s_irq_cb[cb_slot].used = 1;
        s_irq_cb[cb_slot].cb_ref = cb_ref;
        cpy8(s_irq_cb[cb_slot].pin, pin);
    }
    return 0;
}

static int l_irq_off(lua_State *L)
{
    const char *pin = luaL_checkstring(L, 1);
    for (int i = 0; i < N_IRQ_CB; i++) {
        if (s_irq_cb[i].used && strcmp(s_irq_cb[i].pin, pin) == 0) {
            luaL_unref(L, LUA_REGISTRYINDEX, s_irq_cb[i].cb_ref);
            s_irq_cb[i].used = 0;
            s_irq_cb[i].cb_ref = LUA_NOREF;
        }
    }
    board_gpio_irq_disable(pin);
    if (board_pin_owner(pin) == PIN_OWN_IRQ) {
        board_pin_release_owned(pin, PIN_OWN_IRQ);
    }
    return 0;
}

static int l_irq_count(lua_State *L)
{
    lua_pushinteger(L, (lua_Integer)board_gpio_irq_count(luaL_checkstring(L, 1)));
    return 1;
}

static int task_find(lua_State *co)
{
    for (int i = 0; i < N_TASK; i++) {
        if (s_task[i].state != TASK_FREE && s_task[i].co == co) return i;
    }
    return -1;
}

static void task_drop(lua_State *L, int id)
{
    luaL_unref(L, LUA_REGISTRYINDEX, s_task[id].ref);
    s_task[id].state = TASK_FREE;
    s_task[id].ref = LUA_NOREF;
    s_task[id].co = NULL;
    s_task[id].wake = 0;
}

static int l_task_spawn(lua_State *L)
{
    int id = -1;
    lua_State *co;
    luaL_checktype(L, 1, LUA_TFUNCTION);
    for (int i = 0; i < N_TASK; i++) {
        if (s_task[i].state == TASK_FREE) {
            id = i;
            break;
        }
    }
    if (id < 0) return luaL_error(L, "no task");
    co = lua_newthread(L);
    s_task[id].ref = luaL_ref(L, LUA_REGISTRYINDEX);
    lua_sethook(co, stop_hook, LUA_MASKCOUNT, 1024);
    lua_pushvalue(L, 1);
    lua_xmove(L, co, 1);
    s_task[id].co = co;
    s_task[id].wake = 0;
    s_task[id].state = TASK_READY;
    lua_pushinteger(L, id);
    return 1;
}

static int l_task_sleep(lua_State *L)
{
    int id = task_find(L);
    uint32_t ms = (uint32_t)luaL_checkinteger(L, 1);
    if (id < 0) return luaL_error(L, "task.sleep outside task");
    s_task[id].wake = board_millis() + ms;
    s_task[id].state = TASK_SLEEPING;
    return lua_yield(L, 0);
}

static int l_task_yield(lua_State *L)
{
    int id = task_find(L);
    if (id < 0) return luaL_error(L, "task.yield outside task");
    s_task[id].state = TASK_READY;
    return lua_yield(L, 0);
}

static int l_task_cancel(lua_State *L)
{
    int id = (int)luaL_checkinteger(L, 1);
    if (id < 0 || id >= N_TASK || s_task[id].state == TASK_FREE) return 0;
    if (s_task[id].co == L) {
        return luaL_error(L, "task cannot cancel itself; return instead");
    }
    task_drop(L, id);
    return 0;
}

/* Return dispatched count, or -1 with a Lua error object on the stack. */
static int dispatch_callbacks(lua_State *L)
{
    int dispatched = 0;
    for (int i = 0; i < N_TMR; i++) {
        uint32_t n;
        if (!s_tmr[i].used || s_tmr[i].cb_ref == LUA_NOREF) continue;
        n = board_soft_timer_take((unsigned)i);
        if (!n) continue;
        lua_rawgeti(L, LUA_REGISTRYINDEX, s_tmr[i].cb_ref);
        lua_pushinteger(L, i);
        lua_pushinteger(L, (lua_Integer)n);
        if (lua_pcall(L, 2, 0, 0) != LUA_OK) return -1;
        dispatched++;
    }
    for (int i = 0; i < N_IRQ_CB; i++) {
        uint32_t n;
        if (!s_irq_cb[i].used) continue;
        n = board_gpio_irq_count(s_irq_cb[i].pin);
        if (!n) continue;
        lua_rawgeti(L, LUA_REGISTRYINDEX, s_irq_cb[i].cb_ref);
        lua_pushstring(L, s_irq_cb[i].pin);
        lua_pushinteger(L, (lua_Integer)n);
        if (lua_pcall(L, 2, 0, 0) != LUA_OK) return -1;
        dispatched++;
    }
    return dispatched;
}

static int dispatch_tasks(lua_State *L)
{
    uint32_t now = board_millis();
    int dispatched = 0;
    for (int i = 0; i < N_TASK; i++) {
        if (s_task[i].state == TASK_SLEEPING &&
                (int32_t)(now - s_task[i].wake) >= 0) {
            s_task[i].state = TASK_READY;
        }
    }
    for (int i = 0; i < N_TASK; i++) {
        int st, nres = 0;
        lua_State *co;
        if (s_task[i].state != TASK_READY) continue;
        co = s_task[i].co;
        s_task[i].state = TASK_RUNNING;
        st = lua_resume(co, L, 0, &nres);
        dispatched++;
        if (st == LUA_YIELD) {
            if (nres) lua_pop(co, nres);
            if (s_task[i].state == TASK_RUNNING) s_task[i].state = TASK_READY;
        } else if (st == LUA_OK) {
            if (nres) lua_pop(co, nres);
            task_drop(L, i);
        } else {
            lua_xmove(co, L, 1);
            task_drop(L, i);
            return -1;
        }
    }
    return dispatched;
}

static int event_has_work(void)
{
    for (int i = 0; i < N_TMR; i++)
        if (s_tmr[i].used && s_tmr[i].cb_ref != LUA_NOREF) return 1;
    for (int i = 0; i < N_IRQ_CB; i++)
        if (s_irq_cb[i].used) return 1;
    for (int i = 0; i < N_TASK; i++)
        if (s_task[i].state != TASK_FREE) return 1;
    return 0;
}

static int event_step(lua_State *L)
{
    int a = dispatch_callbacks(L);
    int b;
    if (a < 0) return -1;
    b = dispatch_tasks(L);
    if (b < 0) return -1;
    return a + b;
}

static int l_event_poll(lua_State *L)
{
    int n = event_step(L);
    if (n < 0) return lua_error(L);
    lua_pushinteger(L, n);
    return 1;
}

static int l_event_stop(lua_State *L)
{
    (void)L;
    s_event_stop = 1;
    return 0;
}

static int l_event_run(lua_State *L)
{
    if (task_find(L) >= 0) return luaL_error(L, "event.run inside task");
    s_event_stop = 0;
    while (!s_event_stop) {
        int n;
        board_wdt_feed();
        poll_stop_uart();
        if (s_stop) return luaL_error(L, "STOP");
        n = event_step(L);
        if (n < 0) return lua_error(L);
        if (!event_has_work()) break;
        if (!n) __WFI();
    }
    return 0;
}

static int l_uart_write(lua_State *L)
{
    size_t n = 0;
    const char *s = luaL_checklstring(L, 1, &n);
    board_uart_write(s, n);
    return 0;
}

static int l_uart_read(lua_State *L)
{
    char data[64];
    size_t n = 0;
    size_t max = (size_t)luaL_optinteger(L, 2, sizeof(data));
    uint32_t timeout = (uint32_t)luaL_optinteger(L, 1, 0);
    uint32_t start = board_millis();
    if (max > sizeof(data)) {
        max = sizeof(data);
    }
    while (n < max) {
        int c = board_uart_getc_nonblock();
        if (c >= 0) {
            if (c == '!') {
                s_stop = 1;
                return luaL_error(L, "STOP");
            }
            data[n++] = (char)c;
            continue;
        }
        if (n || (uint32_t)(board_millis() - start) >= timeout) {
            break;
        }
    }
    if (n) {
        lua_pushlstring(L, data, n);
    } else {
        lua_pushnil(L);
    }
    return 1;
}

#ifdef LUA_BINARY_ONLY
static int app_uart_id(lua_State *L)
{
    int id = (int)luaL_checkinteger(L, 1);
    if (id < 1 || id > 3) luaL_error(L, "uart id 1..3");
    return id;
}

static board_resource_t uart_resource(int id)
{
    return (board_resource_t)(BOARD_RES_UART1 + id - 1);
}

static int l_uart_app_open(lua_State *L)
{
    int id = app_uart_id(L);
    const char *dtx = id == 1 ? "PA17" : id == 2 ? "PA23" : "PA26";
    const char *drx = id == 1 ? "PA18" : id == 2 ? "PA24" : "PA25";
    {
        const char *tx = luaL_optstring(L, 2, dtx);
        const char *rx = luaL_optstring(L, 3, drx);
        uint8_t own = (uint8_t)(PIN_OWN_UART1 + (id - 1));
        board_resource_t resource = uart_resource(id);
        if (s_uart_app[id - 1].open) {
            board_uart_app_close(&s_uart_app[id - 1]);
            board_pin_release_owner(own);
            board_resource_release(resource, own);
        }
        if (board_resource_claim(resource, own) != 0) {
            return luaL_error(L, "uart%d:busy", id);
        }
        int st = board_pin_claim(tx, own, 0);
        if (st != 0) {
            board_resource_release(resource, own);
            return luaL_error(L, "%s:%s", tx, board_pin_errstr(st));
        }
        st = board_pin_claim(rx, own, 0);
        if (st != 0) {
            board_pin_release_owned(tx, own);
            board_resource_release(resource, own);
            return luaL_error(L, "%s:%s", rx, board_pin_errstr(st));
        }
        if (board_uart_app_open(&s_uart_app[id - 1], (unsigned)id, tx, rx,
                (uint32_t)luaL_optinteger(L, 4, 115200)) != 0) {
            board_pin_release_owner(own);
            board_resource_release(resource, own);
            return luaL_error(L, "uart open");
        }
    }
    return 0;
}

static int l_uart_app_close(lua_State *L)
{
    int id = app_uart_id(L);
    board_uart_app_close(&s_uart_app[id - 1]);
    board_pin_release_owner((uint8_t)(PIN_OWN_UART1 + (id - 1)));
    board_resource_release(uart_resource(id),
        (uint8_t)(PIN_OWN_UART1 + (id - 1)));
    return 0;
}

static int l_uart_app_tx(lua_State *L)
{
    int id = app_uart_id(L);
    size_t n;
    const char *data = luaL_checklstring(L, 2, &n);
    if (board_uart_app_write(&s_uart_app[id - 1], (const uint8_t *)data, n)) {
        return luaL_error(L, "uart tx");
    }
    return 0;
}

static int l_uart_app_rx(lua_State *L)
{
    uint8_t data[64];
    int id = app_uart_id(L);
    uint32_t timeout = (uint32_t)luaL_optinteger(L, 2, 0);
    size_t max = (size_t)luaL_optinteger(L, 3, sizeof(data));
    size_t n;
    if (max > sizeof(data)) max = sizeof(data);
    n = board_uart_app_read(&s_uart_app[id - 1], data, max, timeout);
    if (n) lua_pushlstring(L, (const char *)data, n);
    else lua_pushnil(L);
    return 1;
}

#endif

/* Tiny binary helpers; full Lua string library is intentionally omitted. */
static int l_bytes(lua_State *L)
{
    uint8_t data[64];
    int n = lua_gettop(L);
    int i;
    if (n > (int)sizeof(data)) {
        return luaL_error(L, "bytes max 64");
    }
    for (i = 0; i < n; i++) {
        data[i] = (uint8_t)luaL_checkinteger(L, i + 1);
    }
    lua_pushlstring(L, (const char *)data, (size_t)n);
    return 1;
}

static int l_byte(lua_State *L)
{
    size_t n;
    size_t index = (size_t)luaL_optinteger(L, 2, 1);
    const uint8_t *data = (const uint8_t *)luaL_checklstring(L, 1, &n);
    if (index == 0 || index > n) {
        lua_pushnil(L);
    } else {
        lua_pushinteger(L, data[index - 1]);
    }
    return 1;
}

/* Arg1: channel 0..7 or pin name "PA27" etc. Errors via luaL_error. */
static int adc_ch_arg(lua_State *L, int idx)
{
    if (lua_type(L, idx) == LUA_TSTRING) {
        const char *pin = lua_tostring(L, idx);
        int ch = board_adc_claim_pin(pin);
        if (ch < 0) {
            luaL_error(L, "%s:%s", pin, ch == -3 ? "busy" : "pin");
        }
        return ch;
    }
    return (int)luaL_checkinteger(L, idx);
}

static int l_adc_read(lua_State *L)
{
    int ch = adc_ch_arg(L, 1);
    int value = board_adc_read((uint8_t)ch);
    if (value < 0) {
        lua_pushnil(L);
    } else {
        lua_pushinteger(L, value);
    }
    return 1;
}

/* High-speed DMA burst: returns packed little-endian u16 string + period_ns. */
static int l_adc_capture(lua_State *L)
{
    uint16_t buf[BOARD_ADC_DMA_MAX];
    int ch = adc_ch_arg(L, 1);
    int n = (int)luaL_optinteger(L, 2, 64);
    uint32_t timeout = (uint32_t)luaL_optinteger(L, 3, 200);
    int rate = (int)luaL_optinteger(L, 4, 1);
    int got;
    if (n < 2) {
        n = 2;
    }
    if (n > BOARD_ADC_DMA_MAX) {
        n = BOARD_ADC_DMA_MAX;
    }
    if (rate < 0) {
        rate = 0;
    } else if (rate > 2) {
        rate = 2;
    }
    got = board_adc_capture(
        (uint8_t)ch, buf, (size_t)n, timeout, (uint8_t)rate);
    if (got < 0) {
        lua_pushnil(L);
        return 1;
    }
    lua_pushlstring(L, (const char *)buf, (size_t)got * sizeof(uint16_t));
    lua_pushinteger(L, (lua_Integer)board_adc_capture_period_ns());
    return 2;
}

static int l_adc_channel(lua_State *L)
{
    int ch = board_adc_pin_channel(luaL_checkstring(L, 1));
    if (ch < 0) {
        lua_pushnil(L);
    } else {
        lua_pushinteger(L, ch);
    }
    return 1;
}

/* LittleFS helpers: large tables/assets live off-chip (W25Q). */
static int valid_fs_name(const char *name)
{
    size_t n, i;
    if (!name || !name[0]) {
        return 0;
    }
    n = strlen(name);
    if (n > 28) {
        return 0;
    }
    for (i = 0; i < n; i++) {
        char c = name[i];
        if (!((c >= 'A' && c <= 'Z') || (c >= 'a' && c <= 'z') ||
                (c >= '0' && c <= '9') || c == '.' || c == '_' || c == '-')) {
            return 0;
        }
    }
    return 1;
}

static int l_fs_ready(lua_State *L)
{
    lua_pushboolean(L, board_lfs_ready());
    return 1;
}

static int l_fs_exists(lua_State *L)
{
    const char *name = luaL_checkstring(L, 1);
    int ok = 0;
    if (valid_fs_name(name) && board_lfs_ready() &&
        board_lfs_read_open(name) == 0) {
        (void)board_lfs_read_close();
        ok = 1;
    }
    lua_pushboolean(L, ok);
    return 1;
}

/* fs.read(name, max?) — up to 512 bytes into Lua string (tables/config). */
static int l_fs_read(lua_State *L)
{
    char buf[512];
    const char *name = luaL_checkstring(L, 1);
    size_t max = (size_t)luaL_optinteger(L, 2, sizeof(buf));
    int n;
    if (!valid_fs_name(name) || !board_lfs_ready()) {
        lua_pushnil(L);
        return 1;
    }
    if (max > sizeof(buf)) {
        max = sizeof(buf);
    }
    if (board_lfs_read_open(name) != 0) {
        lua_pushnil(L);
        return 1;
    }
    n = board_lfs_read_chunk(buf, max);
    (void)board_lfs_read_close();
    if (n < 0) {
        lua_pushnil(L);
    } else {
        lua_pushlstring(L, buf, (size_t)n);
    }
    return 1;
}

/* fs.write(name, data) — small asset ≤512 B (upload big files via HEX). */
static int l_fs_write(lua_State *L)
{
    size_t n;
    const char *name = luaL_checkstring(L, 1);
    const char *data = luaL_checklstring(L, 2, &n);
    if (!valid_fs_name(name) || !board_lfs_ready() || n > 512) {
        lua_pushboolean(L, 0);
        return 1;
    }
    /* board_lfs_write_file returns byte count (>=0) or <0 on error. */
    lua_pushboolean(L, board_lfs_write_file(name, data, n) >= 0);
    return 1;
}

static int l_fs_remove(lua_State *L)
{
    const char *name = luaL_checkstring(L, 1);
    if (!valid_fs_name(name) || !board_lfs_ready() ||
        strcmp(name, "main.luac") == 0) {
        lua_pushboolean(L, 0);
        return 1;
    }
    lua_pushboolean(L, board_lfs_remove(name) == 0);
    return 1;
}

static int l_fs_size(lua_State *L)
{
    lua_pushinteger(L, (lua_Integer)board_lfs_capacity_bytes());
    return 1;
}

static int i2c_id(lua_State *L)
{
    int id = (int)luaL_checkinteger(L, 1);
#ifdef LUA_BINARY_ONLY
    if (id != 0 && id != 1) {
#else
    if (id != 0) {
#endif
        luaL_error(L, "i2c id");
    }
    return id;
}

static int claim_pair(lua_State *L, const char *a, const char *b, uint8_t own)
{
    int st = board_pin_claim(a, own, 0);
    if (st != 0) {
        return luaL_error(L, "%s:%s", a, board_pin_errstr(st));
    }
    st = board_pin_claim(b, own, 0);
    if (st != 0) {
        board_pin_release_owned(a, own);
        return luaL_error(L, "%s:%s", b, board_pin_errstr(st));
    }
    return 0;
}

static int l_i2c_open(lua_State *L)
{
    int id = i2c_id(L);
    const char *scl;
    const char *sda;
#ifdef LUA_BINARY_ONLY
    if (id == 1) {
        if (s_i2c1.open) {
            board_i2c1_close(&s_i2c1);
            board_pin_release_owner(PIN_OWN_I2C1);
            board_resource_release(BOARD_RES_I2C1, PIN_OWN_I2C1);
        }
        scl = luaL_optstring(L, 2, "PA15");
        sda = luaL_optstring(L, 3, "PA16");
        if (claim_pair(L, scl, sda, PIN_OWN_I2C1) != 0) {
            return 0;
        }
        if (board_resource_claim(BOARD_RES_I2C1, PIN_OWN_I2C1) != 0) {
            board_pin_release_owner(PIN_OWN_I2C1);
            return luaL_error(L, "i2c1:busy");
        }
        if (board_i2c1_open(&s_i2c1, scl, sda,
                (uint32_t)luaL_optinteger(L, 4, 100000)) != 0) {
            board_pin_release_owner(PIN_OWN_I2C1);
            board_resource_release(BOARD_RES_I2C1, PIN_OWN_I2C1);
            return luaL_error(L, "i2c1 open");
        }
        return 0;
    }
#endif
    if (s_i2c.open) {
        board_i2c_close(&s_i2c);
        board_pin_release_owner(PIN_OWN_I2C0);
        board_resource_release(BOARD_RES_I2C0, PIN_OWN_I2C0);
    }
    scl = luaL_optstring(L, 2, "PA1");
    sda = luaL_optstring(L, 3, "PA0");
    if (claim_pair(L, scl, sda, PIN_OWN_I2C0) != 0) {
        return 0;
    }
    if (board_resource_claim(BOARD_RES_I2C0, PIN_OWN_I2C0) != 0) {
        board_pin_release_owner(PIN_OWN_I2C0);
        return luaL_error(L, "i2c0:busy");
    }
    if (board_i2c_open(&s_i2c, scl, sda,
            (int)luaL_optinteger(L, 4, 100000)) != 0) {
        board_pin_release_owner(PIN_OWN_I2C0);
        board_resource_release(BOARD_RES_I2C0, PIN_OWN_I2C0);
        return luaL_error(L, "i2c open");
    }
    return 0;
}

static int l_i2c_close(lua_State *L)
{
    int id = i2c_id(L);
#ifdef LUA_BINARY_ONLY
    if (id == 1) {
        board_i2c1_close(&s_i2c1);
        board_pin_release_owner(PIN_OWN_I2C1);
        board_resource_release(BOARD_RES_I2C1, PIN_OWN_I2C1);
        return 0;
    }
#endif
    board_i2c_close(&s_i2c);
    board_pin_release_owner(PIN_OWN_I2C0);
    board_resource_release(BOARD_RES_I2C0, PIN_OWN_I2C0);
    return 0;
}

static int l_i2c_write(lua_State *L)
{
    size_t n;
    const char *data;
    int id = i2c_id(L);
    data = luaL_checklstring(L, 3, &n);
#ifdef LUA_BINARY_ONLY
    if (id == 1) {
        lua_pushboolean(L, board_i2c1_write(&s_i2c1,
            (uint8_t)luaL_checkinteger(L, 2), (const uint8_t *)data, n) == 0);
        return 1;
    }
#endif
    lua_pushboolean(L, board_i2c_write(&s_i2c,
        (uint8_t)luaL_checkinteger(L, 2), (const uint8_t *)data, n) == 0);
    return 1;
}

/* i2c.writev(id, addr7, b0, b1, ...) — stack buffer, no Lua string alloc. */
static int l_i2c_writev(lua_State *L)
{
    uint8_t buf[32];
    int id = i2c_id(L);
    uint8_t addr = (uint8_t)luaL_checkinteger(L, 2);
    int top = lua_gettop(L);
    int n = top - 2;
    int i;
    if (n < 0 || n > (int)sizeof(buf)) {
        return luaL_error(L, "i2c writev len");
    }
    for (i = 0; i < n; i++) {
        buf[i] = (uint8_t)luaL_checkinteger(L, 3 + i);
    }
#ifdef LUA_BINARY_ONLY
    if (id == 1) {
        lua_pushboolean(L, board_i2c1_write(&s_i2c1, addr, buf, (size_t)n) == 0);
        return 1;
    }
#endif
    lua_pushboolean(L, board_i2c_write(&s_i2c, addr, buf, (size_t)n) == 0);
    return 1;
}

static int l_i2c_read(lua_State *L)
{
    uint8_t data[64];
    size_t n;
    int id = i2c_id(L);
    n = (size_t)luaL_checkinteger(L, 3);
#ifdef LUA_BINARY_ONLY
    if (id == 1) {
        if (n > sizeof(data) || board_i2c1_read(&s_i2c1,
                (uint8_t)luaL_checkinteger(L, 2), data, n) != 0) {
            lua_pushnil(L);
        } else {
            lua_pushlstring(L, (const char *)data, n);
        }
        return 1;
    }
#endif
    if (n > sizeof(data) || board_i2c_read(&s_i2c,
            (uint8_t)luaL_checkinteger(L, 2), data, n) != 0) {
        lua_pushnil(L);
    } else {
        lua_pushlstring(L, (const char *)data, n);
    }
    return 1;
}

static int l_i2c_write_read(lua_State *L)
{
    uint8_t data[64];
    size_t wn;
    size_t rn;
    const char *w;
    int id = i2c_id(L);
    w = luaL_checklstring(L, 3, &wn);
    rn = (size_t)luaL_checkinteger(L, 4);
#ifdef LUA_BINARY_ONLY
    if (id == 1) {
        if (rn > sizeof(data) || board_i2c1_write_read(&s_i2c1,
                (uint8_t)luaL_checkinteger(L, 2), (const uint8_t *)w, wn,
                data, rn) != 0) {
            lua_pushnil(L);
        } else {
            lua_pushlstring(L, (const char *)data, rn);
        }
        return 1;
    }
#endif
    if (rn > sizeof(data) || board_i2c_write_read(&s_i2c,
            (uint8_t)luaL_checkinteger(L, 2), (const uint8_t *)w, wn,
            data, rn) != 0) {
        lua_pushnil(L);
    } else {
        lua_pushlstring(L, (const char *)data, rn);
    }
    return 1;
}

static int spi_id(lua_State *L)
{
    int id = (int)luaL_checkinteger(L, 1);
#ifdef LUA_BINARY_ONLY
    if (id != 0 && id != 1) {
#else
    if (id != 0) {
#endif
        luaL_error(L, "spi id");
    }
    return id;
}

static int claim4(lua_State *L, const char *a, const char *b, const char *c,
    const char *d, uint8_t own)
{
    int st;
    if ((st = board_pin_claim(a, own, 0)) != 0) {
        return luaL_error(L, "%s:%s", a, board_pin_errstr(st));
    }
    if ((st = board_pin_claim(b, own, 0)) != 0) {
        board_pin_release_owned(a, own);
        return luaL_error(L, "%s:%s", b, board_pin_errstr(st));
    }
    if ((st = board_pin_claim(c, own, 0)) != 0) {
        board_pin_release_owned(a, own);
        board_pin_release_owned(b, own);
        return luaL_error(L, "%s:%s", c, board_pin_errstr(st));
    }
    if ((st = board_pin_claim(d, own, 0)) != 0) {
        board_pin_release_owned(a, own);
        board_pin_release_owned(b, own);
        board_pin_release_owned(c, own);
        return luaL_error(L, "%s:%s", d, board_pin_errstr(st));
    }
    return 0;
}

static int l_spi_open(lua_State *L)
{
    int id = spi_id(L);
    const char *sck, *pico, *poci, *cs;
#ifdef LUA_BINARY_ONLY
    if (id == 1) {
        if (s_spi0.open) {
            board_spi0_close(&s_spi0);
            board_pin_release_owner(PIN_OWN_SPI0);
            board_resource_release(BOARD_RES_SPI0, PIN_OWN_SPI0);
        }
        sck = luaL_optstring(L, 2, "PA12");
        pico = luaL_optstring(L, 3, "PA14");
        poci = luaL_optstring(L, 4, "PA13");
        cs = luaL_optstring(L, 5, "PA18");
        if (claim4(L, sck, pico, poci, cs, PIN_OWN_SPI0) != 0) {
            return 0;
        }
        if (board_resource_claim(BOARD_RES_SPI0, PIN_OWN_SPI0) != 0) {
            board_pin_release_owner(PIN_OWN_SPI0);
            return luaL_error(L, "spi0:busy");
        }
        if (board_spi0_open(&s_spi0, sck, pico, poci, cs,
                (uint32_t)luaL_optinteger(L, 6, 1000000)) != 0) {
            board_pin_release_owner(PIN_OWN_SPI0);
            board_resource_release(BOARD_RES_SPI0, PIN_OWN_SPI0);
            return luaL_error(L, "spi0 open");
        }
        return 0;
    }
#endif
    if (s_spi.open) {
        board_spi_close(&s_spi);
        board_pin_release_owner(PIN_OWN_SPI1);
    }
    /* SPI1 bus pins are Flash-locked; only CS may be app pin. */
    sck = luaL_optstring(L, 2, "PB16");
    pico = luaL_optstring(L, 3, "PB15");
    poci = luaL_optstring(L, 4, "PB14");
    cs = luaL_optstring(L, 5, "PA18");
    {
        int st = board_pin_claim(cs, PIN_OWN_SPI1, 0);
        if (st != 0) {
            return luaL_error(L, "%s:%s", cs, board_pin_errstr(st));
        }
        (void)sck;
        (void)pico;
        (void)poci;
    }
    if (board_spi_open(&s_spi, sck, pico, poci, cs,
            (int)luaL_optinteger(L, 6, 1000000)) != 0) {
        board_pin_release_owned(cs, PIN_OWN_SPI1);
        return luaL_error(L, "spi open");
    }
    return 0;
}

static int l_spi_close(lua_State *L)
{
    int id = spi_id(L);
#ifdef LUA_BINARY_ONLY
    if (id == 1) {
        board_spi0_close(&s_spi0);
        board_pin_release_owner(PIN_OWN_SPI0);
        board_resource_release(BOARD_RES_SPI0, PIN_OWN_SPI0);
        return 0;
    }
#endif
    board_spi_close(&s_spi);
    board_pin_release_owner(PIN_OWN_SPI1);
    return 0;
}

static int l_spi_cs(lua_State *L)
{
    int id = spi_id(L);
#ifdef LUA_BINARY_ONLY
    if (id == 1) {
        board_spi0_cs(&s_spi0, lua_toboolean(L, 2) != 0);
        return 0;
    }
#endif
    board_spi_cs(&s_spi, lua_toboolean(L, 2) != 0);
    return 0;
}

static int l_spi_xfer(lua_State *L)
{
    uint8_t rx[64];
    size_t n;
    const char *tx;
    int hold;
    int id = spi_id(L);
    tx = luaL_checklstring(L, 2, &n);
    hold = lua_toboolean(L, 3);
    if (n > sizeof(rx)) {
        return luaL_error(L, "spi max 64");
    }
#ifdef LUA_BINARY_ONLY
    if (id == 1) {
        board_spi0_cs(&s_spi0, true);
        if (board_spi0_xfer(&s_spi0, (const uint8_t *)tx, rx, n) < 0) {
            board_spi0_cs(&s_spi0, false);
            return luaL_error(L, "spi0 xfer");
        }
        if (!hold) board_spi0_cs(&s_spi0, false);
        lua_pushlstring(L, (const char *)rx, n);
        return 1;
    }
#endif
    board_spi_cs(&s_spi, true);
    if (board_spi_xfer(&s_spi, (const uint8_t *)tx, rx, n) < 0) {
        board_spi_cs(&s_spi, false);
        return luaL_error(L, "spi xfer");
    }
    if (!hold) {
        board_spi_cs(&s_spi, false);
    }
    lua_pushlstring(L, (const char *)rx, n);
    return 1;
}

static int l_pwm_open(lua_State *L)
{
    const char *pin = luaL_optstring(L, 1, "PA14");
    int freq = (int)luaL_optinteger(L, 2, 1000);
    int st;
    int id;
    if (board_pwm_route(pin, NULL, NULL, NULL) != 0) {
        return luaL_error(L, "%s:pin", pin);
    }
    st = board_pwm_open(pin, (uint32_t)freq);
    if (st < 0) {
        return luaL_error(L, "%s:%s", pin, board_pin_errstr(st));
    }
    id = st;
    if (id < 0 || id >= N_PWM) {
        return luaL_error(L, "pwm");
    }
    s_pwm[id].used = 1;
    s_pwm[id].freq = freq;
    s_pwm[id].duty = 0;
    cpy8(s_pwm[id].pin, pin);
    lua_pushinteger(L, id);
    return 1;
}

static int l_pwm_duty(lua_State *L)
{
    int id = (int)luaL_checkinteger(L, 1);
    int duty = (int)luaL_checkinteger(L, 2);
    if (id < 0 || id >= N_PWM || !s_pwm[id].used) {
        return luaL_error(L, "id");
    }
    if (duty < 0) {
        duty = 0;
    }
    if (duty > 100) {
        duty = 100;
    }
    s_pwm[id].duty = duty;
    board_pwm_set_duty(id, (uint8_t)duty);
    return 0;
}

static int l_pwm_close(lua_State *L)
{
    int id = (int)luaL_checkinteger(L, 1);
    if (id >= 0 && id < N_PWM && s_pwm[id].used) {
        board_pwm_close(id);
        s_pwm[id].used = 0;
    }
    return 0;
}

/*
 * pwm.comp(freq?, duty?, dead_ns? [, hi, lo]) → id
 * or pwm.comp(hi, lo, freq?, duty?, dead_ns?) when arg1 is string.
 */
static int l_pwm_comp(lua_State *L)
{
    const char *hi = "PA8";
    const char *lo = "PA22";
    uint32_t freq = 20000;
    uint8_t duty = 50;
    uint32_t dead = 500;
    int st;
    if (lua_type(L, 1) == LUA_TSTRING) {
        hi = luaL_checkstring(L, 1);
        lo = luaL_checkstring(L, 2);
        freq = (uint32_t)luaL_optinteger(L, 3, 20000);
        duty = (uint8_t)luaL_optinteger(L, 4, 50);
        dead = (uint32_t)luaL_optinteger(L, 5, 500);
    } else {
        freq = (uint32_t)luaL_optinteger(L, 1, 20000);
        duty = (uint8_t)luaL_optinteger(L, 2, 50);
        dead = (uint32_t)luaL_optinteger(L, 3, 500);
        hi = luaL_optstring(L, 4, "PA8");
        lo = luaL_optstring(L, 5, "PA22");
    }
    if (board_pwm_comp_route(hi, lo, NULL, NULL, NULL) != 0) {
        return luaL_error(L, "pair");
    }
    st = board_pwm_comp_open(hi, lo, freq, duty, dead);
    if (st < 0) {
        return luaL_error(L, "%s/%s:%s", hi, lo, board_pin_errstr(st));
    }
    if (st >= 0 && st < N_COMP) {
        s_comp[st].used = 1;
        cpy8(s_comp[st].hi, hi);
        cpy8(s_comp[st].lo, lo);
    }
    lua_pushinteger(L, st);
    return 1;
}

static int l_pwm_comp_duty(lua_State *L)
{
    int id = 0;
    int duty;
    if (lua_gettop(L) >= 2) {
        id = (int)luaL_checkinteger(L, 1);
        duty = (int)luaL_checkinteger(L, 2);
    } else {
        duty = (int)luaL_checkinteger(L, 1);
    }
    board_pwm_comp_set_duty(id, (uint8_t)duty);
    return 0;
}

static int l_pwm_comp_close(lua_State *L)
{
    int id = (int)luaL_optinteger(L, 1, 0);
    if (id >= 0 && id < N_COMP) {
        board_pwm_comp_close(id);
        s_comp[id].used = 0;
    }
    return 0;
}

#ifdef LUA_BINARY_ONLY
static int l_oled_open(lua_State *L)
{
    const char *scl = luaL_optstring(L, 1, "PA15");
    const char *sda = luaL_optstring(L, 2, "PA16");
    int addr = (int)luaL_optinteger(L, 3, 0x3c);
    int hz = (int)luaL_optinteger(L, 4, 100000);
    int st;
    if (!strcmp(scl, "PA17") && !strcmp(sda, "PA18")) {
        return luaL_error(L,
            "oled pins PA17/PA18 unsupported: PA18 is BOOT; use PA15/PA16");
    }
    board_oled_close();
    /* Steal I2C1 from generic i2c.open(1) / prior oled. */
    if (s_i2c1.open) {
        board_i2c1_close(&s_i2c1);
    }
    board_pin_release_owner(PIN_OWN_I2C1);
    board_resource_release(BOARD_RES_I2C1, PIN_OWN_I2C1);
    board_pin_release_owner(PIN_OWN_OLED);
    board_resource_release(BOARD_RES_I2C1, PIN_OWN_OLED);
    if (claim_pair(L, scl, sda, PIN_OWN_OLED) != 0) {
        return 0;
    }
    if (board_resource_claim(BOARD_RES_I2C1, PIN_OWN_OLED) != 0) {
        board_pin_release_owner(PIN_OWN_OLED);
        return luaL_error(L, "oled:i2c1 busy");
    }
    st = board_oled_open(scl, sda, (uint8_t)addr, (uint32_t)hz);
    if (st != 0) {
        board_pin_release_owner(PIN_OWN_OLED);
        board_resource_release(BOARD_RES_I2C1, PIN_OWN_OLED);
        if (st == -1) {
            return luaL_error(L,
                "oled I2C1 unavailable on %s/%s", scl, sda);
        }
        if (st == -2) {
            return luaL_error(L,
                "oled no ACK at addr %d on %s/%s",
                addr & 0x7F, scl, sda);
        }
        return luaL_error(L, "oled open %d", st);
    }
    return 0;
}

static int l_oled_close(lua_State *L)
{
    (void)L;
    board_oled_close();
    board_pin_release_owner(PIN_OWN_OLED);
    board_resource_release(BOARD_RES_I2C1, PIN_OWN_OLED);
    return 0;
}

static int l_oled_clear(lua_State *L)
{
    if (board_oled_clear() != 0) {
        return luaL_error(L, "oled clear");
    }
    return 0;
}

/* oled.fill([byte]) default 0xFF full white — hardware smoke without font. */
static int l_oled_fill(lua_State *L)
{
    if (board_oled_fill((uint8_t)luaL_optinteger(L, 1, 0xff)) != 0) {
        return luaL_error(L, "oled fill");
    }
    return 0;
}

static int l_oled_cursor(lua_State *L)
{
    if (board_oled_cursor((uint8_t)luaL_checkinteger(L, 1),
            (uint8_t)luaL_checkinteger(L, 2)) != 0) {
        return luaL_error(L, "oled cursor");
    }
    return 0;
}

static int l_oled_print(lua_State *L)
{
    if (board_oled_puts(luaL_checkstring(L, 1)) != 0) {
        return luaL_error(L, "oled print");
    }
    return 0;
}

static int l_oled_num(lua_State *L)
{
    /* oled.num(x, page, value, dec?)  value is fixed-point * 10^dec */
    if (board_oled_num((uint8_t)luaL_checkinteger(L, 1),
            (uint8_t)luaL_checkinteger(L, 2),
            (int32_t)luaL_checkinteger(L, 3),
            (uint8_t)luaL_optinteger(L, 4, 1)) != 0) {
        return luaL_error(L, "oled num");
    }
    return 0;
}

/* oled.wave(raw_le_u16) — pages 0..6 trace */
static int l_oled_wave(lua_State *L)
{
    size_t len = 0;
    const char *p = luaL_checklstring(L, 1, &len);
    size_t n = len / 2u;
    uint16_t buf[BOARD_ADC_DMA_MAX];
    size_t i;
    if (n < 2u || n > BOARD_ADC_DMA_MAX || (len & 1u)) {
        return luaL_error(L, "wave");
    }
    for (i = 0; i < n; i++) {
        buf[i] = (uint16_t)((uint8_t)p[i * 2u] | ((uint16_t)(uint8_t)p[i * 2u + 1u] << 8));
    }
    if (board_oled_wave(buf, n) != 0) {
        return luaL_error(L, "wave");
    }
    return 0;
}

static int l_oled_ready(lua_State *L)
{
    lua_pushboolean(L, board_oled_ready() != 0);
    return 1;
}

/* oled.glyph(code, b0..b5) — 6 columns of 8px */
static int l_oled_glyph(lua_State *L)
{
    uint8_t g[6];
    int i;
    int code = (int)luaL_checkinteger(L, 1);
    if (code < 0 || code > 255) {
        return luaL_error(L, "code");
    }
    for (i = 0; i < 6; i++) {
        g[i] = (uint8_t)luaL_checkinteger(L, 2 + i);
    }
    if (board_oled_glyph_set((uint8_t)code, g) != 0) {
        return luaL_error(L, "glyph bank full");
    }
    return 0;
}

static int l_oled_glyph_clear(lua_State *L)
{
    (void)L;
    board_oled_glyph_clear();
    return 0;
}

/* oled.font(path?) default _run.fnt — load F6 pack from LittleFS */
static int l_oled_font(lua_State *L)
{
    const char *path = luaL_optstring(L, 1, "_run.fnt");
    int n = board_oled_font_load(path);
    if (n < 0) {
        lua_pushnil(L);
        return 1;
    }
    lua_pushinteger(L, n);
    return 1;
}

static int l_oled_has(lua_State *L)
{
    int code = (int)luaL_checkinteger(L, 1);
    lua_pushboolean(L, board_oled_has_glyph((uint8_t)code) != 0);
    return 1;
}

/* oled.text(x, page, utf8) — 16x16 bank (CJK/Latin from _run.f16) */
static int l_oled_text(lua_State *L)
{
    if (board_oled_text((uint8_t)luaL_checkinteger(L, 1),
            (uint8_t)luaL_checkinteger(L, 2),
            luaL_checkstring(L, 3)) != 0) {
        return luaL_error(L, "oled text");
    }
    return 0;
}

/* oled.font16(path?) default _run.f16 */
static int l_oled_font16(lua_State *L)
{
    const char *path = luaL_optstring(L, 1, "_run.f16");
    int n = board_oled_font16_load(path);
    if (n < 0) {
        lua_pushnil(L);
        return 1;
    }
    lua_pushinteger(L, n);
    return 1;
}

/* oled.cjk(code, 32 bytes as string or 32 ints) */
static int l_oled_cjk(lua_State *L)
{
    uint8_t bmp[32];
    int code = (int)luaL_checkinteger(L, 1);
    size_t n;
    int i;
    if (code < 0 || code > 0xffff) {
        return luaL_error(L, "code");
    }
    if (lua_type(L, 2) == LUA_TSTRING) {
        const char *s = luaL_checklstring(L, 2, &n);
        if (n != 32) {
            return luaL_error(L, "bmp 32");
        }
        memcpy(bmp, s, 32);
    } else {
        for (i = 0; i < 32; i++) {
            bmp[i] = (uint8_t)luaL_checkinteger(L, 2 + i);
        }
    }
    if (board_oled_cjk_set((uint16_t)code, bmp) != 0) {
        return luaL_error(L, "cjk bank full");
    }
    return 0;
}

static int l_oled_cjk_clear(lua_State *L)
{
    (void)L;
    board_oled_cjk_clear();
    return 0;
}

static int l_oled_has_cjk(lua_State *L)
{
    int code = (int)luaL_checkinteger(L, 1);
    lua_pushboolean(L, board_oled_has_cjk((uint16_t)code) != 0);
    return 1;
}

#endif

/* IQ16 fixed-point: values are opaque int32 Q16.16 (1.0 == 65536). */
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
    /* arg: degrees ×10 */
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
    /* iq.atan2_deg(y, x) → degrees ×10 */
    lua_pushinteger(L, iq16_atan2_deg_x10((iq16_t)luaL_checkinteger(L, 1),
        (iq16_t)luaL_checkinteger(L, 2)));
    return 1;
}

/* ---- pid: positional / incremental / cascade (C hot path) ---- */
static int l_pid_open(lua_State *L)
{
    const char *mode = luaL_optstring(L, 1, "pos");
    int m = (mode[0] == 'i' || mode[0] == 'I') ? BOARD_PID_INC : BOARD_PID_POS;
    int id = board_pid_open(m);
    if (id < 0) {
        return luaL_error(L, "no pid");
    }
    lua_pushinteger(L, id);
    return 1;
}

static int l_pid_close(lua_State *L)
{
    board_pid_close((int)luaL_checkinteger(L, 1));
    return 0;
}

static int l_pid_reset(lua_State *L)
{
    board_pid_reset((int)luaL_checkinteger(L, 1));
    return 0;
}

/* pid.tune(id, kp_x100, ki_x100, kd_x100) — gains ×100 for easy Lua ints */
static int l_pid_tune(lua_State *L)
{
    int id = (int)luaL_checkinteger(L, 1);
    board_pid_tune(id, iq16_from_x100((int32_t)luaL_checkinteger(L, 2)),
        iq16_from_x100((int32_t)luaL_checkinteger(L, 3)),
        iq16_from_x100((int32_t)luaL_checkinteger(L, 4)));
    return 0;
}

/* pid.tune_iq(id, kp, ki, kd) — raw IQ16 (from iq.from_x10 etc.) */
static int l_pid_tune_iq(lua_State *L)
{
    board_pid_tune((int)luaL_checkinteger(L, 1),
        (int32_t)luaL_checkinteger(L, 2),
        (int32_t)luaL_checkinteger(L, 3),
        (int32_t)luaL_checkinteger(L, 4));
    return 0;
}

static int l_pid_limit(lua_State *L)
{
    board_pid_out_limit((int)luaL_checkinteger(L, 1),
        (int32_t)luaL_checkinteger(L, 2),
        (int32_t)luaL_checkinteger(L, 3));
    return 0;
}

static int l_pid_ilimit(lua_State *L)
{
    board_pid_i_limit((int)luaL_checkinteger(L, 1),
        (int32_t)luaL_checkinteger(L, 2));
    return 0;
}

/* pid.step(id, sp, fb, dt_ms) → u */
static int l_pid_step(lua_State *L)
{
    lua_pushinteger(L, board_pid_step((int)luaL_checkinteger(L, 1),
        (int32_t)luaL_checkinteger(L, 2),
        (int32_t)luaL_checkinteger(L, 3),
        (uint32_t)luaL_optinteger(L, 4, 1)));
    return 1;
}

/* pid.cascade(outer, inner, sp, fb_out, fb_in, dt_ms) → u */
static int l_pid_cascade(lua_State *L)
{
    lua_pushinteger(L, board_pid_cascade(
        (int)luaL_checkinteger(L, 1),
        (int)luaL_checkinteger(L, 2),
        (int32_t)luaL_checkinteger(L, 3),
        (int32_t)luaL_checkinteger(L, 4),
        (int32_t)luaL_checkinteger(L, 5),
        (uint32_t)luaL_optinteger(L, 6, 1)));
    return 1;
}

/* ---- filt ---- */
static int l_filt_open(lua_State *L)
{
    const char *k = luaL_optstring(L, 1, "lp");
    int kind = (k[0] == 'm' || k[0] == 'M') ? BOARD_FILT_MA : BOARD_FILT_LP;
    int id = board_filt_open(kind);
    if (id < 0) {
        return luaL_error(L, "no filt");
    }
    if (!lua_isnoneornil(L, 2)) {
        board_filt_config(id, (int)luaL_checkinteger(L, 2));
    }
    lua_pushinteger(L, id);
    return 1;
}

static int l_filt_close(lua_State *L)
{
    board_filt_close((int)luaL_checkinteger(L, 1));
    return 0;
}

static int l_filt_reset(lua_State *L)
{
    board_filt_reset((int)luaL_checkinteger(L, 1));
    return 0;
}

static int l_filt_config(lua_State *L)
{
    board_filt_config((int)luaL_checkinteger(L, 1), (int)luaL_checkinteger(L, 2));
    return 0;
}

static int l_filt_update(lua_State *L)
{
    lua_pushinteger(L, board_filt_update((int)luaL_checkinteger(L, 1),
        (int32_t)luaL_checkinteger(L, 2)));
    return 1;
}

static int l_filt_get(lua_State *L)
{
    lua_pushinteger(L, board_filt_get((int)luaL_checkinteger(L, 1)));
    return 1;
}

/* ---- btn ---- */
static int l_btn_open(lua_State *L)
{
    const char *pin = luaL_checkstring(L, 1);
    uint32_t deb = (uint32_t)luaL_optinteger(L, 2, 15);
    uint32_t lng = (uint32_t)luaL_optinteger(L, 3, 600);
    int id = board_btn_open(pin, deb, lng);
    if (id < 0) {
        return luaL_error(L, "btn");
    }
    lua_pushinteger(L, id);
    return 1;
}

static int l_btn_close(lua_State *L)
{
    board_btn_close((int)luaL_checkinteger(L, 1));
    return 0;
}

static int l_btn_scan(lua_State *L)
{
    lua_pushinteger(L, board_btn_scan());
    return 1;
}

static int l_btn_event(lua_State *L)
{
    int ev = board_btn_event((int)luaL_checkinteger(L, 1));
    if (ev == BOARD_BTN_PRESS) {
        lua_pushstring(L, "press");
    } else if (ev == BOARD_BTN_RELEASE) {
        lua_pushstring(L, "release");
    } else if (ev == BOARD_BTN_LONG) {
        lua_pushstring(L, "long");
    } else {
        lua_pushnil(L);
    }
    return 1;
}

static int l_btn_down(lua_State *L)
{
    lua_pushboolean(L, board_btn_down((int)luaL_checkinteger(L, 1)) != 0);
    return 1;
}

static int l_btn_held_ms(lua_State *L)
{
    lua_pushinteger(L, (lua_Integer)board_btn_held_ms((int)luaL_checkinteger(L, 1)));
    return 1;
}

/* ---- enc ---- */
static int l_enc_open(lua_State *L)
{
    int id = board_enc_open(luaL_checkstring(L, 1), luaL_checkstring(L, 2));
    if (id < 0) {
        return luaL_error(L, "enc");
    }
    lua_pushinteger(L, id);
    return 1;
}

static int l_enc_close(lua_State *L)
{
    board_enc_close((int)luaL_checkinteger(L, 1));
    return 0;
}

static int l_enc_set(lua_State *L)
{
    board_enc_set((int)luaL_checkinteger(L, 1), (int32_t)luaL_checkinteger(L, 2));
    return 0;
}

static int l_enc_pos(lua_State *L)
{
    lua_pushinteger(L, board_enc_pos((int)luaL_checkinteger(L, 1)));
    return 1;
}

static int l_enc_delta(lua_State *L)
{
    lua_pushinteger(L, board_enc_delta((int)luaL_checkinteger(L, 1)));
    return 1;
}

static int l_enc_cps(lua_State *L)
{
    lua_pushinteger(L, board_enc_cps((int)luaL_checkinteger(L, 1)));
    return 1;
}

static int l_enc_poll(lua_State *L)
{
    (void)L;
    board_enc_poll();
    return 0;
}

/* ---- wdt ---- */
static int l_wdt_start(lua_State *L)
{
    if (board_wdt_start((uint32_t)luaL_optinteger(L, 1, 500)) != 0) {
        return luaL_error(L, "wdt");
    }
    return 0;
}

static int l_wdt_feed(lua_State *L)
{
    (void)L;
    board_wdt_feed();
    return 0;
}

static int l_wdt_active(lua_State *L)
{
    lua_pushboolean(L, board_wdt_active() != 0);
    return 1;
}

/* ---- ramp ---- */
static int l_ramp_open(lua_State *L)
{
    int id = board_ramp_open();
    if (id < 0) {
        return luaL_error(L, "no ramp");
    }
    if (!lua_isnoneornil(L, 1)) {
        board_ramp_config(id, (int32_t)luaL_checkinteger(L, 1));
    }
    lua_pushinteger(L, id);
    return 1;
}

static int l_ramp_close(lua_State *L)
{
    board_ramp_close((int)luaL_checkinteger(L, 1));
    return 0;
}

static int l_ramp_config(lua_State *L)
{
    board_ramp_config((int)luaL_checkinteger(L, 1), (int32_t)luaL_checkinteger(L, 2));
    return 0;
}

static int l_ramp_set(lua_State *L)
{
    board_ramp_set((int)luaL_checkinteger(L, 1), (int32_t)luaL_checkinteger(L, 2));
    return 0;
}

static int l_ramp_jump(lua_State *L)
{
    board_ramp_jump((int)luaL_checkinteger(L, 1), (int32_t)luaL_checkinteger(L, 2));
    return 0;
}

static int l_ramp_step(lua_State *L)
{
    lua_pushinteger(L, board_ramp_step((int)luaL_checkinteger(L, 1)));
    return 1;
}

static int l_ramp_get(lua_State *L)
{
    lua_pushinteger(L, board_ramp_get((int)luaL_checkinteger(L, 1)));
    return 1;
}

static int l_ramp_done(lua_State *L)
{
    lua_pushboolean(L, board_ramp_done((int)luaL_checkinteger(L, 1)) != 0);
    return 1;
}

/* ---- util (global table) ---- */
static int l_util_clamp(lua_State *L)
{
    lua_pushinteger(L, board_clamp(
        (int32_t)luaL_checkinteger(L, 1),
        (int32_t)luaL_checkinteger(L, 2),
        (int32_t)luaL_checkinteger(L, 3)));
    return 1;
}

static int l_util_deadzone(lua_State *L)
{
    lua_pushinteger(L, board_deadzone(
        (int32_t)luaL_checkinteger(L, 1),
        (int32_t)luaL_checkinteger(L, 2)));
    return 1;
}

static int l_util_map(lua_State *L)
{
    lua_pushinteger(L, board_map(
        (int32_t)luaL_checkinteger(L, 1),
        (int32_t)luaL_checkinteger(L, 2),
        (int32_t)luaL_checkinteger(L, 3),
        (int32_t)luaL_checkinteger(L, 4),
        (int32_t)luaL_checkinteger(L, 5)));
    return 1;
}

static int l_util_med3(lua_State *L)
{
    lua_pushinteger(L, board_med3(
        (int32_t)luaL_checkinteger(L, 1),
        (int32_t)luaL_checkinteger(L, 2),
        (int32_t)luaL_checkinteger(L, 3)));
    return 1;
}

static int l_util_slew(lua_State *L)
{
    lua_pushinteger(L, board_slew(
        (int32_t)luaL_checkinteger(L, 1),
        (int32_t)luaL_checkinteger(L, 2),
        (int32_t)luaL_checkinteger(L, 3)));
    return 1;
}

static int l_util_avg(lua_State *L)
{
    int32_t buf[32];
    int n = 0;
    int i;
    luaL_checktype(L, 1, LUA_TTABLE);
    n = (int)lua_rawlen(L, 1);
    if (n < 1) {
        lua_pushinteger(L, 0);
        return 1;
    }
    if (n > 32) {
        n = 32;
    }
    for (i = 0; i < n; i++) {
        lua_rawgeti(L, 1, i + 1);
        buf[i] = (int32_t)luaL_checkinteger(L, -1);
        lua_pop(L, 1);
    }
    lua_pushinteger(L, board_avg_n(buf, n));
    return 1;
}

static int l_util_sign(lua_State *L)
{
    lua_pushinteger(L, board_sign((int32_t)luaL_checkinteger(L, 1)));
    return 1;
}

/* crc.crc8(s[, init]) / crc.modbus(s) — binary string */
static int l_crc_crc8(lua_State *L)
{
    size_t n = 0;
    const char *s = luaL_checklstring(L, 1, &n);
    uint8_t init = (uint8_t)luaL_optinteger(L, 2, 0);
    lua_pushinteger(L, board_crc8((const uint8_t *)s, n, init));
    return 1;
}

static int l_crc_modbus(lua_State *L)
{
    size_t n = 0;
    const char *s = luaL_checklstring(L, 1, &n);
    lua_pushinteger(L, board_crc16_modbus((const uint8_t *)s, n));
    return 1;
}

/* ---- cap ---- */
static int l_cap_open(lua_State *L)
{
    const char *pin = luaL_optstring(L, 1, "PA22");
    int edge = (int)luaL_optinteger(L, 2, 0);
    int st = board_cap_open(pin, edge);
    if (st != 0) {
        return luaL_error(L, "cap");
    }
    return 0;
}

static int l_cap_close(lua_State *L)
{
    (void)L;
    board_cap_close();
    return 0;
}

static int l_cap_period(lua_State *L)
{
    lua_pushinteger(L, (lua_Integer)board_cap_period());
    return 1;
}

static int l_cap_hz_x10(lua_State *L)
{
    lua_pushinteger(L, (lua_Integer)board_cap_hz_x10());
    return 1;
}

static int l_cap_ready(lua_State *L)
{
    lua_pushboolean(L, board_cap_ready() != 0);
    return 1;
}

static int l_cap_hits(lua_State *L)
{
    lua_pushinteger(L, (lua_Integer)board_cap_hits());
    return 1;
}

/* ---- qei (hw TIMG8) + qgen stim ---- */
static int l_qei_open(lua_State *L)
{
    if (board_qei_open() != 0) {
        return luaL_error(L, "qei");
    }
    return 0;
}

static int l_qei_close(lua_State *L)
{
    (void)L;
    board_qei_close();
    return 0;
}

static int l_qei_set(lua_State *L)
{
    board_qei_set((int32_t)luaL_checkinteger(L, 1));
    return 0;
}

static int l_qei_pos(lua_State *L)
{
    lua_pushinteger(L, board_qei_pos());
    return 1;
}

static int l_qei_delta(lua_State *L)
{
    lua_pushinteger(L, board_qei_delta());
    return 1;
}

static int l_qei_dir(lua_State *L)
{
    lua_pushinteger(L, board_qei_dir());
    return 1;
}

static int l_qei_active(lua_State *L)
{
    lua_pushboolean(L, board_qei_active() != 0);
    return 1;
}

/* qei.stim(steps[, dir[, half_us]]) — PA14/PA25 gray code (wire to PA26/PA27) */
static int l_qei_stim(lua_State *L)
{
    int steps = (int)luaL_checkinteger(L, 1);
    int dir = (int)luaL_optinteger(L, 2, 1);
    uint32_t half = (uint32_t)luaL_optinteger(L, 3, 200);
    if (board_qgen_run(steps, dir, half) != 0) {
        return luaL_error(L, "stim");
    }
    return 0;
}

/* ---- runtime diagnostics / bounded-GC control ---- */
static int l_sys_mem(lua_State *L)
{
    lua_runtime_mem_t m;
    lua_runtime_mem(&m);
    lua_createtable(L, 0, 9);
#define MEM_FIELD(k, v) do { \
    lua_pushinteger(L, (lua_Integer)(v)); lua_setfield(L, -2, (k)); \
} while (0)
    MEM_FIELD("capacity", m.capacity);
    MEM_FIELD("used", m.used);
    MEM_FIELD("free", m.free);
    MEM_FIELD("largest_free", m.largest_free);
    MEM_FIELD("blocks", m.blocks);
    MEM_FIELD("free_blocks", m.free_blocks);
    MEM_FIELD("stack_free_now", m.stack_free_now);
    MEM_FIELD("gc_kb", lua_gc(L, LUA_GCCOUNT));
    MEM_FIELD("gc_bytes", lua_gc(L, LUA_GCCOUNTB));
#undef MEM_FIELD
    return 1;
}

static int l_sys_gc(lua_State *L)
{
    int done;
    if (lua_isnoneornil(L, 1)) {
        (void)lua_gc(L, LUA_GCCOLLECT);
        done = 1;
    } else {
        done = lua_gc(L, LUA_GCSTEP, (int)luaL_checkinteger(L, 1));
    }
    lua_pushboolean(L, done != 0);
    return 1;
}

static const char *const k_resource_name[BOARD_RES_COUNT] = {
    "TIMG0", "TIMG6", "TIMG7", "TIMG8", "TIMG12", "TIMA0", "TIMA1",
    "ADC0", "DMA0", "I2C0", "I2C1", "SPI0", "UART1", "UART2", "UART3",
    "WWDT1",
};

static int l_sys_resource(lua_State *L)
{
    int i;
    lua_createtable(L, 0, BOARD_RES_COUNT);
    for (i = 0; i < BOARD_RES_COUNT; i++) {
        lua_pushstring(L, board_pin_owner_str(
            board_resource_owner((board_resource_t)i)));
        lua_setfield(L, -2, k_resource_name[i]);
    }
    return 1;
}

#include "lua_bind_registry.inc"
