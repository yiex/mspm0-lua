-- Wire: PA14->PA26 (A), PA25->PA27 (B)
print("HI_QEI_START")
gpio.release("PA14"); gpio.release("PA25")
gpio.release("PA26"); gpio.release("PA27")

-- 1) prove wires with GPIO irq (poll count, no event.run)
gpio.mode("PA14", "out"); gpio.set("PA14", 0)
gpio.mode("PA25", "out"); gpio.set("PA25", 0)
irq.on("PA26", "both", nil, 0)
irq.on("PA27", "both", nil, 0)
for i = 1, 10 do
  gpio.toggle("PA14"); delay_ms(2)
  gpio.toggle("PA25"); delay_ms(2)
end
local e26 = irq.count("PA26")
local e27 = irq.count("PA27")
irq.off("PA26"); irq.off("PA27")
print("WIRE_EDGES", e26, e27)

gpio.release("PA14"); gpio.release("PA25")
gpio.release("PA26"); gpio.release("PA27")

-- 2) hardware QEI
qei.open()
qei.set(0)
qei.stim(40, 1, 150)
local p1 = qei.pos()
print("FWD", p1, "DIR", qei.dir())
qei.set(0)
qei.stim(40, -1, 150)
local p2 = qei.pos()
print("REV", p2, "DIR", qei.dir())

local ok = 0
if e26 > 5 and e27 > 5 then
  if (p1 > 5 and p2 < -5) or (p1 < -5 and p2 > 5) then ok = 1 end
end
qei.close()
print("HI_QEI_OK", ok, "wire", e26, e27, "p", p1, p2)
