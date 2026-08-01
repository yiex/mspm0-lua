-- PID smoke: positional + incremental + cascade (plant = first-order lag stub)
print("HI_PID_START")

local function plant_step(x, u, a)
  -- x += a*(u-x); a in 0..100 percent per step
  return x + ((u - x) * a) // 100
end

-- positional
-- plant tracks u directly enough that PI reaches sp
local p = pid.open("pos")
pid.tune(p, 100, 300, 0)  -- strong Ki for lag plant
pid.limit(p, -200, 200)
pid.ilimit(p, 400)
pid.reset(p)

local x, sp, dt = 0, 50, 20
for i = 1, 100 do
  x = plant_step(x, pid.step(p, sp, x, dt), 50)
end
print("POS", x)
local pos_ok = (x > 42 and x < 58) and 1 or 0

local q = pid.open("inc")
pid.tune(q, 80, 250, 0)
pid.limit(q, -200, 200)
pid.reset(q)
x = 0
for i = 1, 100 do
  x = plant_step(x, pid.step(q, sp, x, dt), 50)
end
print("INC", x)
local inc_ok = (x > 42 and x < 58) and 1 or 0

local out = pid.open("pos")
local inn = pid.open("pos")
pid.tune(out, 60, 150, 0)
pid.tune(inn, 80, 200, 0)
pid.limit(out, -120, 120)
pid.limit(inn, -200, 200)
pid.reset(out)
pid.reset(inn)
x = 0
local v = 0
for i = 1, 120 do
  local u = pid.cascade(out, inn, sp, x, v, dt)
  v = plant_step(v, u, 60)
  x = plant_step(x, v, 50)
end
print("CAS", x)
local cas_ok = (x > 40 and x < 60) and 1 or 0

pid.close(p)
pid.close(q)
pid.close(out)
pid.close(inn)
print("HI_PID_OK", pos_ok, inc_ok, cas_ok)
