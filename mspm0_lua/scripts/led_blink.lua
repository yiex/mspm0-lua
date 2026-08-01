-- PA14 LED blink test for the default full-feature firmware.
print("LED_BLINK_START")

gpio.mode("PA14", "out")
for i = 1, 6 do
  gpio.set("PA14", 1)
  delay_ms(150)
  gpio.set("PA14", 0)
  delay_ms(150)
end

gpio.release("PA14")
print("LED_BLINK_DONE")
