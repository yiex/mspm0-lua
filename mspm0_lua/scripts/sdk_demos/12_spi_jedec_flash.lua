-- SPI1 shared with W25Q: JEDEC ID via separate CS (demo uses hold carefully)
-- WARNING: SPI1 bus is PB16/15/14; Flash CS is PB17 (firmware-managed).
-- This demo uses app CS PA18 and does NOT touch Flash CS — JEDEC only works
-- if you temporarily use board-level Flash helpers (not exposed).
-- Instead: SPI0 JEDEC-style 0x9F read pattern for any SPI NOR on SPI0.
print("SPI_JEDEC_START")
local ok = pcall(function()
  spi.open(1, "PA12", "PA14", "PA13", "PA18", 1000000)
end)
if not ok then
  print("SPI_JEDEC_SKIP")
  return
end
-- JEDEC ID command 0x9F + 3 dummy clocks
local rx = spi.xfer(1, bytes(0x9f, 0xff, 0xff, 0xff))
if rx then
  print("jedec", byte(rx, 2), byte(rx, 3), byte(rx, 4))
else
  print("xfer fail")
end
spi.close(1)
print("SPI_JEDEC_OK")
