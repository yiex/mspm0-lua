-- IQ16 self-test (SDK iqmath_ops_test spirit; compact C, no soft float)
print("HI_IQ_START")
local fail = 0
local function chk(name, got, exp, tol)
  local d = got - exp
  if d < 0 then d = -d end
  if d > tol then
    print("FAIL", name, "got", got, "exp", exp)
    fail = fail + 1
  else
    print("OK", name, got)
  end
end

-- 1.0 + 2.5 = 3.5  (×100)
local a = iq.from_x10(10)
local b = iq.from_x10(25)
local c = a + b  -- IQ values are plain int32; + is valid on same Q
chk("add", iq.to_x100(c), 350, 1)

-- 2.5 * 1.5 = 3.75
c = iq.mul(b, iq.from_x10(15))
chk("mul", iq.to_x100(c), 375, 1)

-- 3.75 / 2.5 = 1.5
c = iq.div(c, b)
chk("div", iq.to_x100(c), 150, 1)

-- sin(45°)=0.7071 → ×1000 ≈ 707
c = iq.sin_deg(450)
chk("sin45", iq.to_x1000(c), 707, 8)

-- cos(0)=1
c = iq.cos_deg(0)
chk("cos0", iq.to_x1000(c), 1000, 2)

-- sin^2+cos^2 ≈ 1 at 30°
local s = iq.sin_deg(300)
local co = iq.cos_deg(300)
local one = iq.mul(s, s) + iq.mul(co, co)
chk("ident30", iq.to_x1000(one), 1000, 15)

-- atan2 / projection-style use of °×10
local ang = iq.atan2_deg(iq.from(1), iq.from(1))
chk("atan2_45", ang, 450, 15)
local r = iq.from_x10(100)
local sy = iq.sin_deg(300)
print("proj_x10", iq.to_x10(iq.mul(r, sy)))

if fail == 0 then print("HI_IQ_OK") else print("HI_IQ_FAIL", fail) end
