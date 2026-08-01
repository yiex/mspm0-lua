-- Secondary UART open/tx smoke (UART2 defaults)
-- For true loopback, wire PA23(TX) to PA24(RX). Without wire, only open/tx checked.
print("UART_APP_START")
if type(uart.open) ~= "function" then
  print("UART_APP_SKIP need bytecode fw")
  return
end
uart.open(2, "PA23", "PA24", 115200)
uart.tx(2, "PING")
local got = uart.rx(2, 50, 16)
if got then
  print("rx", #got, byte(got, 1))
else
  print("rx none (no loopback wire?)")
end
uart.close(2)
print("UART_APP_OK")
