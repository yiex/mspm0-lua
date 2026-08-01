#include "native_module.h"

static int l_plug_ping(lua_State *L)
{
    NATIVE_CORE_API->push_integer(L, 3507);
    return 1;
}

static const native_lua_reg_t k_plug_functions[] = {
    {"ping", l_plug_ping},
    {0, 0},
};

static int plug_init(lua_State *L, const native_core_api_t *api)
{
    if (!api || api->magic != NATIVE_CORE_API_MAGIC ||
            api->abi_version != NATIVE_CORE_ABI_VERSION ||
            api->struct_size < sizeof(native_core_api_t)) {
        return -1;
    }
    return api->register_lua_module(L, "plug", k_plug_functions);
}

const native_module_header_t g_native_module_header
    __attribute__((section(".module_header"), used, aligned(4))) = {
        NATIVE_MODULE_MAGIC,
        NATIVE_MODULE_FORMAT,
        NATIVE_CORE_ABI_VERSION,
        0,
        0,
        sizeof(native_module_header_t),
        plug_init,
        0,
        "plug",
    };
