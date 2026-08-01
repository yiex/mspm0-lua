print('HI_LED_START')
gpio.mode('PA14', 'out')
for i = 1, 6 do
  gpio.toggle('PA14')
  delay_ms(120)
end
gpio.set('PA14', 0)
gpio.release('PA14')
print('HI_LED_OK')
