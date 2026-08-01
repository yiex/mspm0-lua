print("UART test from uploaded script")
uart.write("uart.write hello\r\n")
log.info("log.info ok")
gpio.mode("PA14", "out")
for i = 1, 4 do
  gpio.toggle("PA14")
  delay_ms(100)
end
-- peripheral logical smoke
local p = pwm.open("PA15", 1000)
pwm.duty(p, 75)
pwm.close(p)
spi.open(0)
local echo = spi.xfer(0, "AB")
print("spi echo len", #echo)
i2c.open(0)
print("adc0", adc.read(0))
print("uart test done")
