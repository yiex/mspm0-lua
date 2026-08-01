assert(can.valid("PA26", "PA27"), "can route")
local function check(rate, id, extended)
    can.open(rate, true, "PA26", "PA27")
    assert(can.send(id, "\x12\x34\x56", 100, extended), "can send")
    local got_id, data, got_extended = can.recv(100)
    assert(got_id == id and data == "\x12\x34\x56" and
        got_extended == extended, "can loopback")
    can.close()
end
check(125000, 0x121, false)
check(250000, 0x221, false)
check(500000, 0x321, false)
check(1000000, 0x1abcde, true)
print("DIAG_CAN_OK")
