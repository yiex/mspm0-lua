-- Soft timer callback + PA14
gpio.mode("PA14", "out")
local n = 0
tmr.every(100, function(id, hits)
  n = n + hits
  gpio.toggle("PA14")
  if n >= 10 then
    tmr.stop(id)
    event.stop()
  end
end)
event.run()
gpio.set("PA14", 0)
print("HI_TMR_OK", n)
