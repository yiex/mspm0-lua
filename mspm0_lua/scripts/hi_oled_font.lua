-- Digits built-in; letters need bank or _run.fnt (IDE uploads on Run).
oled.open()
oled.clear()
oled.cursor(0, 0)
oled.print("12.34")          -- built-in only
oled.cursor(0, 2)
-- If IDE uploaded _run.fnt with A-Z, this shows; else blanks for letters:
oled.print("R")
oled.num(18, 2, -1234, 1)
print("has R", oled.has(0x52) and 1 or 0)
print("font", oled.font() or "none")
