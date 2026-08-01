gpio.mode("PA14", "out", 0)
gpio.set("PA14", 1)
gpio.release("PA14")

local sample = adc.read("PA27", 64, 4, 12)
assert(sample >= 0 and sample <= 4095, "ADC")

dac.open(12, 0, 1)
dac.write(1234)
assert(dac.write_mv(1650, 3300) >= 2046, "DAC")
dac.close()

comp.open(0, "PA26", "PA27", 1, 0, 0)
assert(type(comp.read(0)) == "boolean", "COMP")
comp.close(0)

opa.open(0, 1, 4, 2, 0, 0, 0, 1, 0)
assert(opa.ready(0), "OPA")
opa.close(0)

rtc.open()
rtc.set(2026, 7, 27, 1, 1, 2, 3)
local year, month, day, dow, hour, minute, second = rtc.get()
assert(year == 2026 and month == 7 and day == 27 and dow == 1 and
    hour == 1 and minute == 2 and second >= 3, "RTC")
rtc.close()

assert(crc.crc16("123456789") == 0x4B37, "CRC16")
assert(crc.crc32("123456789") == -873187034, "CRC32")
assert(i2c.probe_on(1, "PA15", "PA16", 0x3c, 400000), "I2C after DAC")

print("AUDIT_EXTENDED_OK", sample, year, second)
