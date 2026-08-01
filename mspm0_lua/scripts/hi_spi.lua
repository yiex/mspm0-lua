-- SDK: spi_controller_register_format spirit (SPI0, no slave needed)
print("HI_SPI_START")
local ok = pcall(function()
  spi.open(1, "PA12", "PA14", "PA13", "PA18", 1000000)
end)
if not ok then
  print("HI_SPI_SKIP")
  return
end
-- cmd + payload pattern
local rx = spi.xfer(1, bytes(0x9f, 0xff, 0xff, 0xff))
if rx then
  print("jedec_pat", byte(rx, 2), byte(rx, 3), byte(rx, 4))
end
rx = spi.xfer(1, bytes(3, 0x11))
print("w0_len", rx and #rx or 0)
spi.close(1)
-- restore PA14 after SPI0 used it as PICO
gpio.mode("PA14", "out")
gpio.set("PA14", 0)
print("HI_SPI_OK")
