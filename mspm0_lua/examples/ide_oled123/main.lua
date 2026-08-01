local oled = require("oled")

oled.open(1, "PA15", "PA16", 0x3c, 100000)
oled.clear()
oled.text(0, 0, "123", 16)
print("OLED_123_AUTO_FONT_OK")
