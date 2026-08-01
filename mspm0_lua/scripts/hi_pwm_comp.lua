-- Complementary PWM: PA8 (CCP0) + PA22 (CCP0_CMPL), dead-band ns.
-- Scope: expect non-overlap high/low around edges.
pwm.comp(20000, 50, 500)
print("comp 20kHz 50% dead=500ns PA8/PA22")
local t = tmr.every(500)
local d = 20
while not stopped() do
  if tmr.ready(t) then
    d = d + 10
    if d > 80 then d = 20 end
    pwm.comp_duty(d)
    print("duty", d)
  end
  yield()
end
pwm.comp_close()
print("done")
