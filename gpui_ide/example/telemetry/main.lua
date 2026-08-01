-- Run, then choose "视图 -> 打开数据可视化" in the IDE.
local tick = 0
local started = millis()
local function fixed(value, scale, width)
  local sign = ""
  if value < 0 then sign = "-"; value = -value end
  local whole = value // scale
  local fraction = value % scale
  local digits = "" .. fraction
  while #digits < width do digits = "0" .. digits end
  return sign .. whole .. "." .. digits
end
while not stopped() and millis() - started < 5000 do
  local phase = tick % 200
  local wave = phase < 100 and phase or 200 - phase
  local pitch_phase = (tick + 50) % 160
  local pitch_wave = pitch_phase < 80 and pitch_phase or 160 - pitch_phase
  local roll_x10 = (wave - 50) * 8
  local pitch_x10 = (pitch_wave - 40) * 5
  local yaw = (tick * 2) % 360
  local gx_x1000 = roll_x10 * 12
  local gy_x1000 = pitch_x10 * 12
  local ax_x1000 = roll_x10
  local ay_x1000 = pitch_x10

  print('roll:' .. fixed(roll_x10, 10, 1) ..
    ',pitch:' .. fixed(pitch_x10, 10, 1) .. ',yaw:' .. yaw ..
    ',gx:' .. fixed(gx_x1000, 1000, 3) ..
    ',gy:' .. fixed(gy_x1000, 1000, 3) .. ',gz:2' ..
    ',ax:' .. fixed(ax_x1000, 1000, 3) ..
    ',ay:' .. fixed(ay_x1000, 1000, 3) .. ',az:1')
  tick = tick + 1
  if tick % 8 == 0 then collectgarbage('step', 96) end
  delay_ms(25)
end
print('TELEMETRY_OK', tick)
