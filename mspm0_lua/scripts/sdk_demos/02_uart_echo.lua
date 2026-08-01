-- SDK: uart_echo_interrupts_standby  →  cooperative console echo
-- ref: TI MSPM0 SDK examples/nortos/LP_MSPM0G3507/uart_echo_interrupts_standby
-- Firmware RX IRQ fills ring; Lua polls and echoes (no WFI / sleep-on-exit).
print("UART_ECHO_START")
print("type chars; ! to stop")
gpio.mode("PA14", "out")
local n = 0
while not stopped() do
  local s = uart.read(50, 64)
  if s then
    uart.write(s)
    n = n + #s
    gpio.toggle("PA14")
  end
  yield()
end
print("UART_ECHO_STOP bytes", n)
