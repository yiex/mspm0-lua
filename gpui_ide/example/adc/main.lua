print('HI_ADC_START')
local pins = {'PA27', 'PA26', 'PA25', 'PA24'}
for i = 1, #pins do
  local pin = pins[i]
  local raw = adc.read(pin)
  local mv = raw and (raw * 3300) // 4095 or -1
  print(pin, 'raw', raw or -1, 'mv', mv)
end
print('HI_ADC_OK')
