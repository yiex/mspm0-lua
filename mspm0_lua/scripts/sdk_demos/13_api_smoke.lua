-- One-shot inventory of Lua APIs present on the running firmware
print("API_SMOKE_START")
local function has(t, k)
  return type(t) == "table" and type(t[k]) == "function"
end
local function g(name)
  return type(_G[name]) == "function"
end

print("core", g("delay_ms"), g("millis"), g("yield"), g("stopped"), g("bytes"), g("byte"))
print("gpio", has(gpio, "mode"), has(gpio, "set"), has(gpio, "get"), has(gpio, "toggle"), has(gpio, "af"))
print("irq", has(irq, "on"), has(irq, "off"), has(irq, "count"))
print("uart0", has(uart, "write"), has(uart, "read"))
print("uart_app", has(uart, "open"), has(uart, "tx"), has(uart, "rx"), has(uart, "close"))
print("i2c", has(i2c, "open"), has(i2c, "write"), has(i2c, "writev"), has(i2c, "read"), has(i2c, "write_read"))
print("spi", has(spi, "open"), has(spi, "xfer"), has(spi, "cs"), has(spi, "close"))
print("pwm", has(pwm, "open"), has(pwm, "duty"), has(pwm, "close"))
print("adc", has(adc, "read"), has(adc, "capture"), has(adc, "channel"))
print("tmr", has(tmr, "every"), has(tmr, "ready"), has(tmr, "stop"))
print("event", has(event, "run"), has(event, "poll"), has(event, "stop"))
print("task", has(task, "spawn"), has(task, "sleep"), has(task, "yield"), has(task, "cancel"))
print("oled", type(oled) == "table" and has(oled, "open"))
print("mod", g("runfile"), g("require"))

gpio.mode("PA14", "out")
gpio.set("PA14", 1)
delay_ms(80)
gpio.set("PA14", 0)
print("API_SMOKE_OK", millis())
