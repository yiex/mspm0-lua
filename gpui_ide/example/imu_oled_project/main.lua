-- IMU + OLED main (modular)
-- IDE automatically compiles oled_draw.lua before main.lua.
-- 接线: OLED I2C1 PA15/PA16 0x3C; IMU UART2 PA23/PA24 115200; CH340 PA10/PA11

print("IMU_OLED_START")

local oled = require("oled_draw")

do
  local ok = pcall(oled.open)
  if ok then print("OLED_OK") else print("OLED_FAIL") end
end
collectgarbage("collect")

local roll, pitch, yaw, frames = 0, 0, 0, 0
local function i16(lo, hi)
  local v = lo + hi * 256
  if v >= 32768 then v = v - 65536 end
  return v
end
local function f1(v)
  local n, s = v, ""
  if n < 0 then s = "-"; n = -n end
  return s .. (n // 10) .. "." .. (n % 10)
end

-- Streaming parser with constant memory. It accepts fragmented frames and
-- discards oversized payloads without allocating a per-byte Lua table.
local state, frame_id, frame_len, pos, sum = 0, 0, 0, 0, 0
local p1, p2, p3, p4, p5, p6 = 0, 0, 0, 0, 0, 0
local function feed(b)
  if state == 0 then
    if b == 0x55 then state = 1 end
  elseif state == 1 then
    if b == 0x55 or b == 0xAF then
      sum = 0x55 + b
      state = 2
    elseif b ~= 0x55 then
      state = 0
    end
  elseif state == 2 then
    frame_id = b
    sum = sum + b
    state = 3
  elseif state == 3 then
    frame_len, pos = b, 0
    sum = sum + b
    if frame_len > 28 then
      state = 0
    elseif frame_len == 0 then
      state = 5
    else
      state = 4
    end
  elseif state == 4 then
    pos = pos + 1
    sum = sum + b
    if pos == 1 then p1 = b elseif pos == 2 then p2 = b
    elseif pos == 3 then p3 = b elseif pos == 4 then p4 = b
    elseif pos == 5 then p5 = b elseif pos == 6 then p6 = b end
    if pos >= frame_len then state = 5 end
  else
    if b == sum % 256 and frame_len >= 6 and
        (frame_id == 1 or frame_id == 0x53) then
      roll = (i16(p1, p2) * 1800) // 32768
      pitch = (i16(p3, p4) * 1800) // 32768
      yaw = (i16(p5, p6) * 1800) // 32768
      frames = frames + 1
    end
    state = b == 0x55 and 1 or 0
  end
end
local function push(s)
  if not s then return end
  local i = 1
  while true do
    local b = byte(s, i)
    if not b then return end
    feed(b)
    i = i + 1
  end
end

uart.open(2, "PA23", "PA24", 115200)
print("UART2_OK")

local last_print = millis()
local started = last_print
local last_paint = started
local axis = 0
local dr, dp, dy = 1, 1, 1
local got_imu = 0
local gc_tick = 0

while not stopped() and millis() - started < 5000 do
  local r = 0
  local c = uart.rx(2, 0, 64)
  while c and r < 4 do
    local pr, pp, py, pf = roll, pitch, yaw, frames
    push(c); r = r + 1
    if frames ~= pf then
      got_imu = 1
      if roll ~= pr then dr = 1 end
      if pitch ~= pp then dp = 1 end
      if yaw ~= py then dy = 1 end
    end
    c = uart.rx(2, 0, 64)
  end

  if got_imu == 1 and millis() - last_paint >= 20 then
    last_paint = millis()
    if axis == 0 and dr == 1 then
      if pcall(oled.paint, 2, roll) then dr = 0 end
    elseif axis == 1 and dp == 1 then
      if pcall(oled.paint, 4, pitch) then dp = 0 end
    elseif axis == 2 and dy == 1 then
      if pcall(oled.paint, 6, yaw) then dy = 0 end
    end
    axis = axis + 1
    if axis > 2 then axis = 0 end
  end

  if millis() - last_print >= 1000 then
    print("R=" .. f1(roll) .. " P=" .. f1(pitch) .. " Y=" .. f1(yaw)
      .. " f=" .. frames)
    last_print = millis()
  end
  gc_tick = gc_tick + 1
  -- uart.rx() and oled.number() create short-lived Lua values under a
  -- continuous attitude stream. Advance GC before those allocations build up.
  if r > 0 or gc_tick % 8 == 0 then collectgarbage("step", 256) end
  yield()
end

uart.close(2)
print("STOP", frames)
