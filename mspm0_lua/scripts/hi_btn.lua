-- btn smoke: open / scan / event / down (no press expected on floating PU)
print("HI_BTN_START")

local b = btn.open("PA25", 15, 600)
local n = 0
for i = 1, 5 do
  n = n + btn.scan()
  delay_ms(5)
end
local ev = btn.event(b)
local down = btn.down(b) and 1 or 0
local held = btn.held_ms(b)
print("SCAN_EV", n, "EV", ev or "nil", "DOWN", down, "HELD", held)
-- idle pull-up → not pressed
local ok = (down == 0 and held == 0) and 1 or 0
btn.close(b)
print("HI_BTN_OK", ok)
