-- PA26 out -> PA25 irq (wire if needed)
local edges = 0
gpio.mode("PA26", "out")
gpio.set("PA26", 0)

irq.on("PA25", "both", function(pin, hits)
  edges = edges + hits
end, 0)

task.spawn(function()
  for i = 1, 10 do
    task.sleep(50)
    gpio.toggle("PA26")
  end
  task.sleep(20)
  irq.off("PA25")
end)

event.run()
gpio.set("PA26", 0)
print("HI_IRQ", edges)
