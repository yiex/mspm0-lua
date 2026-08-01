-- Pin claim / conflict smoke test (no hardware required beyond console).
print("owner PA14", gpio.owner("PA14") or "nil")
pwm.open("PA14", 1000)
print("after pwm", gpio.owner("PA14"))
local ok, err = pcall(function()
  irq.on("PA14", "fall")
end)
print("irq on busy pin", ok and "ok" or err)
pwm.close(0)
gpio.release("PA14")
print("released", gpio.owner("PA14"))

-- ADC pin map
print("PA27 ch", adc.channel("PA27"))
print("PA10 ch", adc.channel("PA10")) -- should be nil

-- locked console
ok, err = pcall(function()
  gpio.mode("PA10", "out")
end)
print("PA10 mode", ok and "ok" or err)

-- complementary pair validation
ok, err = pcall(function()
  pwm.comp("PA8", "PA9", 10000, 50, 200)
end)
print("bad pair", ok and "ok" or err)
ok, err = pcall(function()
  pwm.comp("PA8", "PA22", 10000, 50, 200)
end)
print("good pair", ok and "ok" or err)
if ok then pwm.comp_close() end
print("hi_pinmux done")
