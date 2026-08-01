-- wdt smoke: arm + feed only (does not wait for reset)
print("HI_WDT_START")
local a0 = wdt.active() and 1 or 0
wdt.start(500)
local a1 = wdt.active() and 1 or 0
for i = 1, 5 do
  wdt.feed()
  delay_ms(50)
end
print("HI_WDT_OK", (a0 == 0 and a1 == 1) and 1 or 0)
-- keep feeding until script ends; next script re-register does not stop WWDT
