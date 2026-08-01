#include "native_module.h"

static int l_crc16(lua_State *L)
{
    size_t size;
    const uint8_t *data = (const uint8_t *)NATIVE_CORE_API->check_lstring(
        L, 1, &size);
    uint16_t crc = (uint16_t)NATIVE_CORE_API->opt_integer(L, 2, 0xFFFF);
    size_t i;
    unsigned bit;
    for (i = 0; i < size; i++) {
        crc ^= data[i];
        for (bit = 0; bit < 8u; bit++) {
            crc = (uint16_t)((crc >> 1) ^ ((crc & 1u) ? 0xA001u : 0u));
        }
    }
    NATIVE_CORE_API->push_integer(L, (int32_t)crc);
    return 1;
}

static int l_crc32(lua_State *L)
{
    size_t size;
    const uint8_t *data = (const uint8_t *)NATIVE_CORE_API->check_lstring(
        L, 1, &size);
    uint32_t crc = (uint32_t)NATIVE_CORE_API->opt_integer(L, 2, -1);
    size_t i;
    unsigned bit;
    for (i = 0; i < size; i++) {
        crc ^= data[i];
        for (bit = 0; bit < 8u; bit++) {
            crc = (crc >> 1) ^ ((crc & 1u) ? 0xEDB88320u : 0u);
        }
    }
    crc ^= 0xFFFFFFFFu;
    NATIVE_CORE_API->push_integer(L, (int32_t)crc);
    return 1;
}

static const native_lua_reg_t k_crc_functions[] = {
    {"crc16", l_crc16}, {"crc32", l_crc32}, {0, 0},
};

static int crc_init(lua_State *L, const native_core_api_t *api)
{
    if (!api || api->magic != NATIVE_CORE_API_MAGIC ||
            api->abi_version != NATIVE_CORE_ABI_VERSION ||
            api->struct_size < sizeof(native_core_api_t)) return -1;
    return api->register_lua_module(L, "crc", k_crc_functions);
}

const native_module_header_t g_native_module_header
    __attribute__((section(".module_header"), used, aligned(4))) = {
        NATIVE_MODULE_MAGIC, NATIVE_MODULE_FORMAT, NATIVE_CORE_ABI_VERSION,
        0, 0, sizeof(native_module_header_t), crc_init, 0, "crc",
    };
