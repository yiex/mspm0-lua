-- ATK attitude on UART2 + OLED. Current firmware has no imu.* module.
print('HI_UART_ATK_OLED_START')

local UART_ID = 2
uart.open(UART_ID, 'PA23', 'PA24', 115200)

local oled
local oled_ok = 0
local probe = type(i2c) == "table" and i2c.probe_on
if probe and probe(1, "PA15", "PA16", 0x3c, 100000) then
  local loaded, module = pcall(require, "oled")
  if loaded then
    oled = module
    oled_ok = pcall(function()
      oled.open(1, "PA15", "PA16", 0x3c, 100000)
      oled.clear()
      oled.text(0, 0, "姿态", 16)
      oled.number(0, 16, 0, 1, 8)
      oled.number(0, 32, 0, 1, 8)
      oled.number(0, 48, 0, 1, 8)
    end) and 1 or 0
  end
end

local frames, roll, pitch, yaw = 0, 0, 0, 0
local function i16le(lo, hi)
  local value = lo + hi * 256
  if value >= 32768 then value = value - 65536 end
  return value
end

-- Streaming state machine: no byte table and no queue copies. This keeps the
-- example bounded even when UART2 is continuously sending attitude frames.
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
    frame_id, sum, state = b, sum + b, 3
  elseif state == 3 then
    frame_len, pos, sum = b, 0, sum + b
    state = frame_len > 28 and 0 or (frame_len == 0 and 5 or 4)
  elseif state == 4 then
    pos, sum = pos + 1, sum + b
    if pos == 1 then p1 = b elseif pos == 2 then p2 = b
    elseif pos == 3 then p3 = b elseif pos == 4 then p4 = b
    elseif pos == 5 then p5 = b elseif pos == 6 then p6 = b end
    if pos >= frame_len then state = 5 end
  else
    if b == sum % 256 and frame_len >= 6 and
        (frame_id == 0x01 or frame_id == 0x53) then
      roll = (i16le(p1, p2) * 1800) // 32768
      pitch = (i16le(p3, p4) * 1800) // 32768
      yaw = (i16le(p5, p6) * 1800) // 32768
      frames = frames + 1
    end
    state = b == 0x55 and 1 or 0
  end
end

local function push(s)
  if not s then return end
  for i = 1, #s do feed(byte(s, i)) end
end

local started = millis()
local last_oled = started
while not stopped() and millis() - started < 4000 do
  push(uart.rx(UART_ID, 5, 64))
  if oled_ok == 1 and millis() - last_oled >= 50 then
    last_oled = millis()
    pcall(function()
      oled.number(0, 16, roll, 1, 8)
      oled.number(0, 32, pitch, 1, 8)
      oled.number(0, 48, yaw, 1, 8)
    end)
  end
  yield()
end

uart.close(UART_ID)
print('frames', frames, 'R', roll, 'P', pitch, 'Y', yaw)
print(frames > 0 and 'HI_UART_ATK_OLED_OK' or 'HI_UART_ATK_OLED_NOFRAME')
