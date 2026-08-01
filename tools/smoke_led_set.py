import serial, time, sys
sys.stdout.reconfigure(encoding="utf-8", errors="replace")
if len(sys.argv) < 2:`r`n    raise SystemExit("usage: smoke script <serial-port>")`r`nport = sys.argv[1]`r`ns = serial.Serial(port, 115200, timeout=0.3)
s.dtr = False
s.rts = False
s.reset_input_buffer()
time.sleep(0.3)
# force visible slow blink with set 0/1, longer delays
lines = [
    "print('LED_TEST')",
    "gpio.mode('PA14','out')",
    "for i=1,4 do",
    "  gpio.set('PA14',1)",
    "  print('on',i,millis())",
    "  delay_ms(400)",
    "  gpio.set('PA14',0)",
    "  print('off',i,millis())",
    "  delay_ms(400)",
    "end",
    "print('done',millis())",
]
s.write(b"<<<LUA\r\n")
time.sleep(0.12)
print("begin", s.read(128))
for line in lines:
    s.write(line.encode("ascii") + b"\r\n")
    time.sleep(0.05)
s.write(b">>>LUA\r\n")
time.sleep(5.0)
print(s.read(4096).decode("utf-8", errors="replace"))
s.close()
