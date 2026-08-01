-- Place this on external SPI Flash as scripts/main.lua (phase-2 FS)
print("hello from external main.lua")
gpio.mode("PA14", "out")
for i = 1, 5 do
  gpio.toggle("PA14")
  delay_ms(200)
end
