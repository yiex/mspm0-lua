-- IMU + OLED main (modular)
-- 1) 工程 → 保存模块：把 oled_draw.lua 存为 oled_draw.luac
-- 2) 运行 main：加载模块并显示陀螺仪 R/P/Y
-- 接线: OLED I2C1 PA15/PA16 0x3C; IMU UART2 PA23/PA24 115200; CH340 PA10/PA11

print("IMU_OLED_START")

-- load OLED module from flash
local ok_mod = pcall(function()
  runfile("oled_draw.luac")
end)
if not ok_mod then
  print("NEED_MODULE oled_draw.luac")
  print("Use: open oled_draw.lua -> 保存模块")
  return
end

do
  local ok = pcall(oled_boot_ui)
  if ok then print("OLED_OK") else print("OLED_FAIL"); oled_ok = 0 end
end

local roll, pitch, yaw, frames = 0, 0, 0, 0
local N = 128
local q = {}
for i = 1, N do q[i] = 0 end
local head, tail = 1, 1

local function qput(b)
  local nh = head + 1
  if nh > N then nh = 1 end
  if nh == tail then tail = tail + 1; if tail > N then tail = 1 end end
  q[head] = b; head = nh
end
local function qget()
  if tail == head then return nil end
  local b = q[tail]; tail = tail + 1; if tail > N then tail = 1 end; return b
end
local function qlen()
  local n = head - tail; if n < 0 then n = n + N end; return n
end
local function qpk(i)
  local p = tail + i - 1; if p > N then p = p - N end; return q[p]
end
local function push(s)
  if not s then return end
  local i = 1
  while true do
    local b = byte(s, i)
    if not b then break end
    qput(b); i = i + 1
  end
end
local function i16(lo, hi)
  local v = lo + hi * 256
  if v >= 32768 then v = v - 65536 end
  return v
end
local function parse()
  local steps = 0
  while qlen() >= 6 and steps < 32 do
    steps = steps + 1
    if qpk(1) ~= 0x55 then qget()
    else
      local h = qpk(2)
      if h ~= 0x55 and h ~= 0xAF then qget()
      else
        local id, len = qpk(3), qpk(4)
        if len > 28 then qget()
        elseif qlen() < 5 + len then break
        else
          local sum = 0x55 + h + id + len
          local i = 1
          while i <= len do sum = sum + qpk(4 + i); i = i + 1 end
          if qpk(5 + len) == sum % 256 and (id == 1 or id == 0x53) and len >= 6 then
            roll = (i16(qpk(5), qpk(6)) * 1800) // 32768
            pitch = (i16(qpk(7), qpk(8)) * 1800) // 32768
            yaw = (i16(qpk(9), qpk(10)) * 1800) // 32768
            frames = frames + 1
          end
          for _ = 1, 5 + len do qget() end
        end
      end
    end
  end
end

uart.open(2, "PA23", "PA24", 115200)
print("UART2_OK")

local tid = tmr.every(20)
local last_print = millis()
local axis = 0
local dr, dp, dy = 1, 1, 1
local got_imu = 0

while not stopped() do
  local r = 0
  local c = uart.rx(2, 0, 64)
  while c and r < 12 do
    local pr, pp, py, pf = roll, pitch, yaw, frames
    push(c); parse(); r = r + 1
    if frames ~= pf then
      got_imu = 1
      if roll ~= pr then dr = 1 end
      if pitch ~= pp then dp = 1 end
      if yaw ~= py then dy = 1 end
    end
    c = uart.rx(2, 0, 64)
  end

  if tmr.ready(tid) and got_imu == 1 then
    if axis == 0 and dr == 1 then
      if oled_paint_axis(0, roll) then dr = 0 end
    elseif axis == 1 and dp == 1 then
      if oled_paint_axis(1, pitch) then dp = 0 end
    elseif axis == 2 and dy == 1 then
      if oled_paint_axis(2, yaw) then dy = 0 end
    end
    axis = axis + 1
    if axis > 2 then axis = 0 end
  end

  if millis() - last_print >= 1000 then
    local function f1(v)
      local n, s = v, ""
      if n < 0 then s = "-"; n = -n end
      return s .. (n // 10) .. "." .. (n % 10)
    end
    print("R=" .. f1(roll) .. " P=" .. f1(pitch) .. " Y=" .. f1(yaw)
      .. " f=" .. frames .. " oe=" .. oe .. " sk=" .. skips)
    last_print = millis()
  end
  yield()
end

tmr.stop(tid)
uart.close(2)
pcall(function() i2c.close(1) end)
print("STOP", frames, skips)
