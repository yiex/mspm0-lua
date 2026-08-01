-- SDK: gpio_toggle_output  →  Dimengxing PA14 LED
-- ref: TI MSPM0 SDK examples/nortos/LP_MSPM0G3507/gpio_toggle_output
print("GPIO_TOGGLE_START")
gpio.mode("PA14", "out")
gpio.set("PA14", 1)
local n = 0
while not stopped() do
  gpio.toggle("PA14")
  n = n + 1
  if n % 2 == 0 then print("toggles", n // 2) end
  delay_ms(250)
end
gpio.set("PA14", 0)
print("GPIO_TOGGLE_STOP", n)
