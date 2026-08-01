-- Hardware GPIO interrupt callback. Wire PA26 directly to PA25.
-- The ISR only counts edges; this Lua callback runs in the event dispatcher.
print("GPIO_IRQ_START")

local total = 0
gpio.mode("PA26", "out")
gpio.set("PA26", 0)

irq.on("PA25", "both", function(pin, hits)
  total = total + hits
  print("edges", pin, hits, "total", total)
end, 0)

task.spawn(function()
  for i = 1, 20 do
    task.sleep(100)
    gpio.toggle("PA26")
  end
  task.sleep(30)
  irq.off("PA25")
end)

event.run()
gpio.set("PA26", 0)
print("GPIO_IRQ_OK total", total)
