-- Finite MPU6050 I2C diagnostic for LKDMX / MSPM0G3507.
-- PB2 = I2C1_SCL, PB3 = I2C1_SDA. This tests both possible MPU6050 addresses.

local BUS = 1
local SCL = "PB2"
local SDA = "PB3"
local WHO_AM_I = 0x75
local ADDRESSES = { 0x68, 0x69 }
local RATES = { 100000, 40000 }

local function read_who_am_i(address, hz)
  local ok, data = pcall(
    i2c.write_read_on, BUS, SCL, SDA, address, i2c.bytes(WHO_AM_I), 1, hz
  )
  if not ok then return nil, "xfer-error" end
  if not data or #data ~= 1 then return nil, "short-read" end
  return byte(data, 1), "ok"
end

local function wake(address, hz)
  local ok, result = pcall(
    i2c.write_on, BUS, SCL, SDA, address, i2c.bytes(0x6B, 0x00), hz
  )
  return ok and result == true
end

local function raw_read(address, hz)
  local ok, data = pcall(i2c.read_on, BUS, SCL, SDA, address, 1, hz)
  return ok and data and #data == 1
end

local function read_motion_frame(address, hz)
  local ok, data = pcall(
    i2c.write_read_on, BUS, SCL, SDA, address, i2c.bytes(0x3B), 14, hz
  )
  if not ok or not data or #data ~= 14 then return nil end
  return data
end

print("HI_MPU_DIAG_START")
print("I2C_ROUTE", i2c.valid(BUS, SCL, SDA) and "VALID" or "INVALID")
print("I2C_RECOVER", i2c.recover(BUS, SCL, SDA) and "READY" or "LINES_LOW_OR_BUSY")

for _, hz in ipairs(RATES) do
  for _, address in ipairs(ADDRESSES) do
    local probe = i2c.probe_on(BUS, SCL, SDA, address, hz)
    local wrote = wake(address, hz)
    local read = raw_read(address, hz)
    local who, status = read_who_am_i(address, hz)
    if who then
      print("MPU_RESULT", hz, address, "PROBE", probe,
        "WRITE", wrote, "READ", read, "WHO_AM_I", who)
      if address == 0x68 and hz == 100000 then
        local frame = read_motion_frame(address, hz)
        if frame then
          print("MPU_FRAME", #frame, byte(frame, 1), byte(frame, 2),
            byte(frame, 13), byte(frame, 14))
        else
          print("MPU_FRAME", "xfer-error")
        end
      end
    else
      print("MPU_RESULT", hz, address, "PROBE", probe,
        "WRITE", wrote, "READ", read, "WHO_AM_I", status)
    end
  end
end

print("HI_MPU_DIAG_DONE")
