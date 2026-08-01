local I2C_ID = 1
local I2C_SCL = 'PA15'
local I2C_SDA = 'PA16'
local I2C_HZ = 100000

print('I2C_SCAN_START', I2C_ID, I2C_SCL, I2C_SDA, I2C_HZ)
local hits, addr = 0, 0x08
while addr <= 0x77 do
  if i2c.probe_on(I2C_ID, I2C_SCL, I2C_SDA, addr, I2C_HZ) then
    print('HIT', addr)
    hits = hits + 1
  end
  if addr % 0x10 == 0 then print('SCAN', addr) end
  addr = addr + 1; yield()
end
print('I2C_SCAN_OK', 'hits', hits)
