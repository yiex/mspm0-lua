-- SDK: empty  →  cooperative idle (keeps stop/console alive)
-- ref: TI MSPM0 SDK examples/nortos/LP_MSPM0G3507/empty
print("EMPTY_IDLE_START")
local t0 = millis()
while not stopped() do
  if millis() - t0 >= 1000 then
    print("idle", millis())
    t0 = millis()
  end
  yield()
end
print("EMPTY_IDLE_STOP")
