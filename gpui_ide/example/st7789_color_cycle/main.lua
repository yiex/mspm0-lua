-- ST7789 240x320 RGB565 color-cycle test for Di Meng Xing / MSPM0G3507.
-- SPI0: SCK=PA12, PICO(MOSI)=PA14, POCI(MISO)=PA13.
-- Connect display CS/DC/RST to PA15/PA16/PA17 respectively.

local PIN_SCK = "PA12"
local PIN_PICO = "PA14"
local PIN_POCI = "PA13"
local PIN_CS = "PA15"
local PIN_DC = "PA16"
local PIN_RST = "PA17"

local SPI_ID = 0
local SPI_HZ = 10000000
local SPI_MODE = 0

local function write_cmd(cmd)
    gpio.set(PIN_DC, 0)
    spi.xfer_on(SPI_ID, PIN_SCK, PIN_PICO, PIN_POCI, PIN_CS,
        spi.bytes(cmd), SPI_HZ, SPI_MODE)
end

local function write_data(data)
    gpio.set(PIN_DC, 1)
    spi.xfer_on(SPI_ID, PIN_SCK, PIN_PICO, PIN_POCI, PIN_CS,
        data, SPI_HZ, SPI_MODE)
end

local function write_byte(value)
    write_data(spi.bytes(value))
end

local function reset_display()
    gpio.set(PIN_RST, 0)
    delay_ms(10)
    gpio.set(PIN_RST, 1)
    delay_ms(120)
end

local function init_display()
    reset_display()
    write_cmd(0x11)
    delay_ms(120)
    write_cmd(0x3A)
    write_byte(0x55)
    write_cmd(0x36)
    write_byte(0x00)
    write_cmd(0x29)
    delay_ms(120)
end

local function set_full_window()
    write_cmd(0x2A)
    write_data(spi.bytes(0x00, 0x00, 0x00))
    write_data(spi.bytes(0xEF))
    write_cmd(0x2B)
    write_data(spi.bytes(0x00, 0x00, 0x01))
    write_data(spi.bytes(0x3F))
    write_cmd(0x2C)
end

local function make_line(color)
    local pixel = spi.bytes(color // 256, color % 256)
    local block = pixel .. pixel
    block = block .. block
    block = block .. block
    block = block .. block

    local line = block
    for i = 2, 15 do
        line = line .. block
    end
    return line
end

local function fill_color(color)
    local line = make_line(color)
    set_full_window()
    for i = 1, 320 do
        write_data(line)
    end
end

if not spi.valid(SPI_ID, PIN_SCK, PIN_PICO, PIN_POCI) then
    error("ST7789: invalid SPI0 route")
end

gpio.mode(PIN_DC, "out")
gpio.mode(PIN_RST, "out")
init_display()
print("ST7789_READY")

while not stopped() do
    fill_color(0xF800)
    delay_ms(2000)
    fill_color(0x07E0)
    delay_ms(2000)
    fill_color(0x001F)
    delay_ms(2000)
    fill_color(0xFFFF)
    delay_ms(2000)
    fill_color(0x0000)
    delay_ms(2000)
end

gpio.set(PIN_DC, 0)
gpio.set(PIN_RST, 0)
print("ST7789_STOPPED")
