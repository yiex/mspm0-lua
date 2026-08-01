/*
 * In-process Lua 5.5.1 / LUA_32BITS compiler for MSPM0 bytecode ABI.
 *
 * Copyright (C) 1994-2026 Lua.org, PUC-Rio (Lua core).
 * SPDX-License-Identifier: MIT
 */
#include "mspm0_luac_api.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "lua.h"
#include "lauxlib.h"

#ifndef MSPM0_LUAC_SEED
#define MSPM0_LUAC_SEED 0x12345678u
#endif

typedef struct {
    unsigned char *data;
    size_t size;
    size_t cap;
} DumpBuf;

static void *host_alloc(void *ud, void *ptr, size_t osize, size_t nsize)
{
    (void)ud;
    (void)osize;
    if (nsize == 0) {
        free(ptr);
        return NULL;
    }
    return realloc(ptr, nsize);
}

static int dump_writer(lua_State *L, const void *p, size_t sz, void *ud)
{
    DumpBuf *b = (DumpBuf *)ud;
    (void)L;
    if (sz == 0) {
        return 0;
    }
    if (b->size + sz > b->cap) {
        size_t ncap = b->cap ? b->cap : 256;
        while (ncap < b->size + sz) {
            ncap *= 2;
        }
        {
            unsigned char *nd = (unsigned char *)realloc(b->data, ncap);
            if (!nd) {
                return 1;
            }
            b->data = nd;
            b->cap = ncap;
        }
    }
    memcpy(b->data + b->size, p, sz);
    b->size += sz;
    return 0;
}

static void set_err(char *errbuf, size_t errbuf_len, const char *msg)
{
    if (!errbuf || errbuf_len == 0) {
        return;
    }
    if (!msg) {
        msg = "compile failed";
    }
    strncpy(errbuf, msg, errbuf_len - 1);
    errbuf[errbuf_len - 1] = 0;
}

int mspm0_luac_compile(
    const char *source,
    size_t source_len,
    unsigned char **out,
    size_t *out_len,
    char *errbuf,
    size_t errbuf_len)
{
    lua_State *L;
    DumpBuf buf;
    int st;

    if (out) {
        *out = NULL;
    }
    if (out_len) {
        *out_len = 0;
    }
    if (!source || !out || !out_len) {
        set_err(errbuf, errbuf_len, "invalid arguments");
        return 2;
    }
    if (source_len == 0) {
        set_err(errbuf, errbuf_len, "empty source");
        return 2;
    }

    L = lua_newstate(host_alloc, NULL, MSPM0_LUAC_SEED);
    if (!L) {
        set_err(errbuf, errbuf_len, "cannot create Lua state");
        return 1;
    }

    st = luaL_loadbufferx(L, source, source_len, "=editor", "t");
    if (st != LUA_OK) {
        set_err(errbuf, errbuf_len, lua_tostring(L, -1));
        lua_close(L);
        return 1;
    }

    buf.data = NULL;
    buf.size = 0;
    buf.cap = 0;
    /* strip = 1 matches tools/luac_mspm0.c */
    st = lua_dump(L, dump_writer, &buf, 1);
    lua_close(L);
    if (st != 0 || !buf.data || buf.size == 0) {
        free(buf.data);
        set_err(errbuf, errbuf_len, "failed to write bytecode");
        return 1;
    }

    *out = buf.data;
    *out_len = buf.size;
    return 0;
}

void mspm0_luac_free(void *p)
{
    free(p);
}
