-- SDK: spi_controller_register_format (logic + open/close)
-- ref: TI MSPM0 SDK examples/nortos/LP_MSPM0G3507/spi_controller_register_format
-- SPI0 (id=1) PA12/SCK PA14/PICO PA13/POCI CS=PA18 — does not touch LittleFS SPI1.
-- No peripheral attached: pattern smoke only; MISO may read 0xFF/0x00.
print("SPI_REG_START")

local function dump(tag, s)
  if not s then print(tag, "nil"); return end
  local i = 1
  local parts = tag
  while i <= #s do
    local b = byte(s, i)
    parts = parts .. " " .. b
    i = i + 1
  end
  print(parts)
end

local ok, err = pcall(function()
  spi.open(1, "PA12", "PA14", "PA13", "PA18", 1000000)
end)
if not ok then
  print("SPI0 open fail:", err)
  print("SPI_REG_SKIP")
  return
end

-- CMD_WRITE_TYPE_0/1/2 then CMD_READ_TYPE_0/1/2 (SDK command codes)
local rx
rx = spi.xfer(1, bytes(3, 0x11))
dump("W0", rx)
rx = spi.xfer(1, bytes(4, 8, 9))
dump("W1", rx)
rx = spi.xfer(1, bytes(5, 65, 66, 67, 68, 69, 70))
dump("W2", rx)
rx = spi.xfer(1, bytes(0, 0xff))
dump("R0", rx)
rx = spi.xfer(1, bytes(1, 0xff, 0xff))
dump("R1", rx)
rx = spi.xfer(1, bytes(2, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff))
dump("R2", rx)

spi.close(1)
gpio.mode("PA14", "out")
local i = 1
while i <= 6 do
  gpio.toggle("PA14")
  delay_ms(80)
  i = i + 1
end
print("SPI_REG_OK")
