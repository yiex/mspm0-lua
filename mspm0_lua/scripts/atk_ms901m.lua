-- ATK-MS601/MS901M UART protocol (shared).
-- Upload frame: 55 55 ID LEN DATA... SUM
-- Ack frame:    55 AF ID LEN DATA... SUM
local M = {}

M.ID_ATT = 0x01
M.ID_GYRO_ACCE = 0x03

local HEX = {
  [0] = "0", "1", "2", "3", "4", "5", "6", "7",
  "8", "9", "A", "B", "C", "D", "E", "F"
}

local function i16le(lo, hi)
  local v = lo + hi * 256
  if v >= 32768 then
    v = v - 65536
  end
  return v
end

function M.open(id, tx, rx, baud)
  id = id or 2
  uart.open(id, tx or "PA23", rx or "PA24", baud or 115200)
  M.id = id
  M.q = {}
  M.qi = 1
  M.qn = 0
end

function M.close()
  if M.id then
    uart.close(M.id)
    M.id = nil
  end
end

local function q_push(s)
  if not s then
    return
  end
  local i = 1
  while true do
    local b = byte(s, i)
    if not b then
      break
    end
    M.qn = M.qn + 1
    M.q[M.qn] = b
    i = i + 1
  end
end

local function q_get(off)
  return M.q[M.qi + off - 1]
end

local function q_drop(n)
  M.qi = M.qi + n
  if M.qi > 64 then
    local nq = {}
    local j = 1
    local k = M.qi
    while k <= M.qn do
      nq[j] = M.q[k]
      j = j + 1
      k = k + 1
    end
    M.q = nq
    M.qn = j - 1
    M.qi = 1
  end
end

local function q_avail()
  return M.qn - M.qi + 1
end

-- Returns id, payload-table or nil
function M.poll_frame(timeout_ms)
  local t0 = millis()
  timeout_ms = timeout_ms or 20
  while true do
    local chunk = uart.rx(M.id, 5, 64)
    if chunk then
      q_push(chunk)
    end
    while q_avail() >= 4 do
      if q_get(1) ~= 0x55 then
        q_drop(1)
      elseif q_get(2) ~= 0x55 and q_get(2) ~= 0xAF then
        q_drop(1)
      else
        local id = q_get(3)
        local len = q_get(4)
        if len > 28 then
          q_drop(1)
        elseif q_avail() < 5 + len then
          break
        else
          local sum = 0x55 + q_get(2) + id + len
          local payload = {}
          local i = 1
          while i <= len do
            local b = q_get(4 + i)
            payload[i] = b
            sum = sum + b
            i = i + 1
          end
          sum = sum % 256
          local got = q_get(5 + len)
          q_drop(5 + len)
          if got == sum then
            return id, payload
          end
        end
      end
    end
    if (millis() - t0) >= timeout_ms then
      return nil
    end
    if stopped() then
      return nil
    end
    yield()
  end
end

function M.decode_attitude(p)
  if not p or not p[6] then
    return nil
  end
  return {
    roll = i16le(p[1], p[2]) * 180 / 32768,
    pitch = i16le(p[3], p[4]) * 180 / 32768,
    yaw = i16le(p[5], p[6]) * 180 / 32768,
  }
end

function M.decode_gyro_acce(p)
  if not p or not p[12] then
    return nil
  end
  local gx = i16le(p[1], p[2])
  local gy = i16le(p[3], p[4])
  local gz = i16le(p[5], p[6])
  local ax = i16le(p[7], p[8])
  local ay = i16le(p[9], p[10])
  local az = i16le(p[11], p[12])
  return {
    gx = gx, gy = gy, gz = gz,
    ax = ax, ay = ay, az = az,
    gxd = gx * 2000 / 32768,
    gyd = gy * 2000 / 32768,
    gzd = gz * 2000 / 32768,
    axg = ax * 16 / 32768,
    ayg = ay * 16 / 32768,
    azg = az * 16 / 32768,
  }
end

function M.hex_payload(p)
  if not p then
    return ""
  end
  local s = ""
  local i = 1
  while p[i] do
    local b = p[i]
    local hi = (b - (b % 16)) / 16
    local lo = b % 16
    if i > 1 then
      s = s .. " "
    end
    s = s .. HEX[hi] .. HEX[lo]
    i = i + 1
  end
  return s
end

function M.payload_len(p)
  local n = 0
  while p and p[n + 1] do
    n = n + 1
  end
  return n
end

return M
