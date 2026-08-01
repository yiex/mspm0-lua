-- ATK-MS601/901 attitude demo over generic uart.* (no C imu module)
-- Wire: module TX -> PA24, module RX -> PA23, GND common. 115200 8N1
-- Frame: 55 55 ID LEN DATA SUM  (attitude ID=0x01 or legacy 0x53)
print("HI_UART_ATK_START")

local UART_ID = 2
local TX, RX, BAUD = "PA23", "PA24", 115200

uart.open(UART_ID, TX, RX, BAUD)

local q, qi, qn = {}, 1, 0
local frames = 0
local roll, pitch, yaw = 0, 0, 0

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
  -- bound queue (~256 B) so GC stays calm
  if qn - qi + 1 > 256 then
    local nq, j, k = {}, 1, qi
    while k <= qn and j <= 128 do
      nq[j] = q[k]
      j = j + 1
      k = k + 1
    end
    q, qn, qi = nq, j - 1, 1
  end
end

local function get(o) return q[qi + o - 1] end

local function drop(n)
  qi = qi + n
  if qi > 64 then
    local nq, j, k = {}, 1, qi
    while k <= qn do
      nq[j] = q[k]
      j = j + 1
      k = k + 1
    end
    q, qn, qi = nq, j - 1, 1
  end
end

local function avail() return qn - qi + 1 end

local function i16le(lo, hi)
  local v = lo + hi * 256
  if v >= 32768 then v = v - 65536 end
  return v
end

-- raw int16 * 1800 / 32768 => degrees * 10
local function deg_x10(raw)
  return (raw * 1800) // 32768
end

local function parse()
  while avail() >= 6 do
    if get(1) ~= 0x55 then
      drop(1)
    elseif get(2) ~= 0x55 and get(2) ~= 0xAF then
      drop(1)
    else
      local id = get(3)
      local len = get(4)
      if len > 28 then
        drop(1)
      elseif avail() < 5 + len then
        return
      else
        local sum = 0x55 + get(2) + id + len
        local i = 1
        while i <= len do
          sum = sum + get(4 + i)
          i = i + 1
        end
        sum = sum % 256
        if get(5 + len) == sum then
          if (id == 0x01 or id == 0x53) and len >= 6 then
            roll = deg_x10(i16le(get(5), get(6)))
            pitch = deg_x10(i16le(get(7), get(8)))
            yaw = deg_x10(i16le(get(9), get(10)))
            frames = frames + 1
          end
        end
        drop(5 + len)
      end
    end
  end
end

local t0 = millis()
local last_print = t0
while not stopped() and (millis() - t0) < 5000 do
  push(uart.rx(UART_ID, 5, 64))
  parse()
  if (millis() - last_print) >= 200 then
    last_print = millis()
    print("f", frames, "R", roll, "P", pitch, "Y", yaw)
  end
  yield()
end

uart.close(UART_ID)
print(frames > 0 and "HI_UART_ATK_OK" or "HI_UART_ATK_NOFRAME", frames)
