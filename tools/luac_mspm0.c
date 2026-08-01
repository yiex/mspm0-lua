/*
 * Host-side Lua 5.5.1 compiler for the MSPM0 firmware ABI.
 *
 * Copyright (C) 1994-2026 Lua.org, PUC-Rio (Lua core).
 * SPDX-License-Identifier: MIT
 */
#include <stdio.h>
#include <stdlib.h>

#include "lua.h"
#include "lauxlib.h"

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

static int dump_writer(lua_State *L, const void *data, size_t size, void *ud)
{
    FILE *out = (FILE *)ud;
    (void)L;
    return fwrite(data, 1, size, out) == size ? 0 : 1;
}

int main(int argc, char **argv)
{
    lua_State *L;
    FILE *out;
    int st;
    if (argc != 3) {
        fprintf(stderr, "usage: luac_mspm0 input.lua output.luac\n");
        return 2;
    }
    L = lua_newstate(host_alloc, NULL, 0x12345678u);
    if (!L) {
        fprintf(stderr, "cannot create Lua state\n");
        return 1;
    }
    st = luaL_loadfilex(L, argv[1], "t");
    if (st != LUA_OK) {
        fprintf(stderr, "%s\n", lua_tostring(L, -1));
        lua_close(L);
        return 1;
    }
    out = fopen(argv[2], "wb");
    if (!out) {
        perror(argv[2]);
        lua_close(L);
        return 1;
    }
    st = lua_dump(L, dump_writer, out, 1);
    if (fclose(out) != 0) {
        st = 1;
    }
    lua_close(L);
    if (st != 0) {
        fprintf(stderr, "failed to write bytecode\n");
        return 1;
    }
    return 0;
}
