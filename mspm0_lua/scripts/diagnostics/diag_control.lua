gpio.mode("PA14", "out")
gpio.set("PA14", 1)
gpio.toggle("PA14")
gpio.release("PA14")

tmr.start(0, 10)
while not tmr.ready(0) do yield() end
tmr.stop(0)

local id = pwm.open("PA14", 1000, 20)
tmr.delay(5)
pwm.duty(id, 60)
tmr.delay(5)
pwm.close(id, "PA14")

assert(adc.channel("PA27") == 0, "adc channel")
local sample = adc.read("PA27")
assert(sample >= 0 and sample <= 4095, "adc sample")
adc.release("PA27")
print("DIAG_CONTROL_OK")
