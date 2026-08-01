-- ISR-driven periodic timer. Lua callback runs in the event dispatcher.
print("TMR_BLINK_START")
gpio.mode("PA14", "out")

local n = 0
local tid
tid = tmr.every(200, function(id, hits)
  for i = 1, hits do
    if n >= 20 then break end
    gpio.toggle("PA14")
    n = n + 1
  end
  if n >= 20 then
    tmr.stop(id)
    gpio.set("PA14", 0)
  end
end)

event.run()
print("TMR_BLINK_OK", n)
