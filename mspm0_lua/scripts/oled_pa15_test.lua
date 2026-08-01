-- PA15/PA16 OLED hard check (do not gpio.mode pins before oled/i2c claim)
print("OLED_PA15_TEST")

local ok, err = pcall(function()
  i2c.open(1, "PA15", "PA16", 100000)
end)
print("i2c_open", ok and "Y" or tostring(err))
if ok then
  print("ACK3C", i2c.write(1, 0x3c, bytes(0x00, 0xae)) and 1 or 0)
  print("ACK3D", i2c.write(1, 0x3d, bytes(0x00, 0xae)) and 1 or 0)
  pcall(function() i2c.close(1) end)
end

ok, err = pcall(function()
  oled.open("PA15", "PA16")
  oled.fill(0xff)
  delay_ms(300)
  oled.clear()
  oled.cursor(0, 0)
  oled.print("PA15/16 OK")
  oled.num(0, 2, 12345, 0)
end)
if ok then
  print("OLED_PA15_OK")
else
  print("OLED_PA15_FAIL", tostring(err))
end
