-- full OLED init + text smoke (PA15/PA16 preferred)
print("OLED_FULL")
local oled = require("oled_ssd1306")
local font = require("font6x8")
local function putc(x,page,ch)
  oled.set_cursor(x,page)
  oled.data(font.glyph_bytes(ch))
  return x+6
end
local function puts(x,page,s)
  local i=1
  while true do
    local b=byte(s,i)
    if not b then break end
    if b>=97 and b<=122 then b=b-32 end
    x=putc(x,page,b)
    if x>122 then break end
    i=i+1
  end
end
oled.init(1,"PA15","PA16",0x3c,400000)
oled.clear()
puts(0,0,"OLED OK")
puts(0,2,"PA15/PA16")
puts(0,4,"400KHZ")
puts(0,6,"0X3C")
print("OLED_FULL_OK")
-- leave display on; close bus only
oled.close()
