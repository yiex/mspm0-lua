assert(uart.valid(0, "PA0", "PA1"), "uart0 route")
uart.open(0, "PA0", "PA1", 115200, 8, "none", 1)
uart.close(0)
print("DIAG_UART0_OK")
