-- filt smoke: LP settles toward step; MA averages ramp
print("HI_FILT_START")

local lp = filt.open("lp", 64)
local y = 0
for i = 1, 40 do
  y = filt.update(lp, 1000)
end
print("LP", y)
local lp_ok = (y > 900 and y < 1001) and 1 or 0

local ma = filt.open("ma", 4)
local sum = 0
for i = 1, 4 do
  sum = filt.update(ma, i * 10)
end
-- after 10,20,30,40 window mean = 25
print("MA", sum)
local ma_ok = (sum == 25) and 1 or 0

filt.reset(lp)
local z = filt.update(lp, 50)
local reset_ok = (z == 50) and 1 or 0
local g_ok = (filt.get(lp) == z) and 1 or 0

filt.close(lp)
filt.close(ma)
print("HI_FILT_OK", lp_ok, ma_ok, reset_ok, g_ok)
