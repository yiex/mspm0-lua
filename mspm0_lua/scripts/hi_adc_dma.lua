-- High-speed ADC DMA burst (ch0..7), prints min/max/avg and period.
local ch = 0
local n = 64
local raw, pns = adc.capture(ch, n)
if not raw then
  print("adc.capture fail")
  return
end
local samples = #raw // 2
local function u16(i)
  local lo = byte(raw, i * 2 - 1)
  local hi = byte(raw, i * 2)
  return lo + hi * 256
end
local mn, mx, sum = 4095, 0, 0
for i = 1, samples do
  local v = u16(i)
  if v < mn then mn = v end
  if v > mx then mx = v end
  sum = sum + v
end
print("n", samples, "min", mn, "max", mx, "avg", sum // samples, "pns", pns or 0)
