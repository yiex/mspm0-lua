-- SSD1306 + 6x8 text helpers (uses require font6x8)
local base = require("oled_ssd1306")
local font = require("font6x8")
local M = base

function M.put_char(x, page, ch)
  M.set_cursor(x, page)
  M.data(font.glyph_bytes(ch))
  return x + 6
end

function M.put_str(x, page, s)
  local i = 1
  while true do
    local b = byte(s, i)
    if not b then break end
    x = M.put_char(x, page, b)
    if x > 122 then break end
    i = i + 1
  end
  return x
end

-- int tenths -> "-12.3"
function M.fmt_x10(v)
  local n, s = v, ""
  if n < 0 then s = "-"; n = -n end
  return s .. (n // 10) .. "." .. (n % 10)
end

return M
