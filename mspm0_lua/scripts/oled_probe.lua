-- OLED bus scan: both I2C1 pin pairs, addrs, rates
print("OLED_SCAN")
local pairs = {
  {"PA15","PA16"},
  {"PA17","PA18"},
}
local addrs = {0x3c, 0x3d, 0x27, 0x3f}
local rates = {100000, 400000}
local found = 0
for pi = 1, 2 do
  local scl, sda = pairs[pi][1], pairs[pi][2]
  for ri = 1, 2 do
    local hz = rates[ri]
    local okopen = pcall(function()
      i2c.open(1, scl, sda, hz)
    end)
    print("open", scl, sda, hz, okopen and "Y" or "N")
    if okopen then
      for ai = 1, 4 do
        local addr = addrs[ai]
        local st = pcall(function()
          assert(i2c.write(1, addr, bytes(0x00, 0xae)))
        end)
        if st then
          print("HIT", scl, sda, addr, hz)
          found = 1
        end
      end
      pcall(function() i2c.close(1) end)
    end
  end
end
if found == 0 then print("OLED_SCAN_NONE") else print("OLED_SCAN_OK") end
