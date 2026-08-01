-- SDK: mcan_loopback → can.* internal loopback
print("HI_CAN_START")
if type(can) ~= "table" then
  print("HI_CAN_SKIP")
  return
end
can.open(500000, true)
assert(can.send(0x321, bytes(0x12, 0x34, 0x56), 100))
local id, data = can.recv(100)
assert(id == 0x321 and data and #data == 3)
assert(byte(data, 1) == 0x12)
can.close()
can.open(250000, true)
assert(can.send(0x100, bytes(0xaa), 100))
id, data = can.recv(100)
assert(id == 0x100)
can.close()
print("HI_CAN_OK")
