assert(plug and plug.ping() == 3507, "plug")
assert(gpio, "gpio")
assert(tmr, "tmr")

gpio.mode("PA14", "out")
gpio.set("PA14", 1)
gpio.toggle("PA14")

local started = tmr.millis()
tmr.start(0, 25)
while not tmr.ready(0) do
    yield()
end
tmr.stop(0)

print("MODULE_SET_OK", plug.ping(), tmr.millis() - started)
