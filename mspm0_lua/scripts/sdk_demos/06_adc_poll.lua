-- TI-style ADC12 single-shot poll (channels 0..7)
-- Pin mux: call gpio.af(pin, 0) before sampling analog pins if needed.
print("ADC_POLL_START")
local ch = 0
while ch < 8 do
  local v = adc.read(ch)
  if v then print("ch", ch, v) else print("ch", ch, "fail") end
  ch = ch + 1
end
-- continuous sample ch0 until stop
local n, t0 = 0, millis()
while not stopped() and n < 20 do
  local v = adc.read(0)
  print("adc0", v or -1)
  n = n + 1
  delay_ms(100)
end
print("ADC_POLL_OK", n)
