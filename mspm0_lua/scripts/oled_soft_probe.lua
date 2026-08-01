-- Fast soft-I2C probe: normal + swap, only 0x3C/0x3D
print("SOFT_PROBE")
local function delay() local i=1; while i<400 do i=i+1 end end
local function probe(SCL, SDA)
  local function scl_h() gpio.mode(SCL,"in") end
  local function scl_l() gpio.mode(SCL,"out"); gpio.set(SCL,0) end
  local function sda_h() gpio.mode(SDA,"in") end
  local function sda_l() gpio.mode(SDA,"out"); gpio.set(SDA,0) end
  local function sda_rd() gpio.mode(SDA,"in"); return gpio.get(SDA) end
  local function start()
    sda_h(); scl_h(); delay(); sda_l(); delay(); scl_l(); delay()
  end
  local function stop()
    sda_l(); delay(); scl_h(); delay(); sda_h(); delay()
  end
  local function write_byte(v)
    local m=128
    while m>0 do
      if (v//m)%2==1 then sda_h() else sda_l() end
      delay(); scl_h(); delay(); scl_l(); delay()
      m=m//2
    end
    sda_h(); delay(); scl_h(); delay()
    local ack=sda_rd()
    scl_l(); delay()
    return ack==0 and 1 or 0
  end
  gpio.mode(SCL,"in"); gpio.mode(SDA,"in")
  print("idle", SCL, SDA, gpio.get(SCL), gpio.get(SDA))
  for _,a in ipairs({0x3c,0x3d,0x27}) do
    start()
    local ack=write_byte(a*2)
    stop()
    print("ACK", SCL, SDA, a, ack)
  end
end
probe("PA15","PA16")
probe("PA16","PA15")
print("SOFT_PROBE_DONE")
