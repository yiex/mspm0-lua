-- Cooperative tasks + PA14 blink
task.spawn(function()
  gpio.mode("PA14", "out")
  while not stopped() do
    gpio.toggle("PA14")
    task.sleep(200)
  end
  gpio.set("PA14", 0)
end)

task.spawn(function()
  for i = 1, 5 do
    print("tick", i, millis())
    task.sleep(150)
  end
end)

event.run()
print("HI_TASK_OK")
