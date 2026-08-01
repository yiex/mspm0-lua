#!/usr/bin/env python3
"""Invoke the native Windows Lua 5.5.1/LUA_32BITS compiler."""

import os
import subprocess
import sys
import tempfile
from pathlib import Path

PROJECT = Path(__file__).resolve().parents[1]
COMPILER = Path(os.environ.get(
    "MSPM0_LUAC",
    PROJECT / "tools" / "bin" / "luac_mspm0.exe",
))

TIMER_EVENT_PRELUDE = r'''-- IDE-injected SysTick callback dispatcher.
do
  local start, take, native_stop = tmr.start, tmr.take, tmr.stop
  local callbacks, active = {}, {}
  local event_stop = false
  function tmr.every(ms, fn)
    local id
    for i = 0, 3 do if not active[i] then id = i break end end
    if id == nil then error("tmr:full") end
    if fn ~= nil and type(fn) ~= "function" then error("tmr:callback") end
    start(id, ms)
    active[id], callbacks[id] = true, fn
    return id
  end
  function tmr.stop(id)
    callbacks[id], active[id] = nil, nil
    return native_stop(id)
  end
  event = event or {}
  function event.stop() event_stop = true end
  function event.poll()
    local dispatched = 0
    for i = 0, 3 do
      local fn = callbacks[i]
      if fn then
        local hits = take(i)
        if hits > 0 then fn(i, hits); dispatched = dispatched + 1 end
      end
    end
    return dispatched
  end
  function event.run()
    event_stop = false
    while not event_stop do
      local dispatched = event.poll()
      if next(callbacks) == nil then break end
      if dispatched == 0 then yield() end
    end
  end
end
'''


def add_timer_event_runtime(source: str) -> str:
    return TIMER_EVENT_PRELUDE + source if (
        "tmr.every" in source
        or "event.run" in source
        or "event.poll" in source
        or "event.stop" in source
    ) else source


def main() -> None:
    if len(sys.argv) not in (2, 3):
        raise SystemExit("usage: compile_lua.py input.lua [output.luac]")
    src = Path(sys.argv[1]).resolve()
    dst = Path(sys.argv[2]).resolve() if len(sys.argv) == 3 else src.with_suffix(".luac")
    if not src.is_file():
        raise SystemExit(f"not found: {src}")
    if not COMPILER.is_file():
        raise SystemExit(
            "native compiler missing; build it with: python tools/build_luac.py"
        )
    dst.parent.mkdir(parents=True, exist_ok=True)
    prepared = add_timer_event_runtime(src.read_text(encoding="utf-8"))
    with tempfile.NamedTemporaryFile(
        mode="w", suffix=".lua", encoding="utf-8", delete=False
    ) as temp:
        temp.write(prepared)
        temp_path = Path(temp.name)
    try:
        proc = subprocess.run([str(COMPILER), str(temp_path), str(dst)])
        if proc.returncode != 0:
            raise SystemExit(proc.returncode)
    finally:
        temp_path.unlink(missing_ok=True)
    print(f"OK {src.stat().st_size} source bytes -> {dst.stat().st_size} bytecode bytes: {dst}")


if __name__ == "__main__":
    main()
