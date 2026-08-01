import sys, time, serial
if len(sys.argv) < 2:
    raise SystemExit("usage: upload_clean_main.py <serial-port>")
PORT = sys.argv[1]
SCRIPT = "print(123)\nprint(millis())\ngpio.mode(\"PA14\", \"out\")\ngpio.set(\"PA14\", 1)\ndelay_ms(50)\ngpio.set(\"PA14\", 0)\nprint(456)\n"
s = serial.Serial(PORT, 115200, timeout=0.3)
s.dtr = False; s.rts = False
s.reset_input_buffer(); time.sleep(0.2)
s.write(b"<<<LUA\r\n")
for line in SCRIPT.strip().split("\n"):
    s.write(line.encode("ascii") + b"\r\n"); time.sleep(0.05)
s.write(b">>>LUA\r\n"); time.sleep(2.0)
print(s.read(4096).decode("utf-8", errors="replace"))
s.write(b"r\r\n"); time.sleep(1.2)
print("run:", s.read(2048).decode("utf-8", errors="replace"))
s.close()
