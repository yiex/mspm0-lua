-- Exercise NACK cleanup and repeated controller initialization.
-- The test passes without an OLED; OLED_ACK reports whether 0x3c answered.
print("I2C_RECOVERY_START")

local round = 1
while round <= 5 do
  i2c.open(1, "PA15", "PA16", 100000)

  -- Reserved address should NACK. The next transaction must still start.
  local n = 1
  while n <= 4 do
    i2c.write(1, 0x7f, bytes(0))
    n = n + 1
  end

  local oled_ack = i2c.write(1, 0x3c, bytes(0x00))
  print("ROUND", round, "OLED_ACK", oled_ack)
  i2c.close(1)
  round = round + 1
  yield()
end

print("I2C_RECOVERY_OK")
