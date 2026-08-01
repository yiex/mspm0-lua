-- main.luac example; install oled_ssd1306.luac before running this file.
local oled = require("oled_ssd1306")

oled.init(1, "PA17", "PA18", 0x3c)
oled.clear()
oled.set_cursor(0, 0)
oled.data(bytes(0xff, 0x81, 0xbd, 0xa5, 0xa5, 0xbd, 0x81, 0xff))
delay_ms(1000)
oled.close()
print("OLED_DEMO_OK")
