-- Triggered OLED scope on PA27 / ADC0 channel 0.
-- DMA rates: 0~2MSPS, 1~96kSPS, 2~7.8kSPS. The script selects one once.
local ch, n = "PA27", 256
local bins = {}
for i = 1, 16 do bins[i] = 0 end

local function u16(raw, i)
  local o = (i - 1) * 2
  return byte(raw, o + 1) + byte(raw, o + 2) * 256
end

local function estimate(raw, pns)
  local count = #raw // 2
  if count < 16 or not pns or pns == 0 then return 0 end
  for i = 1, 16 do bins[i] = 0 end
  for i = 1, count do
    local v = u16(raw, i)
    local b = v // 256 + 1
    bins[b] = bins[b] + 1
  end
  local trim, acc = count // 16, 0
  local lb, hb = 1, 16
  for i = 1, 16 do
    acc = acc + bins[i]
    if acc > trim then lb = i; break end
  end
  acc = 0
  for i = 16, 1, -1 do
    acc = acc + bins[i]
    if acc > trim then hb = i; break end
  end
  local low, high = (lb - 1) * 256, hb * 256 - 1
  local span = high - low
  if span < 384 then return 0 end
  local mid = (low + high) // 2
  local h = span // 6
  if h < 32 then h = 32 end
  local lo, hi = mid - h, mid + h
  local armed = u16(raw, 1) <= lo
  local last, periods, ticks = 0, 0, 0
  for i = 2, count do
    local v = u16(raw, i)
    if v <= lo then
      armed = true
    elseif armed and v >= hi then
      if last > 0 then
        local dt = i - last
        if dt >= 4 then periods = periods + 1; ticks = ticks + dt end
      end
      last, armed = i, false
    end
  end
  if periods == 0 then return 0 end
  local sps = 1000000000 // pns
  local hz = (sps * periods + ticks // 2) // ticks
  if hz > sps // 4 or hz > 500000 then return 0 end
  return hz
end

local function capture(rate)
  local raw, pns = adc.capture(ch, n, 200, rate)
  return raw, pns, raw and estimate(raw, pns) or 0
end

local function select_rate()
  for rate = 0, 2 do
    local raw, pns, hz = capture(rate)
    if hz > 0 then return rate, raw, pns, hz end
  end
  local raw, pns = adc.capture(ch, n, 200, 2)
  return 2, raw, pns, 0
end

local rate, raw, pns, hz = select_rate()
local candidate, agrees, shown, missing, painted = 0, 0, 0, 0, -1

local function render(samples, value, repaint)
  if not oled.ready() then oled.open() end
  oled.wave(samples)
  if repaint then
    oled.cursor(80, 7)
    oled.print("        ")
    oled.num(80, 7, value, 0)
  end
end

pcall(oled.open)

while not stopped() do
  if not raw then raw, pns, hz = capture(rate) end

  if hz > 0 then
    local tol = candidate // 20
    if tol < 2 then tol = 2 end
    local d = hz - candidate
    if d < 0 then d = -d end
    if candidate > 0 and d <= tol then
      agrees = agrees + 1
    else
      candidate, agrees = hz, 1
    end
    if agrees >= 3 then
      shown = shown == 0 and candidate or (shown * 3 + candidate + 2) // 4
      missing = 0
    end
  else
    missing = missing + 1
    if missing >= 6 then shown = 0 end
    if missing >= 32 then
      rate, raw, pns, hz = select_rate()
      candidate, agrees, missing = 0, 0, 0
    end
  end

  if raw then
    local repaint = shown ~= painted
    if pcall(render, raw, shown, repaint) then
      if repaint then painted = shown end
    else
      painted = -1
      pcall(oled.close)
      delay_ms(20)
      pcall(oled.open)
    end
  end
  raw, pns, hz = capture(rate)
  delay_ms(60)
end
