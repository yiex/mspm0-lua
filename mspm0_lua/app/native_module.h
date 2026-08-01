#ifndef NATIVE_MODULE_H
#define NATIVE_MODULE_H

#include <stddef.h>
#include <stdint.h>

typedef struct lua_State lua_State;

#ifdef MSPM0_MODULAR_CORE
#define NATIVE_CORE_API_ADDR      0x00017F00u
#define NATIVE_MODULE_SLOT_ADDR   0x00018000u
#define NATIVE_MODULE_SLOT_SIZE   0x00001000u
#define NATIVE_MODULE_SLOT_COUNT  8u
#define NATIVE_CORE_ABI_VERSION   7u
#else
#define NATIVE_CORE_API_ADDR      0x0001F700u
#define NATIVE_MODULE_SLOT_ADDR   0x0001F800u
#define NATIVE_MODULE_SLOT_SIZE   0x00000800u
#define NATIVE_MODULE_SLOT_COUNT  1u
#define NATIVE_CORE_ABI_VERSION   1u
#endif
#define NATIVE_CORE_API_MAGIC     0x49504143u /* "CAPI" */
#define NATIVE_MODULE_MAGIC       0x444F4D4Cu /* "LMOD" */
#define NATIVE_MODULE_FORMAT      2u
#define NATIVE_MODULE_HEADER_SIZE 32u
#define NATIVE_MODULE_STATE_SIZE  32u

typedef int (*native_lua_cfunction_t)(lua_State *L);

typedef struct {
    const char *name;
    native_lua_cfunction_t function;
} native_lua_reg_t;

typedef struct {
    uintptr_t port;
    uint32_t pin;
    uint32_t iomux;
} native_pin_t;

typedef struct {
    uint32_t magic;
    uint16_t abi_version;
    uint16_t struct_size;
    int (*register_lua_module)(lua_State *L, const char *name,
        const native_lua_reg_t *functions);
    void (*push_integer)(lua_State *L, int32_t value);
    void (*uart_puts)(const char *text);
#ifdef MSPM0_MODULAR_CORE
    int32_t (*check_integer)(lua_State *L, int index);
    int32_t (*opt_integer)(lua_State *L, int index, int32_t fallback);
    const char *(*check_string)(lua_State *L, int index);
    const char *(*opt_string)(lua_State *L, int index, const char *fallback);
    void (*push_boolean)(lua_State *L, int value);
    int (*raise_error)(lua_State *L, const char *message);
    int (*pin_resolve)(const char *name, native_pin_t *pin);
    int (*pin_claim)(const char *name, uint8_t owner);
    void (*pin_release)(const char *name, uint8_t owner);
    uint32_t (*millis)(void);
    int (*timer_start)(unsigned id, uint32_t period_ms);
    void (*timer_stop)(unsigned id);
    uint32_t (*timer_take)(unsigned id);
    void (*delay_ms)(uint32_t ms);
    const char *(*check_lstring)(lua_State *L, int index, size_t *length);
    void (*push_lstring)(lua_State *L, const char *data, size_t length);
    uint32_t (*bus_clock_hz)(void);
    int (*pin_af)(const char *name, unsigned function, int input_enable);
    int (*resource_claim)(uint8_t resource, uint8_t owner);
    void (*resource_release)(uint8_t resource, uint8_t owner);
    int (*spi1_acquire)(uint32_t timeout_ms);
    void (*spi1_release)(void);
    int (*pin_owner)(const char *name);
    unsigned (*pin_policy)(const char *name);
    void *(*module_state)(unsigned slot, size_t size);
    int (*uart0_acquire)(void);
    void (*uart0_release)(void);
    int (*to_boolean)(lua_State *L, int index);
    uint32_t (*udiv32)(uint32_t numerator, uint32_t denominator);
#endif
} native_core_api_t;

typedef int (*native_module_init_t)(lua_State *L,
    const native_core_api_t *api);
typedef void (*native_module_deinit_t)(void);

typedef struct {
    uint32_t magic;
    uint16_t format_version;
    uint16_t abi_version;
    uint32_t image_size;
    uint16_t payload_crc16;
    uint16_t header_size;
    native_module_init_t init;
    native_module_deinit_t deinit;
    char name[8];
} native_module_header_t;

#define NATIVE_CORE_API \
    ((const native_core_api_t *)(uintptr_t)NATIVE_CORE_API_ADDR)

int native_module_register(lua_State *L);
void native_module_deinit(void);

#endif
