# Interrupt events and cooperative tasks

The 1 ms TIMG0 ISR advances four timer slots. GPIOA/GPIOB use the GROUP1
hardware interrupt. ISRs only update saturating pending counters; Lua callbacks
always execute later in the main VM context.

## Thread-like tasks

```lua
task.spawn(function()
  gpio.mode("PA14", "out")
  while not stopped() do
    gpio.toggle("PA14")
    task.sleep(100)
  end
end)

task.spawn(function()
  while not stopped() do
    task.sleep(500)
    print("background")
  end
end)

event.run()
```

Tasks are cooperative Lua coroutines, not preemptive OS threads. Each task must
call `task.sleep()` or `task.yield()` regularly. Up to four tasks.

## Timer callbacks

```lua
local tid = tmr.every(100, function(id, hits)
  gpio.toggle("PA14")
  if hits > 1 then print("late", hits) end
end)

event.run()
tmr.stop(tid)
```

Do not mix callback and `tmr.ready` on the same id; same for `irq` callback vs
`irq.count` on the same pin.

## GPIO callbacks

```lua
irq.on("PA25", "rise", function(pin, hits)
  print(pin, hits)
end, 20)

event.run()
irq.off("PA25")
```

`irq.on()` owns the pin; do not `gpio.mode` it first.

## Event control

- `event.run()` — dispatch until `event.stop()` or no work; WFI while idle
- `event.poll()` — one dispatch round
- Console `!` stops event loops and tight Lua tasks
