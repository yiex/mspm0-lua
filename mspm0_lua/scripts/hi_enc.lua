-- enc smoke: open dual IRQ pins, pos/delta/set without turning shaft
print("HI_ENC_START")

local e = enc.open("PA25", "PA26")
local p0 = enc.pos(e)
local d0 = enc.delta(e)
enc.set(e, 100)
local p1 = enc.pos(e)
local d1 = enc.delta(e)
enc.poll()
local c0 = enc.cps(e)
delay_ms(20)
local c1 = enc.cps(e)
print("P0", p0, "D0", d0, "P1", p1, "D1", d1, "C", c0, c1)
local ok = (p0 == 0 and d0 == 0 and p1 == 100 and d1 == 0) and 1 or 0
enc.close(e)
print("HI_ENC_OK", ok)
