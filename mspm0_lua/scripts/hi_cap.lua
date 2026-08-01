-- Wire PA14 (PWM 1kHz) -> PA22 (TIMG6 CCP0). Confirmed D14_S22 edges.
print("HI_CAP_START")
gpio.release("PA14")
gpio.release("PA22")
pwm.open("PA14", 1000)
pwm.duty(0, 50)
delay_ms(10)
cap.open("PA22", 0)
local ok = 0
for i = 1, 60 do
  delay_ms(20)
  local h = cap.hits()
  if (i % 10) == 0 then
    print("HITS", h, "RDY", cap.ready() and 1 or 0, "P", cap.period(), "HZ10", cap.hz_x10())
  end
  if cap.ready() then
    local hz = cap.hz_x10()
    print("CAP", cap.period(), hz)
    if hz > 5000 and hz < 20000 then
      ok = 1
      break
    end
  end
end
if ok == 0 then
  print("CAP_FAIL hits", cap.hits())
end
cap.close()
pwm.close(0)
print("HI_CAP_OK", ok)
