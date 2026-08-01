local M = {}

local bus_id
local scl_pin
local sda_pin
local address
local bus_hz
local opened = false
local font_data = require("_oled_font")

local function write(payload)
  if not opened then error("oled:not_open") end
  if not i2c.write_on(bus_id, scl_pin, sda_pin, address, payload, bus_hz) then
    error("oled:i2c_write")
  end
end

local function command(payload)
  write("\x00" .. payload)
end

local function selected_font(size)
  local selected = font_data[size]
  if not selected then error("oled:font_size:" .. size) end
  return selected
end

local function render_codes(x, y, codes, count, size)
  if type(x) ~= "number" or type(y) ~= "number" or x < 0 or y < 0 or
      x > 127 or y > 63 or y % 8 ~= 0 then error("oled:position") end
  local font = selected_font(size)
  if x + count * font.width > 128 or y + font.pages * 8 > 64 then
    error("oled:text_bounds")
  end
  for page_offset = 0, font.pages - 1 do
    local row = ""
    for index = 1, count do
      local glyph = font.glyphs[page_offset + 1][codes[index]]
      if not glyph then error("oled:glyph:" .. codes[index] .. ":" .. size) end
      row = row .. glyph
    end
    local page = y // 8 + page_offset
    command(i2c.bytes(0xb0 + page, x % 16, 0x10 + x // 16))
    write("\x40" .. row)
  end
end

function M.open(id, scl, sda, addr, hz)
  if id ~= 0 and id ~= 1 then error("oled:i2c_id") end
  if type(scl) ~= "string" or type(sda) ~= "string" then error("oled:pins") end
  if type(addr) ~= "number" or addr < 0x08 or addr > 0x77 then error("oled:address") end
  if type(hz) ~= "number" or hz < 10000 or hz > 1000000 then error("oled:hz") end
  if not i2c.valid(id, scl, sda) then error("oled:i2c_route") end
  bus_id, scl_pin, sda_pin, address, bus_hz = id, scl, sda, addr, hz
  opened = true
  command("\xae\xd5\x80\xa8\x3f\xd3\x00\x40\x8d\x14\x20\x02" ..
    "\xa1\xc8\xda\x12\x81\xcf\xd9\xf1\xdb\x40\xa4\xa6\xaf")
end

function M.close()
  if opened then command("\xae") end
  opened = false
end

function M.fill(value)
  if type(value) ~= "number" or value < 0 or value > 255 then error("oled:fill") end
  local unit = i2c.bytes(value)
  local block = unit .. unit
  block = block .. block
  block = block .. block
  block = block .. block
  block = block .. block
  block = block .. block
  block = block .. block
  for page = 0, 7 do
    command(i2c.bytes(0xb0 + page, 0, 0x10))
    write("\x40" .. block)
  end
end

function M.clear()
  M.fill(0)
end

function M.text(x, y, value, size)
  if type(value) ~= "string" then error("oled:text_type") end
  local font = selected_font(size)
  local codes = font.texts[value]
  if not codes then error("oled:text_not_rasterized") end
  render_codes(x, y, codes, #codes, size)
end

local function append_unsigned(codes, count, value)
  local divisor = 1
  while value // divisor >= 10 do divisor = divisor * 10 end
  while divisor > 0 do
    count = count + 1
    codes[count] = 48 + (value // divisor) % 10
    divisor = divisor // 10
  end
  return count
end

function M.number(x, y, value, decimals, size)
  if type(value) ~= "number" or type(decimals) ~= "number" or
      decimals < 0 or decimals > 3 then error("oled:number") end
  local codes = {}
  local count = 0
  if value < 0 then
    count = 1
    codes[count] = 45
    value = -value
  end
  local scale = 1
  for _ = 1, decimals do scale = scale * 10 end
  count = append_unsigned(codes, count, value // scale)
  if decimals > 0 then
    count = count + 1
    codes[count] = 46
    local fraction = value % scale
    local divisor = scale // 10
    while divisor > 0 do
      count = count + 1
      codes[count] = 48 + (fraction // divisor) % 10
      divisor = divisor // 10
    end
  end
  render_codes(x, y, codes, count, size)
end

return M
