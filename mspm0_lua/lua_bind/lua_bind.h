#ifndef LUA_BIND_H
#define LUA_BIND_H

#include "lua.h"

void lua_bind_register(lua_State *L);
/* Stop VM-owned hardware and release callbacks/resources before lua_close(). */
void lua_bind_reset_runtime(lua_State *L);

/* Stop state used by the host '!' command and the VM instruction hook. */
void lua_bind_request_stop(void);
void lua_bind_clear_stop(void);
int lua_bind_stop_requested(void);

#endif
