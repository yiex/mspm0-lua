#include <string.h>

#include "board_delay.h"
#include "board_dma.h"
#include "board_irq.h"
#include "board_lfs.h"
#include "board_spiflash.h"
#include "board_status.h"
#include "board_uart.h"
#include "board_wdt.h"
#include "board_reg.h"
#include "board_pins.h"
#include "board_resource.h"
#include "ti_msp_dl_config.h"

#include "lua.h"
#include "lauxlib.h"
#include "lualib.h"
#include "lua_bind.h"
#include "lua_runtime.h"
#ifdef LUA_BINARY_ONLY
#include "native_module.h"
#endif
#ifdef MSPM0_MODULAR_CORE
#include "module_update.h"
#include "release_identity.h"
#endif

#define RX_LINE_MAX 256
#define UPLOAD_TEMP ".upload.tmp"
#define UPLOAD_NONE 0
#define UPLOAD_LUA  1
#define UPLOAD_HEX  2
static char s_line[RX_LINE_MAX];
static size_t s_line_n;
static int s_line_overflow;
static int s_upload_mode;
static uint32_t s_upload_size;
static char s_upload_name[32];

int _write(int fd, char *ptr, int len)
{
    (void)fd;
    board_uart_write(ptr, (size_t)len);
    return len;
}

static void led_blink(int n)
{
    for (int i = 0; i < n; i++) {
        board_reg_gpio_set(GPIO_LEDS_PORT, GPIO_LEDS_USER_LED_PIN);
        board_delay_ms(80);
        board_reg_gpio_clr(GPIO_LEDS_PORT, GPIO_LEDS_USER_LED_PIN);
        board_delay_ms(80);
    }
}

#if !defined(LUA_BINARY_ONLY) && !defined(LUA_SOURCE_FULL_TIGHT)
static const char k_demo_script[] =
    "print(1+2)\n"
    "gpio.mode('PA14','out')\n"
    "gpio.set('PA14',0)\n";
#endif

typedef struct {
    char data[128];
    int error;
} lua_lfs_reader_t;

static const char *lua_lfs_reader(lua_State *L, void *ud, size_t *size)
{
    lua_lfs_reader_t *r = (lua_lfs_reader_t *)ud;
    int n;
    (void)L;
    n = board_lfs_read_chunk(r->data, sizeof(r->data));
    if (n <= 0) {
        if (n < 0) {
            r->error = n;
        }
        *size = 0;
        return NULL;
    }
    *size = (size_t)n;
    return r->data;
}

static int report_lua_result(lua_State *L, int st, const char *phase)
{
    if (st != LUA_OK) {
        const char *err = lua_tostring(L, -1);
        if (err && strstr(err, "STOP")) {
            board_uart_puts("LUA stopped\n");
        } else {
            board_uart_puts("LUA ");
            board_uart_puts(phase);
            board_uart_puts(" err: ");
            board_uart_puts(err ? err : "?");
            board_uart_puts("\n");
        }
        lua_pop(L, 1);
    }
    return st;
}

#if !defined(LUA_BINARY_ONLY) && !defined(LUA_SOURCE_FULL_TIGHT)
static int run_lua_string(lua_State *L, const char *src, const char *name)
{
    int st;
    lua_bind_clear_stop();
    st = luaL_loadbufferx(L, src, strlen(src), name, "t");
    if (st != LUA_OK) {
        return report_lua_result(L, st, "load");
    }
    st = lua_pcall(L, 0, 0, 0);
    return report_lua_result(L, st, "run");
}
#endif

static int lfs_file_exists(const char *path)
{
    int ok = board_lfs_read_open(path) == 0;
    if (ok) {
        (void)board_lfs_read_close();
    }
    return ok;
}

static int load_lua_file(lua_State *L, const char *path)
{
    lua_lfs_reader_t reader;
    int st, close_st;
    const char *mode;
    memset(&reader, 0, sizeof(reader));
    if (board_lfs_read_open(path) < 0) {
        lua_pushliteral(L, "file open");
        return LUA_ERRFILE;
    }
#ifdef LUA_BINARY_ONLY
    mode = "b";
#else
    mode = "bt";
#endif
    st = lua_load(L, lua_lfs_reader, &reader, path, mode);
    close_st = board_lfs_read_close();
    if (reader.error || close_st < 0) {
        lua_pop(L, 1);
        lua_pushliteral(L, "file read");
        return LUA_ERRFILE;
    }
    return st;
}

