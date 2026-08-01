-- OLED draw module (compile/upload as oled_draw.luac)
-- I2C1 PA15=SCL PA16=SDA, SSD1306 0x3C @ 100kHz
-- Exposes globals used by main.lua after runfile('oled_draw.luac')

SP={0,0,0,0,0,0}
MIN={8,8,8,8,8,0}
DOT={0,96,96,0,0,0}
DIG={
  {62,81,73,69,62,0},{0,66,127,64,0,0},{66,97,81,73,70,0},{33,65,69,75,49,0},
  {24,20,18,127,16,0},{39,69,69,69,57,0},{60,74,73,73,48,0},{1,113,9,5,3,0},
  {54,73,73,73,54,0},{6,73,73,41,30,0}
}
CA={124,18,17,18,124,0}
CT={1,1,127,1,1,0}
CK={127,8,20,34,65,0}
CI={0,65,127,65,0,0}
CM={127,2,12,2,127,0}
CU={63,64,64,64,63,0}
CR={127,9,25,41,70,0}
CP={127,9,9,9,6,0}
CY={7,8,112,8,7,0}
COL={0,54,54,0,0,0}

OLED_BUS, OLED_ADDR = 1, 0x3c
oled_ok, oe, skips = 0, 0, 0

function oled_wv(...)
  return i2c.writev(OLED_BUS, OLED_ADDR, ...)
end
function oled_cmd1(a)
  return oled_wv(0x00, a)
end
function oled_d6(g)
  return oled_wv(0x40, g[1], g[2], g[3], g[4], g[5], g[6])
end
function oled_cur(x, page)
  return oled_cmd1(0xb0 + page) and oled_cmd1(x % 16) and oled_cmd1(0x10 + (x // 16))
end

function oled_paint_val(x, page, v)
  if not oled_cur(x, page) then return false end
  local n, neg = v, 0
  if n < 0 then neg = 1; n = -n end
  if n > 9999 then n = 9999 end
  local ip, fp = n // 10, n % 10
  local d2 = ip % 10
  local d1 = (ip // 10) % 10
  local d0 = (ip // 100) % 10
  if not oled_d6(neg == 1 and MIN or SP) then return false end
  if not oled_d6(ip >= 100 and DIG[d0 + 1] or SP) then return false end
  if not oled_d6(ip >= 10 and DIG[d1 + 1] or SP) then return false end
  if not oled_d6(DIG[d2 + 1]) then return false end
  if not oled_d6(DOT) then return false end
  if not oled_d6(DIG[fp + 1]) then return false end
  return true
end

function oled_puts(x, page, list)
  if not oled_cur(x, page) then return false end
  local i = 1
  while list[i] do
    if not oled_d6(list[i]) then return false end
    i = i + 1
  end
  return true
end

function oled_init()
  i2c.open(1, "PA15", "PA16", 100000)
  local init = {
    0xae,0xd5,0x80,0xa8,0x3f,0xd3,0x00,0x40,
    0x8d,0x14,0x20,0x02,0xa1,0xc8,0xda,0x12,
    0x81,0xcf,0xd9,0xf1,0xdb,0x40,0xa4,0xa6,0xaf
  }
  local i = 1
  while init[i] do
    if not oled_cmd1(init[i]) then error("i") end
    i = i + 1
  end
  local p = 0
  while p < 8 do
    if not (oled_cmd1(0xb0 + p) and oled_cmd1(0x00) and oled_cmd1(0x10)) then error("c") end
    local c = 0
    while c < 128 do
      if not oled_wv(0x40, 0, 0, 0, 0, 0, 0, 0) then error("z") end
      c = c + 7
    end
    if not oled_wv(0x40, 0, 0) then error("z2") end
    p = p + 1
  end
end

function oled_boot_ui()
  oled_init()
  oled_puts(0, 0, {CA, CT, CK, SP, CI, CM, CU})
  oled_puts(0, 2, {CR, COL})
  oled_puts(0, 4, {CP, COL})
  oled_puts(0, 6, {CY, COL})
  oled_paint_val(18, 2, 0)
  oled_paint_val(18, 4, 0)
  oled_paint_val(18, 6, 0)
  oled_ok = 1
end

function oled_paint_axis(which, v)
  if oled_ok ~= 1 then return false end
  local page = 2
  if which == 1 then page = 4 elseif which == 2 then page = 6 end
  local ok = pcall(function()
    if not oled_paint_val(18, page, v) then error(1) end
  end)
  if ok then oe = 0; return true end
  oe = oe + 1
  skips = skips + 1
  if oe > 100 then oe = 100 end
  return false
end

print("OLED_MOD_LOADED")
