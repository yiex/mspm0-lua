-- Full I2C1 scan PA15/PA16 then swap via soft only if HW open ok
print("BUS_SCAN")
local ok, err = pcall(function()
  i2c.open(1, "PA15", "PA16", 100000)
end)
print("open", ok and "Y" or tostring(err))
if not ok then
  print("BUS_SCAN_FAIL")
  return
end
local hits = 0
local a = 0x08
while a <= 0x77 do
  if i2c.write(1, a, bytes()) then
    print("HIT", a)
    hits = hits + 1
  end
  a = a + 1
end
print("hits", hits)
pcall(function() i2c.close(1) end)
if hits == 0 then
  print("BUS_SCAN_NONE")
else
  print("BUS_SCAN_OK")
end
