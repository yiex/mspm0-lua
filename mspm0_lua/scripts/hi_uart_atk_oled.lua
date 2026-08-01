-- ATK attitude on UART2 + OLED (Lua parse only; uses uart.* not imu.*)
-- Same wiring as hi_uart_atk.lua; OLED default PA15/PA16
print("HI_UART_ATK_OLED_START")

local UART_ID = 2
uart.open(UART_ID, "PA23", "PA24", 115200)

local oled_ok = pcall(function()
  oled.open()
  oled.clear()
  oled.cursor(0, 0)
  oled.print("ATK UART")
  oled.cursor(0, 2)
  oled.print("R:")
  oled.cursor(0, 4)
  oled.print("P:")
  oled.cursor(0, 6)
  oled.print("Y:")
end) and 1 or 0

local q, qi, qn = {}, 1, 0
local frames, roll, pitch, yaw = 0, 0, 0, 0

local function push(s)
  if not s then return end
  local i = 1
  while true do
    local b = byte(s, i)
    if not b then break end
    qn = qn + 1
    q[qn] = b
    i = i + 1
  end
end

local function get(o) return q[qi + o - 1] end
local function avail() return qn - qi + 1 end
local function drop(n)
  qi = qi + n
  if qi > 48 then
    local nq, j, k = {}, 1, qi
    while k <= qn do nq[j] = q[k]; j = j + 1; k = k + 1 end
    q, qn, qi = nq, j - 1, 1
  end
end

local function i16le(lo, hi)
  local v = lo + hi * 256
  if v >= 32768 then v = v - 65536 end
  return v
end

local function parse()
  while avail() >= 6 do
    if get(1) ~= 0x55 then drop(1)
    elseif get(2) ~= 0x55 and get(2) ~= 0xAF then drop(1)
    else
      local id, len = get(3), get(4)
      if len > 28 then drop(1)
      elseif avail() < 5 + len then return
      else
        local sum = 0x55 + get(2) + id + len
        for i = 1, len do sum = sum + get(4 + i) end
        if get(5 + len) == (sum % 256) then
          if (id == 0x01 or id == 0x53) and len >= 6 then
            roll = (i16le(get(5), get(6)) * 1800) // 32768
            pitch = (i16le(get(7), get(8)) * 1800) // 32768
            yaw = (i16le(get(9), get(10)) * 1800) // 32768
            frames = frames + 1
          end
        end
        drop(5 + len)
      end
    end
  end
end

local t0 = millis()
local last = t0
while not stopped() and (millis() - t0) < 4000 do
  push(uart.rx(UART_ID, 5, 64))
  parse()
  if oled_ok == 1 and (millis() - last) >= 50 then
    last = millis()
    pcall(function()
      oled.num(18, 2, roll, 1)
      oled.num(18, 4, pitch, 1)
      oled.num(18, 6, yaw, 1)
    end)
  end
  yield()
end

uart.close(UART_ID)
pcall(function() oled.close() end)
print("f", frames, "R", roll, "P", pitch, "Y", yaw, "o", oled_ok)
print(frames > 0 and "HI_UART_ATK_OLED_OK" or "HI_UART_ATK_OLED_NOFRAME")
