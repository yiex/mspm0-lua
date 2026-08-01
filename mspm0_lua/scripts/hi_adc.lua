-- adc.read + Lua-side mV: raw * vdda / 4095
local ch = adc.channel("PA27") or 0
for i = 1, 5 do
  local raw = adc.read(ch)
  local mv = raw and ((raw * 3300) // 4095) or -1
  print("adc", ch, raw or -1, "mv", mv)
  delay_ms(50)
end
print("HI_ADC_OK")