static int run_lua_file(lua_State *L, const char *path)
{
    int st;
    lua_bind_clear_stop();
    st = load_lua_file(L, path);
    if (st != LUA_OK) {
        return report_lua_result(L, st, "load");
    }
    st = lua_pcall(L, 0, 0, 0);
    return report_lua_result(L, st, "run");
}

static void probe_spi_flash(void)
{
    uint8_t id[3] = {0, 0, 0};
    board_spiflash_init();
    if (board_spiflash_read_jedec(id)) {
        board_status_set_jedec(((uint32_t)id[0] << 16) |
            ((uint32_t)id[1] << 8) | id[2]);
        board_status_or(ST_SPI_OK);
        board_uart_puts("JEDEC OK\n");
    } else {
        board_status_or(ST_SPI_FAIL);
        board_uart_puts("JEDEC FAIL\n");
    }
}

static int run_boot_script(lua_State *L)
{
    if (board_lfs_ready()) {
        if (lfs_file_exists("main.luac")) {
            board_status_or(ST_SCRIPT_EXT);
            board_uart_puts("main.luac\n");
            return run_lua_file(L, "main.luac");
        }
#ifndef LUA_BINARY_ONLY
        if (lfs_file_exists("main.lua")) {
            board_status_or(ST_SCRIPT_EXT);
            board_uart_puts("main.lua\n");
            return run_lua_file(L, "main.lua");
        }
#endif
    }
#if !defined(LUA_BINARY_ONLY) && !defined(LUA_SOURCE_FULL_TIGHT)
    board_uart_puts("builtin\n");
    return run_lua_string(L, k_demo_script, "builtin");
#else
    board_uart_puts("NO main\n");
    return LUA_ERRFILE;
#endif
}

static int l_runfile(lua_State *L);
static int l_require_file(lua_State *L);

static void lua_destroy(lua_State *L)
{
#ifdef LUA_BINARY_ONLY
    if (L) native_module_deinit();
#endif
    lua_bind_reset_runtime(L);
    if (L) {
        lua_close(L);
    }
}

static lua_State *lua_recreate(lua_State *L)
{
    lua_destroy(L);
    lua_runtime_heap_init();
    L = lua_newstate(lua_runtime_alloc, NULL, 0x12345678u);
    if (!L) {
        return NULL;
    }
    luaL_requiref(L, LUA_GNAME, luaopen_base, 1);
    lua_pop(L, 1);
    lua_bind_register(L);
#ifdef LUA_BINARY_ONLY
    (void)native_module_register(L);
#endif
    /* Incremental GC keeps pauses bounded on a 21 KiB fixed heap. */
    (void)lua_gc(L, LUA_GCINC, 150, 200, 10);
    lua_pushcfunction(L, l_runfile);
    lua_setglobal(L, "runfile");
    lua_pushcfunction(L, l_require_file);
    lua_setglobal(L, "require");
    return L;
}

