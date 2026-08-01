-- Classic CAN internal loopback (bytecode profile)
-- Hardware: PA26 TX / PA27 RX; no transceiver needed in loopback.
print("CAN_LB_START")
if type(can) ~= "table" or type(can.open) ~= "function" then
  print("CAN_LB_SKIP need bytecode fw")
  return
end
can.open(500000, true)
assert(can.send(0x321, bytes(0x12, 0x34, 0x56), 100))
local id, data = can.recv(100)
assert(id == 0x321)
assert(data and #data == 3)
assert(byte(data, 1) == 0x12)
can.close()
-- reopen different rate
can.open(250000, true)
assert(can.send(0x123, bytes(0x5a), 100))
id, data = can.recv(100)
assert(id == 0x123)
can.close()
print("CAN_LB_OK")
