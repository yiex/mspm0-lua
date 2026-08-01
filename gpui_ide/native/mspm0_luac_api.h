/*
 * In-process Lua 5.5.1 / LUA_32BITS compiler for MSPM0 bytecode ABI.
 *
 * Copyright (C) 1994-2026 Lua.org, PUC-Rio (Lua core).
 * SPDX-License-Identifier: MIT
 */
#ifndef MSPM0_LUAC_API_H
#define MSPM0_LUAC_API_H

#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Compile Lua source text to stripped bytecode.
 * On success: returns 0, *out points to malloc'd buffer of *out_len bytes.
 * On failure: returns non-zero, errbuf filled (if non-NULL).
 * Caller must free *out with mspm0_luac_free. */
int mspm0_luac_compile(
    const char *source,
    size_t source_len,
    unsigned char **out,
    size_t *out_len,
    char *errbuf,
    size_t errbuf_len);

void mspm0_luac_free(void *p);

#ifdef __cplusplus
}
#endif

#endif
