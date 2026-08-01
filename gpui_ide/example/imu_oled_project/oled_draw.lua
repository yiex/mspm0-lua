local display = require("oled")
local M = {}

local I2C_ID = 1
local I2C_SCL = "PA15"
local I2C_SDA = "PA16"
local OLED_ADDRESS = 0x3c
local I2C_HZ = 100000

function M.open()
  display.open(I2C_ID, I2C_SCL, I2C_SDA, OLED_ADDRESS, I2C_HZ)
  display.clear()
  display.number(0, 16, 0, 1, 8)
  display.number(0, 32, 0, 1, 8)
  display.number(0, 48, 0, 1, 8)
end

function M.paint(page, value)
  display.number(0, page * 8, value, 1, 8)
end

return M