static int valid_name(const char *n)
{
    size_t i;
    if (!n || !n[0] || strlen(n) > 28) {
        return 0;
    }
    for (i = 0; n[i]; i++) {
        char c = n[i];
        int ok = (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') ||
            (c >= '0' && c <= '9') || c == '_' || c == '.' || c == '-';
        if (!ok) {
            return 0;
        }
    }
    return 1;
}

static int has_suffix(const char *s, const char *suffix)
{
    size_t ns = strlen(s), nx = strlen(suffix);
    return ns >= nx && strcmp(s + ns - nx, suffix) == 0;
}

/* Side-effect module loader: scripts can opt into only the .luac files used. */
static int l_runfile(lua_State *L)
{
    const char *name = luaL_checkstring(L, 1);
    if (!valid_name(name)) {
        return luaL_error(L, "file name");
    }
#ifdef LUA_BINARY_ONLY
    if (!has_suffix(name, ".luac")) {
        return luaL_error(L, "bytecode only");
    }
#endif
    lua_pushboolean(L, run_lua_file(L, name) == LUA_OK);
    return 1;
}

/*
 * Small LittleFS-backed replacement for package.require.  Modules are target
 * bytecode files that return a value (normally a table).  Each module runs at
 * most once per Lua VM and its return value is cached in the registry.
 */
static int l_require_file(lua_State *L)
{
    static const char cache_key[] = "MSP_REQUIRE_CACHE";
    const char *requested = luaL_checkstring(L, 1);
    char name[32];
    size_t n = strlen(requested);
    int st;

    if (has_suffix(requested, ".luac")) {
        if (!valid_name(requested)) {
            return luaL_error(L, "module name");
        }
        strcpy(name, requested);
    } else {
        if (n == 0 || n + 5u > 28u) {
            return luaL_error(L, "module name");
        }
        memcpy(name, requested, n);
        memcpy(name + n, ".luac", 6);
        if (!valid_name(name)) {
            return luaL_error(L, "module name");
        }
    }

    lua_getfield(L, LUA_REGISTRYINDEX, cache_key);
    if (!lua_istable(L, -1)) {
        lua_pop(L, 1);
        lua_newtable(L);
        lua_pushvalue(L, -1);
        lua_setfield(L, LUA_REGISTRYINDEX, cache_key);
    }

    lua_getfield(L, -1, name);
    if (!lua_isnil(L, -1)) {
        lua_remove(L, -2);
        return 1;
    }
    lua_pop(L, 1);

    /* A temporary true value breaks accidental circular imports. */
    lua_pushboolean(L, 1);
    lua_setfield(L, -2, name);

    st = load_lua_file(L, name);
    if (st == LUA_OK) {
        st = lua_pcall(L, 0, 1, 0);
    }
    if (st != LUA_OK) {
        lua_pushnil(L);
        lua_setfield(L, -3, name);
        return lua_error(L);
    }

    if (lua_isnil(L, -1)) {
        lua_pop(L, 1);
        lua_pushboolean(L, 1);
    }
    lua_pushvalue(L, -1);
    lua_setfield(L, -3, name);
    lua_remove(L, -2);
    return 1;
}

static void u32dec(char *d, uint32_t v)
{
    char t[10];
    int i = 0, j;
    if (v == 0) {
        d[0] = '0';
        d[1] = 0;
        return;
    }
    while (v && i < 10) {
        t[i++] = (char)('0' + (v % 10u));
        v /= 10u;
    }
    j = 0;
    while (i--) {
        d[j++] = t[i];
    }
    d[j] = 0;
}

static void u32hex(char *d, uint32_t v, unsigned digits)
{
    static const char hex[] = "0123456789abcdef";
    unsigned i;
    for (i = 0; i < digits; i++) {
        d[digits - 1u - i] = hex[v & 0x0fu];
        v >>= 4;
    }
    d[digits] = 0;
}

#ifdef MSPM0_MODULAR_CORE
static void cmd_fwinfo(void)
{
    char num[12];
    board_uart_puts("FW_INFO " FW_RELEASE_ID " " FW_RELEASE_VERSION "\n");
    board_uart_puts("FW_TARGET " FW_RELEASE_TARGET "\n");
    u32dec(num, NATIVE_CORE_ABI_VERSION);
    board_uart_puts("FW_ABI ");
    board_uart_puts(num);
    board_uart_puts("\n");
    u32dec(num, NATIVE_MODULE_FORMAT);
    board_uart_puts("FW_MODULE_FORMAT ");
    board_uart_puts(num);
    board_uart_puts("\n");
    u32dec(num, FW_RELEASE_NMUP_FORMAT);
    board_uart_puts("FW_NMUP_FORMAT ");
    board_uart_puts(num);
    board_uart_puts("\n");
    u32dec(num, NATIVE_MODULE_SLOT_COUNT);
    board_uart_puts("FW_SLOTS ");
    board_uart_puts(num);
    board_uart_puts(" ");
    u32dec(num, NATIVE_MODULE_SLOT_SIZE);
    board_uart_puts(num);
    board_uart_puts("\n");
    board_uart_puts("FW_CATALOG " FW_RELEASE_CATALOG_SHA256 "\n");
    board_uart_puts("FW_INFO_END\n");
}
#endif

static void cmd_baud(const char *value)
{
    uint32_t baud = 0;
    char num[12];
    const char *p = value;
    if (!p || !*p) {
        board_uart_puts("BAUD_ERR\n");
        return;
    }
    while (*p >= '0' && *p <= '9') {
        uint32_t digit = (uint32_t)(*p++ - '0');
        if (baud > 1000000u / 10u) {
            board_uart_puts("BAUD_ERR\n");
            return;
        }
        baud = baud * 10u + digit;
    }
    if (*p || (baud != 115200u && baud != 460800u)) {
        board_uart_puts("BAUD_ERR\n");
        return;
    }
    u32dec(num, baud);
    if (baud == board_uart_get_baud()) {
        board_uart_puts("BAUD_OK ");
        board_uart_puts(num);
        board_uart_puts("\n");
        return;
    }
    board_uart_puts("BAUD_SWITCH ");
    board_uart_puts(num);
    board_uart_puts("\n");
    /* Allow hosts with buffered reads to consume the old-rate acknowledgement
     * and reconfigure their UART before the first high-rate byte is sent. */
    board_delay_ms(300);
    if (board_uart_set_baud(baud) != 0) {
        return;
    }
    /* Do not emit unsolicited bytes at the new divisor: a USB-UART bridge may
     * drop that first frame. The host confirms by sending the same command at
     * the new rate; the current-rate branch above then returns BAUD_OK. */
    board_delay_ms(50);
}

static void list_cb(const char *name, uint32_t size, void *ud)
{
    char num[12];
    (void)ud;
    board_uart_puts("F ");
    board_uart_puts(name);
    board_uart_puts(" ");
    u32dec(num, size);
    board_uart_puts(num);
    board_uart_puts("\n");
}

static void cmd_ls(void)
{
    board_uart_puts("LS\n");
    if (board_lfs_ready()) {
        (void)board_lfs_list(list_cb, NULL);
    }
    board_uart_puts("LS_END\n");
}

static void cmd_format(void)
{
    char num[12];
#ifdef MSPM0_MODULAR_CORE
    if (module_update_has_pending()) {
        board_uart_puts("FORMAT_ERR pending\n");
        return;
    }
#endif
    if (!board_lfs_format()) {
        board_uart_puts("FORMAT_ERR\n");
        return;
    }
    u32dec(num, board_lfs_capacity_bytes());
    board_uart_puts("FORMAT_OK ");
    board_uart_puts(num);
    board_uart_puts("\n");
}

#ifndef LUA_SOURCE_FULL_TIGHT
static void cmd_get(const char *name)
{
    char buf[64];
    int n;
    char last = '\n';
    if (!valid_name(name) || board_lfs_read_open(name) < 0) {
        board_uart_puts("GET_ERR\n");
        return;
    }
    board_uart_puts("GET_BEGIN ");
    board_uart_puts(name);
    board_uart_puts("\n");
    while ((n = board_lfs_read_chunk(buf, sizeof(buf))) > 0) {
        board_uart_write(buf, (size_t)n);
        last = buf[n - 1];
    }
    (void)board_lfs_read_close();
    if (last != '\n') {
        board_uart_puts("\n");
    }
    board_uart_puts(n < 0 ? "GET_ERR\n" : "GET_END\n");
}

static void cmd_rm(const char *name)
{
#ifdef MSPM0_MODULAR_CORE
    if (module_update_has_pending()) {
        board_uart_puts("RM_ERR pending\n");
        return;
    }
#endif
    if (!valid_name(name) || strcmp(name, "main.lua") == 0 ||
            strcmp(name, "main.luac") == 0 || board_lfs_remove(name) < 0) {
        board_uart_puts("RM_ERR\n");
    } else {
        board_uart_puts("RM_OK\n");
    }
}

static void cmd_boot(const char *name)
{
    const char *dst;
    if (!valid_name(name) || strcmp(name, "main.lua") == 0 ||
            strcmp(name, "main.luac") == 0) {
        board_uart_puts("BOOT_ERR\n");
        return;
    }
    dst = has_suffix(name, ".luac") ? "main.luac" : "main.lua";
    if (board_lfs_copy(name, dst) < 0) {
        board_uart_puts("BOOT_ERR\n");
    } else {
        if (strcmp(dst, "main.lua") == 0) {
            (void)board_lfs_remove("main.luac");
        }
        board_uart_puts("BOOT_OK\n");
    }
}
#endif

static void upload_abort(const char *reason)
{
    board_lfs_write_abort();
    (void)board_lfs_remove(UPLOAD_TEMP);
    s_upload_mode = UPLOAD_NONE;
    s_upload_size = 0;
    board_uart_puts("SCRIPT_ERR ");
    board_uart_puts(reason);
    board_uart_puts("\n");
}

static int upload_write(const void *data, size_t len)
{
    int n = board_lfs_write_chunk(data, len);
    if (n != (int)len) {
        upload_abort("write");
        return 0;
    }
    s_upload_size += (uint32_t)len;
    return 1;
}

static void finish_upload(lua_State **pL)
{
    char num[12];
    int run_st;
    int should_run = strcmp(s_upload_name, "main.lua") == 0 ||
        strcmp(s_upload_name, "main.luac") == 0;
    if (board_lfs_write_close() < 0 ||
            board_lfs_replace(UPLOAD_TEMP, s_upload_name) < 0) {
        upload_abort("save");
        return;
    }
#ifndef LUA_BINARY_ONLY
    /* A successful source boot upload must not be shadowed by old bytecode. */
    if (strcmp(s_upload_name, "main.lua") == 0) {
        (void)board_lfs_remove("main.luac");
    }
#endif
    s_upload_mode = UPLOAD_NONE;
    board_status_or(ST_SCRIPT_EXT);
    u32dec(num, s_upload_size);
    board_uart_puts("SCRIPT_OK ");
    board_uart_puts(num);
    board_uart_puts("\n");
    if (!should_run) {
        return;
    }
#ifdef MSPM0_MODULAR_CORE
    if (module_update_has_pending()) {
        board_uart_puts("SCRIPT_DONE PENDING\n");
        return;
    }
#endif
    *pL = lua_recreate(*pL);
    if (!*pL) {
        board_uart_puts("SCRIPT_ERR OOM\n");
        return;
    }
    run_st = run_lua_file(*pL, s_upload_name);
    board_uart_puts(run_st == LUA_OK ? "SCRIPT_DONE OK\n" : "SCRIPT_DONE ERR\n");
    board_uart_rearm();
}

static int hex_nibble(char c)
{
    if (c >= '0' && c <= '9') return c - '0';
    if (c >= 'a' && c <= 'f') return c - 'a' + 10;
    if (c >= 'A' && c <= 'F') return c - 'A' + 10;
    return -1;
}

static void upload_hex_line(char *line, size_t len)
{
    uint8_t data[(RX_LINE_MAX - 1) / 2];
    size_t i;
    if ((len & 1u) != 0) {
        upload_abort("hex");
        return;
    }
    for (i = 0; i < len; i += 2) {
        int hi = hex_nibble(line[i]);
        int lo = hex_nibble(line[i + 1]);
        if (hi < 0 || lo < 0) {
            upload_abort("hex");
            return;
        }
        data[i / 2] = (uint8_t)((hi << 4) | lo);
    }
    if (upload_write(data, len / 2)) {
        board_uart_puts("HEX_OK\n");
    }
}

static void start_upload(const char *line, int mode)
{
    const char *p = line + 6;
    size_t i = 0;
    while (*p == ' ') p++;
    if (*p) {
        while (p[i] && i < sizeof(s_upload_name) - 1) {
            s_upload_name[i] = p[i];
            i++;
        }
        s_upload_name[i] = 0;
    } else {
        strcpy(s_upload_name, mode == UPLOAD_HEX ? "main.luac" : "main.lua");
    }
    if (!valid_name(s_upload_name) || !board_lfs_ready()) {
        board_uart_puts("SCRIPT_ERR name/fs\n");
        return;
    }
    (void)board_lfs_remove(UPLOAD_TEMP);
    if (board_lfs_write_open(UPLOAD_TEMP) < 0) {
        board_uart_puts("SCRIPT_ERR open\n");
        return;
    }
    s_upload_mode = mode;
    s_upload_size = 0;
    board_uart_puts("SCRIPT_BEGIN\n");
}

static void cmd_storageinfo(void)
{
    char num[12];
    board_uart_puts(board_lfs_ready() ? "FS_READY\n" : "FS_NOT_READY\n");
    board_uart_puts("STORAGE external_littlefs\n");
    board_uart_puts("PART W25Q32JVSSIQ\n");
    board_uart_puts("CAPACITY ");
    u32dec(num, board_lfs_capacity_bytes());
    board_uart_puts(num);
    board_uart_puts("\nPINS SPI1 PB16 PB15 PB14 PB17\nSTORAGE_END\n");
}

static void cmd_fileinfo(const char *name)
{
    char num[12];
    uint32_t size;
    uint32_t crc32;
    int status;
    if (!valid_name(name)) {
        board_uart_puts("FILE_ERR INVALID_NAME\n");
        return;
    }
    if (!board_lfs_ready()) {
        board_uart_puts("FILE_ERR FS_NOT_MOUNTED\n");
        return;
    }
    status = board_lfs_file_info(name, &size, &crc32);
    if (status != 0) {
        board_uart_puts(status == -2 ?
            "FILE_ERR NOT_FOUND\n" : "FILE_ERR IO\n");
        return;
    }
    board_uart_puts("FILE ");
    board_uart_puts(name);
    board_uart_puts(" ");
    u32dec(num, size);
    board_uart_puts(num);
    board_uart_puts(" ");
    u32hex(num, crc32, 8u);
    board_uart_puts(num);
    board_uart_puts("\nFILE_END\n");
}

#ifdef MSPM0_MODULAR_CORE
static void report_module_error(void)
{
    board_uart_puts("MOD_ERR ");
    board_uart_puts(module_update_error());
    board_uart_puts("\n");
}

static void report_module_blocked(void)
{
    board_uart_puts("MOD_BLOCKED ");
    board_uart_puts(module_update_error());
    board_uart_puts("\n");
}

static void cmd_modapply(lua_State **pL, const char *path)
{
    if (!valid_name(path) || module_update_stage(path) != 0) {
        report_module_error();
        return;
    }
    lua_destroy(*pL);
    *pL = NULL;
    board_uart_rearm();
    if (module_update_apply_pending() != 0) {
        report_module_error();
        report_module_blocked();
        return;
    }
    *pL = lua_recreate(NULL);
    if (!*pL) {
        board_uart_puts("MOD_ERR OOM\n");
        return;
    }
    board_status_or(ST_LUA_OK);
    board_uart_rearm();
    board_uart_puts("Idle\n");
}
#endif

static void handle_line(lua_State **pL, char *line)
{
    size_t n = strlen(line);
    while (n && (line[n - 1] == '\r' || line[n - 1] == '\n')) {
        line[--n] = 0;
    }

    if (s_upload_mode == UPLOAD_NONE) {
#ifndef LUA_SOURCE_FULL_TIGHT
        if (strncmp(line, "<<<LUA", 6) == 0 && (line[6] == 0 || line[6] == ' ')) {
            start_upload(line, UPLOAD_LUA);
        } else
#endif
        if (strncmp(line, "<<<HEX", 6) == 0 && (line[6] == 0 || line[6] == ' ')) {
            start_upload(line, UPLOAD_HEX);
        } else if (line[0] == '!' && line[1] == 0) {
            lua_bind_request_stop();
            board_uart_puts("STOP\n");
        } else if ((line[0] == 'r' || line[0] == 'R' ||
                    line[0] == 'f' || line[0] == 'F') && line[1] == 0) {
#ifdef MSPM0_MODULAR_CORE
            if (module_update_has_pending()) {
                board_uart_puts("MOD_BLOCKED pending\n");
                return;
            }
#endif
            *pL = lua_recreate(*pL);
            if (*pL) {
                (void)run_boot_script(*pL);
            }
        } else if (strcmp(line, "ls") == 0) {
            cmd_ls();
        } else if (strcmp(line, "storageinfo") == 0) {
            cmd_storageinfo();
        } else if (strncmp(line, "fileinfo ", 9) == 0) {
            cmd_fileinfo(line + 9);
        } else if (strcmp(line, "format") == 0) {
            cmd_format();
        } else if (strncmp(line, "baud ", 5) == 0) {
            cmd_baud(line + 5);
#ifdef MSPM0_MODULAR_CORE
        } else if (strcmp(line, "fwinfo") == 0) {
            cmd_fwinfo();
        } else if (strncmp(line, "modapply ", 9) == 0) {
            cmd_modapply(pL, line + 9);
        } else if (strcmp(line, "modstatus") == 0) {
            module_update_report_status();
#endif
        } else if (strcmp(line, "bsl") == 0) {
            /* Soft-enter ROM BSL for UART firmware update (same UART @ 9600). */
            board_uart_puts("BSL\n");
            board_delay_ms(20);
            DL_SYSCTL_resetDevice(DL_SYSCTL_RESET_BOOTLOADER_ENTRY);
            for (;;) {
                /* resetDevice does not return */
            }
#ifndef LUA_SOURCE_FULL_TIGHT
        } else if (strncmp(line, "get ", 4) == 0) {
            cmd_get(line + 4);
        } else if (strncmp(line, "rm ", 3) == 0) {
            cmd_rm(line + 3);
        } else if (strncmp(line, "boot ", 5) == 0) {
            cmd_boot(line + 5);
        } else if (line[0] == 'h' || strcmp(line, "help") == 0) {
            board_uart_puts("r/f/!/ls/storageinfo/fileinfo/format/get/rm/boot/baud/bsl/HEX"
#ifdef MSPM0_MODULAR_CORE
                "/fwinfo/modapply/modstatus"
#endif
                "\n");
#endif
        }
        return;
    }

    if ((s_upload_mode == UPLOAD_LUA && strcmp(line, ">>>LUA") == 0) ||
            (s_upload_mode == UPLOAD_HEX && strcmp(line, ">>>HEX") == 0)) {
        finish_upload(pL);
        return;
    }
    if (s_upload_mode == UPLOAD_HEX) {
        upload_hex_line(line, n);
    } else if (upload_write(line, n)) {
        (void)upload_write("\n", 1);
    }
}

static void poll_uart_cmd(lua_State **pL)
{
    for (;;) {
        int c = board_uart_getc_nonblock();
        if (c < 0) {
            break;
        }
        /* CR ignored; only LF ends a line (avoids double-fire / stray bytes). */
        if (c == '\r') {
            continue;
        }
        if (c == '\n') {
            if (s_line_overflow) {
                s_line_overflow = 0;
                s_line_n = 0;
                if (s_upload_mode != UPLOAD_NONE) {
                    upload_abort("line");
                }
            } else if (s_line_n != 0) {
                s_line[s_line_n] = 0;
                s_line_n = 0;
                handle_line(pL, s_line);
            }
        } else if (s_line_n + 1 < RX_LINE_MAX) {
            s_line[s_line_n++] = (char)c;
        } else {
            s_line_overflow = 1;
        }
    }
}

int main(void)
{
    lua_State *L = NULL;
#ifdef MSPM0_MODULAR_CORE
    int recovered_module_update = 0;
#endif
    board_status_set(ST_BOOT);
    SYSCFG_DL_init();
    board_pin_init();
    board_resource_init();
    board_irq_init();
    board_dma_init();
    board_uart_init();
    board_status_or(ST_UART_OK);
#if defined(BOARD_UART_SELFTEST) && BOARD_UART_SELFTEST
    if (board_uart_loopback_ok()) {
        board_status_or(ST_UART_LB_OK);
    } else {
        board_status_or(ST_UART_LB_FAIL);
    }
#endif
    if (board_clock_hfxt_ok()) {
        board_status_or(ST_HFXT_OK);
    } else {
        board_status_or(ST_HFXT_FAIL);
    }
    board_uart_puts("\nLua\n");
    board_uart_puts(board_clock_hfxt_ok() ? "80\n" : "32\n");
    led_blink(2);
    probe_spi_flash();
    if (board_lfs_init()) {
        board_status_or(ST_LFS_OK);
    }

#ifdef MSPM0_MODULAR_CORE
    if (module_update_has_pending()) {
        recovered_module_update = 1;
        board_uart_puts("MOD_RECOVER START\n");
        if (module_update_apply_pending() != 0) {
            report_module_error();
            report_module_blocked();
        }
    }
#endif
#ifdef MSPM0_MODULAR_CORE
    if (!module_update_has_pending())
#endif
        L = lua_recreate(NULL);
    if (L) {
        board_status_or(ST_LUA_OK);
        board_uart_puts("Run\n");
        board_status_or(ST_LUA_RUN);
#if !defined(BOARD_SKIP_BOOT_SCRIPT) || !(BOARD_SKIP_BOOT_SCRIPT)
#ifdef MSPM0_MODULAR_CORE
        if (!recovered_module_update)
#endif
            (void)run_boot_script(L);
#endif
        board_status_or(ST_DEMO_DONE);
#ifdef MSPM0_MODULAR_CORE
        if (recovered_module_update) board_uart_puts("MOD_RECOVERED\n");
#endif
    }
    /* Script may leave UART pinmux/IRQ odd; re-arm without wiping RX. */
    board_uart_rearm();
#ifdef MSPM0_MODULAR_CORE
    if (L) board_uart_puts("Idle\n");
    else board_uart_puts("MOD_RECOVERY_WAIT\n");
#else
    board_uart_puts("Idle\n");
#endif

    for (;;) {
        poll_uart_cmd(&L);
        if (!L
#ifdef MSPM0_MODULAR_CORE
                && !module_update_has_pending()
#endif
                ) {
            L = lua_recreate(NULL);
        }
        board_wdt_feed();
    }
}
