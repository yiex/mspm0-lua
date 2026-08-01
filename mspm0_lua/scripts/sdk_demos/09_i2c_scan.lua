-- I2C bus scan (controller write probe with 0-length or 1-byte)
-- Default: I2C1 PA15/PA16 @100k. Falls back to I2C0 PA1/PA0.
print("I2C_SCAN_START")
local bus, scl, sda = 1, "PA15", "PA16"
local ok = pcall(function() i2c.open(bus, scl, sda, 100000) end)
if not ok then
  bus, scl, sda = 0, "PA1", "PA0"
  i2c.open(bus, scl, sda, 100000)
end
print("bus", bus, scl, sda)
local probe = bytes(0)
local hits, addr = 0, 0x08
while addr <= 0x77 do
  local r = i2c.write(bus, addr, probe)
  if r then
    print("HIT", addr)
    hits = hits + 1
  end
  addr = addr + 1
  yield()
end
i2c.close(bus)
print("I2C_SCAN_OK hits", hits)
