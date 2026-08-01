local function check(id, tx, rx, bits, parity, stop)
    assert(uart.valid(id, tx, rx), "uart route")
    uart.open(id, tx, rx, 115200, bits, parity, stop)
    uart.close(id)
end
check(1, "PA17", "PA18", 7, "even", 1)
check(2, "PA23", "PA24", 8, "none", 1)
check(3, "PA26", "PA25", 6, "odd", 2)
print("DIAG_UART_SECONDARY_OK")
