#ifndef LUA_RUNTIME_H
#define LUA_RUNTIME_H

#include <stddef.h>
#include <stdint.h>

typedef struct {
    uint32_t capacity;
    uint32_t used;
    uint32_t free;
    uint32_t largest_free;
    uint32_t blocks;
    uint32_t free_blocks;
    uint32_t stack_free_now;
} lua_runtime_mem_t;

void lua_runtime_heap_init(void);
void *lua_runtime_alloc(void *ud, void *ptr, size_t osize, size_t nsize);
void lua_runtime_mem(lua_runtime_mem_t *out);

#endif
