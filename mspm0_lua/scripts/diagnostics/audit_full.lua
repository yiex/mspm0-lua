local function must_fail(fn, label)
    local ok = pcall(fn)
    assert(not ok, label)
end

assert(gpio.valid("PA14"), "gpio valid")
gpio.mode("PA14", "out", 0)
gpio.set("PA14", 1)
gpio.toggle("PA14")
gpio.release("PA14")

tmr.start(0, 2)
tmr.delay(6)
assert(tmr.take(0) >= 1, "soft timer")
tmr.stop(0)

tmr.hw_start(6, 32000, 0, 1)
tmr.delay(2)
assert(tmr.hw_value(6) >= 0, "hardware timer")
tmr.hw_stop(6)
must_fail(function() tmr.hw_value(6) end, "stopped timer accepted")

local capture = tmr.capture_open(3, "PA21", 0, 0)
must_fail(function() tmr.capture_close(capture, "PB2") end,
    "wrong capture pin accepted")
tmr.capture_close(capture, "PA21")

local pwm_id = pwm.open("PA14", 1000, 25)
must_fail(function() pwm.open_on(6, "PB20", 1000, 25) end,
    "duplicate PWM channel accepted")
must_fail(function() pwm.close(pwm_id, "PB20") end,
    "wrong PWM pin accepted")
pwm.duty(pwm_id, 60)
pwm.close(pwm_id, "PA14")
must_fail(function() pwm.close(pwm_id, "PA14") end,
    "closed PWM accepted")

assert(adc.channel("PA27") == 0, "ADC route")
local sample = adc.read("PA27", 64, 4, 12)
assert(sample >= 0 and sample <= 4095, "ADC sample")

assert(i2c.probe_on(1, "PA15", "PA16", 0x3c, 400000), "OLED probe")
assert(i2c.write(0x3c,
    "\x00\xAE\xD5\x80\xA8\x3F\xD3\x00\x40\x8D\x14\x20\x02" ..
    "\xA1\xC8\xDA\x12\x81\xCF\xD9\xF1\xDB\x40\xA4\xA6\xAF",
    400000), "OLED init")
assert(i2c.write(0x3c, "\x00\xB0\x00\x10", 400000), "OLED address")
assert(i2c.write(0x3c,
    "\x40\x00\x42\x7F\x40\x00\x00" ..
    "\x42\x61\x51\x49\x46\x00" ..
    "\x21\x41\x45\x4B\x31\x00", 400000), "OLED data")

assert(spi.valid(0, "PA12", "PA14", "PA13"), "SPI route")
local spi_data = spi.xfer("PA18", "\xA5\x5A", 1000000, 0, 0)
assert(#spi_data == 2, "SPI transfer")

assert(uart.valid(0, "PA0", "PA1"), "UART route")
uart.open(0, "PA0", "PA1", 115200, 8, "none", 1)
uart.close(0)
print("AUDIT_UART0_RESTORED")

can.open(500000, true, "PA26", "PA27")
assert(can.send(0x321, "\x11\x22\x33", 100, false), "CAN send")
local can_id, can_data, can_extended = can.recv(100)
assert(can_id == 0x321 and #can_data == 3 and not can_extended, "CAN recv")
can.close()

print("AUDIT_FULL_OK", sample, #spi_data)
