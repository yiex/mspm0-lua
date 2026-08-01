-- SSD1306 page mode, minimal footprint
local M = { bus = 1, address = 0x3c, opened = false, fails = 0 }

local function send(p)
  if not M.opened then return false end
  if i2c.write(M.bus, M.address, p) then
    M.fails = 0
    return true
  end
  M.fails = M.fails + 1
  return false
end

function M.cmd(...)
  local n = select("#", ...)
  local i = 1
  while i <= n do
    if not send(bytes(0x00, select(i, ...))) then return false end
    i = i + 1
  end
  return true
end

function M.blob(g)
  local n = #g
  local o = 1
  while o <= n do
    local L = n - o + 1
    if L > 7 then L = 7 end
    local ok = false
    if L == 1 then ok = send(bytes(0x40, byte(g, o)))
    elseif L == 2 then ok = send(bytes(0x40, byte(g, o), byte(g, o + 1)))
    elseif L == 3 then ok = send(bytes(0x40, byte(g, o), byte(g, o + 1), byte(g, o + 2)))
    elseif L == 4 then ok = send(bytes(0x40, byte(g, o), byte(g, o + 1), byte(g, o + 2), byte(g, o + 3)))
    elseif L == 5 then ok = send(bytes(0x40, byte(g, o), byte(g, o + 1), byte(g, o + 2), byte(g, o + 3), byte(g, o + 4)))
    elseif L == 6 then ok = send(bytes(0x40, byte(g, o), byte(g, o + 1), byte(g, o + 2), byte(g, o + 3), byte(g, o + 4), byte(g, o + 5)))
    else ok = send(bytes(0x40, byte(g, o), byte(g, o + 1), byte(g, o + 2), byte(g, o + 3), byte(g, o + 4), byte(g, o + 5), byte(g, o + 6)))
    end
    if not ok then return false end
    o = o + L
  end
  return true
end

function M.init(bus, scl, sda, addr, hz)
  M.bus = bus or 1
  M.address = addr or 0x3c
  M.scl = scl or "PA15"
  M.sda = sda or "PA16"
  M.hz = hz or 100000
  i2c.open(M.bus, M.scl, M.sda, M.hz)
  M.opened = true
  M.fails = 0
  if not M.cmd(
    0xae, 0xd5, 0x80, 0xa8, 0x3f, 0xd3, 0x00, 0x40,
    0x8d, 0x14, 0x20, 0x02, 0xa1, 0xc8, 0xda, 0x12,
    0x81, 0xcf, 0xd9, 0xf1, 0xdb, 0x40, 0xa4, 0xa6, 0xaf
  ) then error("oled") end
end

function M.clear()
  local p = 0
  while p < 8 do
    if not M.cmd(0xb0 + p, 0x00, 0x10) then return false end
    local c = 0
    while c < 128 do
      if not send(bytes(0x40, 0, 0, 0, 0, 0, 0, 0)) then return false end
      c = c + 7
    end
    if not send(bytes(0x40, 0, 0)) then return false end
    p = p + 1
  end
  return true
end

function M.cur(x, page)
  return M.cmd(0xb0 + page, x % 16, 0x10 + x // 16)
end

function M.puts6(x, page, s, glyph)
  if not M.cur(x, page) then return false end
  local i = 1
  while true do
    local b = byte(s, i)
    if not b then break end
    if b >= 97 and b <= 122 then b = b - 32 end
    if not M.blob(glyph(b)) then return false end
    i = i + 1
    if x + (i - 1) * 6 > 122 then break end
  end
  return true
end

function M.reopen()
  pcall(function() i2c.close(M.bus) end)
  M.opened = false
  if not pcall(function() i2c.open(M.bus, M.scl, M.sda, M.hz) end) then return false end
  M.opened = true
  M.fails = 0
  return M.cmd(
    0xae, 0xd5, 0x80, 0xa8, 0x3f, 0xd3, 0x00, 0x40,
    0x8d, 0x14, 0x20, 0x02, 0xa1, 0xc8, 0xda, 0x12,
    0x81, 0xcf, 0xd9, 0xf1, 0xdb, 0x40, 0xa4, 0xa6, 0xaf
  )
end

function M.close()
  if M.opened then pcall(function() i2c.close(M.bus) end); M.opened = false end
end

return M
