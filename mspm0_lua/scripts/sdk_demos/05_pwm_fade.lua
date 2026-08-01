-- TI-style TIMG PWM duty ramp on PA14 LED (native board_pwm)
print("PWM_FADE_START")
local id = pwm.open("PA14", 1000)
local dir, duty = 1, 0
local cycles = 0
while not stopped() and cycles < 4 do
  pwm.duty(id, duty)
  duty = duty + dir * 5
  if duty >= 100 then duty = 100; dir = -1; cycles = cycles + 1 end
  if duty <= 0 then duty = 0; dir = 1 end
  delay_ms(30)
end
pwm.duty(id, 0)
pwm.close(id)
print("PWM_FADE_OK")
