-- util helpers (abs/min/max are plain Lua; sign stays in util)
local ok = 1
if util.clamp(5, 0, 3) ~= 3 then ok = 0 end
if util.deadzone(2, 5) ~= 0 then ok = 0 end
if util.map(50, 0, 100, 0, 10) ~= 5 then ok = 0 end
if util.med3(1, 9, 5) ~= 5 then ok = 0 end
if util.slew(0, 10, 3) ~= 3 then ok = 0 end
if util.avg({1, 2, 3}) ~= 2 then ok = 0 end
if util.sign(-2) ~= -1 then ok = 0 end
local a = -7
if (a < 0 and -a or a) ~= 7 then ok = 0 end
print(ok == 1 and "HI_UTIL_OK" or "HI_UTIL_FAIL")
