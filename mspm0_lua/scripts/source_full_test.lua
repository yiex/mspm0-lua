print("SOURCE_FULL_START")

local function api(name, value)
  if type(value) ~= "function" then
    error("missing API: " .. name)
  end
end

api("gpio.mode", gpio.mode)
api("gpio.af", gpio.af)
api("irq.on", irq.on)
api("tmr.every", tmr.every)
api("pwm.open", pwm.open)
api("uart.write", uart.write)
api("adc.read", adc.read)
api("i2c.open", i2c.open)
api("spi.open", spi.open)
api("runfile", runfile)

gpio.mode("PA14", "out")
gpio.set("PA14", 1)
delay_ms(120)
gpio.set("PA14", 0)

-- PA27 is a free expansion-header pin. PF1 is GPIO; this also verifies that
-- pins outside the old trimmed table are accepted by the compact decoder.
gpio.mode("PA27", "out")
gpio.set("PA27", 0)
gpio.af("PA27", 1, 1)

print("SOURCE_FULL_OK")
