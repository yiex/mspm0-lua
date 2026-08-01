print('PERIPHERAL_TEST_START')

assert(type(uart.open) == 'function')
assert(type(can.open) == 'function')

-- The secondary buses are opened and closed without touching UART0 or SPI1.
uart.open(1, 'PA17', 'PA18', 115200)
uart.close(1)
i2c.open(1, 'PA17', 'PA18', 100000)
i2c.close(1)
spi.open(1, 'PA12', 'PA14', 'PA13', 'PA18', 1000000)
spi.close(1)

-- MCAN internal loopback does not require an external CAN transceiver.
-- Run twice in one VM to cover power-down and run-time reinitialization.
local function check_can(rate, txid, first)
  can.open(rate, true)
  assert(can.send(txid, bytes(first, 0x34, 0x56), 100))
  local rxid, data = can.recv(100)
  assert(rxid == txid)
  assert(#data == 3)
  assert(byte(data, 1) == first)
  can.close()
end

check_can(500000, 0x321, 0x12)
check_can(250000, 0x123, 0x5a)

print('PERIPHERAL_TEST_OK')
