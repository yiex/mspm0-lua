#include "lua_runtime.h"

#include <string.h>

/*
 * MSPM0G3507 has 32 KiB SRAM. The full bytecode profile has about 5.6 KiB of
 * non-Lua static state (UART, LittleFS, OLED and peripheral state). Keeping
 * the Lua arena at 20 KiB leaves more than 5 KiB for the descending Cortex-M
 * stack. 24 KiB leaves roughly 1.5 KiB and caused stack corruption disguised
 * as random Lua OOMs or a frozen example.
 */
#ifdef MSPM0_MODULAR_CORE
#define LUA_HEAP_SIZE (23u * 1024u)
#else
#define LUA_HEAP_SIZE (20u * 1024u)
#endif

typedef struct heap_block {
    uint32_t size;
    uint32_t used;
} heap_block_t;

static uint8_t s_heap[LUA_HEAP_SIZE] __attribute__((aligned(8)));

static heap_block_t *heap_next(heap_block_t *b)
{
    uint8_t *p = (uint8_t *)(b + 1) + b->size;
    return p + sizeof(heap_block_t) <= s_heap + sizeof(s_heap)
        ? (heap_block_t *)p : NULL;
}

void lua_runtime_heap_init(void)
{
    heap_block_t *b = (heap_block_t *)s_heap;
    b->size = sizeof(s_heap) - sizeof(*b);
    b->used = 0;
}

static void heap_split(heap_block_t *b, size_t need)
{
    if (b->size >= need + sizeof(heap_block_t) + 8u) {
        heap_block_t *after = heap_next(b);
        heap_block_t *n = (heap_block_t *)((uint8_t *)(b + 1) + need);
        n->size = b->size - (uint32_t)need - sizeof(*n);
        n->used = 0;
        b->size = (uint32_t)need;
        if (after && !after->used) {
            n->size += sizeof(*after) + after->size;
        }
    }
}

static void heap_free(void *ptr)
{
    heap_block_t *b, *n, *p;
    if (!ptr) {
        return;
    }
    b = (heap_block_t *)ptr - 1;
    b->used = 0;
    n = heap_next(b);
    if (n && !n->used) {
        b->size += sizeof(*n) + n->size;
    }
    p = (heap_block_t *)s_heap;
    while ((n = heap_next(p)) != NULL && n != b) {
        p = n;
    }
    if (n == b && !p->used) {
        p->size += sizeof(*b) + b->size;
    }
}

void *lua_runtime_alloc(void *ud, void *ptr, size_t osize, size_t nsize)
{
    heap_block_t *b, *n;
    size_t need;
    (void)ud;
    (void)osize;
    if (nsize == 0) {
        heap_free(ptr);
        return NULL;
    }
    need = (nsize + 7u) & ~7u;
    if (!ptr) {
        for (b = (heap_block_t *)s_heap; b; b = heap_next(b)) {
            if (!b->used && b->size >= need) {
                heap_split(b, need);
                b->used = 1;
                return b + 1;
            }
        }
        return NULL;
    }
    b = (heap_block_t *)ptr - 1;
    if (b->size >= need) {
        heap_split(b, need);
        return ptr;
    }
    n = heap_next(b);
    if (n && !n->used && b->size + sizeof(*n) + n->size >= need) {
        b->size += sizeof(*n) + n->size;
        heap_split(b, need);
        return ptr;
    }
    n = lua_runtime_alloc(NULL, NULL, 0, nsize);
    if (n) {
        memcpy(n, ptr, b->size < nsize ? b->size : nsize);
        heap_free(ptr);
    }
    return n;
}

static uintptr_t current_sp(void)
{
    uintptr_t sp;
    __asm volatile ("mov %0, sp" : "=r"(sp));
    return sp;
}

void lua_runtime_mem(lua_runtime_mem_t *out)
{
    heap_block_t *b;
    uintptr_t floor;
    uintptr_t sp;
    if (!out) {
        return;
    }
    memset(out, 0, sizeof(*out));
    out->capacity = sizeof(s_heap) - sizeof(heap_block_t);
    for (b = (heap_block_t *)s_heap; b; b = heap_next(b)) {
        out->blocks++;
        if (b->used) {
            out->used += b->size;
        } else {
            out->free_blocks++;
            out->free += b->size;
            if (b->size > out->largest_free) {
                out->largest_free = b->size;
            }
        }
    }
    floor = (uintptr_t)(s_heap + sizeof(s_heap));
    sp = current_sp();
    if (sp > floor) {
        out->stack_free_now = (uint32_t)(sp - floor);
    }
}
