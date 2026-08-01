# MSPM0G3507 + Lua firmware (Windows / GNU Make or pure recursive make via scripts)
ROOT      := $(abspath .)
SDK       ?= $(ROOT)/third_party/mspm0_sdk
TOOLCHAIN ?= $(abspath $(ROOT)/../tools/arm-gnu-toolchain)
CC        := $(TOOLCHAIN)/bin/arm-none-eabi-gcc
OBJCOPY   := $(TOOLCHAIN)/bin/arm-none-eabi-objcopy
SIZE      := $(TOOLCHAIN)/bin/arm-none-eabi-size
BUILD     := $(ROOT)/build

NAME      := mspm0_lua
LDSCRIPT  := $(ROOT)/ld/mspm0g3507.lds

CPUFLAGS  := -mcpu=cortex-m0plus -march=armv6-m -mthumb -mfloat-abi=soft
CFLAGS    := $(CPUFLAGS) -std=c99 -Os -g -ffunction-sections -fdata-sections -Wall \
	-D__MSPM0G3507__ \
	-DLUA_32BITS \
	-DLUA_USE_C89 \
	-I$(ROOT)/app -I$(ROOT)/board -I$(ROOT)/lua_bind \
	-I$(ROOT)/third_party/lua \
	-I$(SDK)/source \
	-I$(SDK)/source/third_party/CMSIS/Core/Include

LDFLAGS   := $(CPUFLAGS) -nostartfiles -Wl,--gc-sections \
	-Wl,-T,$(LDSCRIPT) -Wl,-Map,$(BUILD)/$(NAME).map \
	-L$(SDK)/source/ti/driverlib/lib/gcc/m0p/mspm0g1x0x_g3x0x \
	-Wl,--start-group -l:driverlib.a -lc -lm -lgcc -Wl,--end-group \
	--specs=nano.specs --specs=nosys.specs

# Board / app
APP_SRCS := \
	$(ROOT)/app/main.c \
	$(ROOT)/board/ti_msp_dl_config.c \
	$(ROOT)/board/board_uart.c \
	$(ROOT)/board/board_delay.c \
	$(ROOT)/board/startup_mspm0g350x_gcc.c \
	$(ROOT)/lua_bind/lua_bind.c

# Lua core (no lua.c host, no loadlib OS, trim later)
LUA_SRCS := \
	lapi.c lauxlib.c lbaselib.c lcode.c lctype.c ldebug.c ldo.c ldump.c \
	lfunc.c lgc.c llex.c lmem.c lobject.c lopcodes.c lparser.c lstate.c \
	lstring.c lstrlib.c ltable.c ltablib.c ltm.c lundump.c lvm.c lzio.c \
	lmathlib.c linit.c

LUA_SRCS := $(addprefix $(ROOT)/third_party/lua/,$(LUA_SRCS))

SRCS := $(APP_SRCS) $(LUA_SRCS)
OBJS := $(addprefix $(BUILD)/,$(notdir $(SRCS:.c=.o)))

vpath %.c $(ROOT)/app $(ROOT)/board $(ROOT)/lua_bind $(ROOT)/third_party/lua

.PHONY: all clean size flash

all: $(BUILD)/$(NAME).elf $(BUILD)/$(NAME).bin size

$(BUILD):
	@mkdir -p $(BUILD)

$(BUILD)/%.o: %.c | $(BUILD)
	@echo CC $<
	@$(CC) $(CFLAGS) -c $< -o $@

$(BUILD)/$(NAME).elf: $(OBJS)
	@echo LD $@
	@$(CC) $(OBJS) $(LDFLAGS) -o $@

$(BUILD)/$(NAME).bin: $(BUILD)/$(NAME).elf
	@$(OBJCOPY) -O binary $< $@

size: $(BUILD)/$(NAME).elf
	@$(SIZE) $<

clean:
	@rm -rf $(BUILD)

flash: $(BUILD)/$(NAME).bin
	@python $(ROOT)/../tools/jlink_flash.py $(BUILD)/$(NAME).bin
