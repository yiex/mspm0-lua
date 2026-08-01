import serial, time, sys
sys.stdout.reconfigure(encoding="utf-8", errors="replace")
if len(sys.argv) < 2:`r`n    raise SystemExit("usage: smoke script <serial-port>")`r`nport = sys.argv[1]`r`ns = serial.Serial(port, 115200, timeout=0.3)
s.dtr = False
s.rts = False
s.reset_input_buffer()
time.sleep(0.3)
# use single quotes in Lua to avoid quote issues
lines = [
    "print(millis())",
    "print(adc.read(0))",
    "id=pwm.open('PA14',1000)",
    "pwm.duty(id,30)",
    "print(id)",
]
s.write(b"<<<LUA\r\n")
time.sleep(0.15)
print("begin", s.read(128))
for line in lines:
    s.write(line.encode("ascii") + b"\r\n")
    time.sleep(0.06)
s.write(b">>>LUA\r\n")
time.sleep(2.0)
print(s.read(2048).decode("utf-8", errors="replace"))
s.write(b"r\r\n")
time.sleep(1.0)
print("r", s.read(1024).decode("utf-8", errors="replace"))
s.close()
