assert(uart and gpio and tmr and pwm and adc and i2c and spi and can,
    "module set")

local function check_uart(id, tx, rx, bits, parity, stop)
    assert(uart.valid(id, tx, rx), "uart route")
    uart.open(id, tx, rx, 115200, bits, parity, stop)
    uart.close(id)
end
check_uart(0, "PA0", "PA1", 8, "none", 1)
check_uart(1, "PA17", "PA18", 7, "even", 1)
check_uart(2, "PA23", "PA24", 8, "none", 1)
check_uart(3, "PA26", "PA25", 6, "odd", 2)

gpio.mode("PA14", "out")
gpio.set("PA14", 1)
gpio.toggle("PA14")
gpio.release("PA14")

local started = tmr.millis()
tmr.start(0, 10)
while not tmr.ready(0) do
    yield()
end
tmr.stop(0)

local pwm_id = pwm.open("PA14", 1000, 20)
tmr.delay(5)
pwm.duty(pwm_id, 60)
tmr.delay(5)
pwm.close(pwm_id, "PA14")

assert(adc.channel("PA27") == 0, "adc channel")
local sample = adc.read("PA27")
assert(sample >= 0 and sample <= 4095, "adc sample")
adc.release("PA27")

local i2c_ok = i2c.write(0x7f, "", 100000)
assert(type(i2c_ok) == "boolean", "i2c")
local response = spi.xfer("PA17", "\x9f", 1000000)
assert(#response == 1, "spi")

assert(can.valid("PA26", "PA27"), "can route")
local function check_can(rate, id, extended)
    can.open(rate, true, "PA26", "PA27")
    assert(can.send(id, "\x12\x34\x56", 100, extended), "can send")
    local got_id, data, got_extended = can.recv(100)
    assert(got_id == id and data == "\x12\x34\x56" and
        got_extended == extended, "can loopback")
    can.close()
end
check_can(125000, 0x121, false)
check_can(250000, 0x221, false)
check_can(500000, 0x321, false)
check_can(1000000, 0x1abcde, true)

print("MODULAR_FULL_OK", sample, tmr.millis() - started)
