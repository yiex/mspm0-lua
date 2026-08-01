local oled = require("oled")

local I2C_ID = 1
local I2C_SCL = "PA15"
local I2C_SDA = "PA16"
local OLED_ADDRESS = 0x3c
local I2C_HZ = 100000

print("OLED_AUTO_FONT_START")
oled.open(I2C_ID, I2C_SCL, I2C_SDA, OLED_ADDRESS, I2C_HZ)
oled.clear()
oled.text(0, 0, "123", 16)
print("OLED_AUTO_FONT_OK")
