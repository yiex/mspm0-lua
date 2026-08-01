-- Wire PA14 -> PA22 (or PA26 -> PA22). Uses irq.count (no event.run needed).
print("HI_CAP_DIAG")

gpio.release("PA14")
gpio.release("PA22")
gpio.release("PA26")
gpio.release("PA25")

-- baseline known: PA26->PA25
gpio.mode("PA26", "out")
gpio.set("PA26", 0)
irq.on("PA25", "both", nil, 0)
for i = 1, 10 do gpio.toggle("PA26"); delay_ms(2) end
print("KNOWN25", irq.count("PA25"))
irq.off("PA25")

-- sense PA22 driven by PA26
irq.on("PA22", "both", nil, 0)
for i = 1, 10 do gpio.toggle("PA26"); delay_ms(2) end
print("D26_S22", irq.count("PA22"))
irq.off("PA22")

-- sense PA22 driven by PA14
gpio.release("PA26")
gpio.mode("PA14", "out")
gpio.set("PA14", 0)
irq.on("PA22", "both", nil, 0)
for i = 1, 10 do gpio.toggle("PA14"); delay_ms(2) end
print("D14_S22", irq.count("PA22"))
irq.off("PA22")

-- PWM + cap if wire likely ok
gpio.release("PA14")
pwm.open("PA14", 1000)
pwm.duty(0, 50)
cap.open("PA22", 0)
local ok = 0
for i = 1, 50 do
  delay_ms(20)
  if cap.ready() then
    local hz = cap.hz_x10()
    print("CAP", cap.period(), hz)
    if hz > 5000 and hz < 20000 then ok = 1; break end
  end
end
print("CAP_READY", cap.ready() and 1 or 0, "OK", ok)
cap.close()
pwm.close(0)
print("HI_CAP_DIAG_DONE", ok)
