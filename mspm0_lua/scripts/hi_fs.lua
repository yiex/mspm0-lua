-- External Flash (LittleFS) for large tables / assets
-- Theory: W25Q capacity >> internal Flash; keep hot IQ sin table in C (180 B).
print("HI_FS_START")
if not fs.ready() then
  print("FS_NOT_READY")
  print("HI_FS_SKIP")
  return
end
print("capacity", fs.capacity())

local name = "demo_tab.bin"
local payload = bytes(1, 2, 3, 4, 5, 6, 7, 8, 9, 10)
assert(fs.write(name, payload))
assert(fs.exists(name))
local got = fs.read(name, 64)
assert(got and #got == 10)
assert(byte(got, 1) == 1 and byte(got, 10) == 10)
print("RW_OK", #got)

-- optional: remove temp demo file
fs.remove(name)
print("HI_FS_OK")
