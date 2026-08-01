local oled = require("oled_ssd1306")
local cached = require("oled_ssd1306.luac")

assert(oled == cached)
assert(type(oled.init) == "function")
assert(type(oled.clear) == "function")
print("MODULE_REQUIRE_OK")
