#include "native_module.h"

#include "board_pins.h"
#include "board_reg.h"
#include <ti/driverlib/driverlib.h>

#define PIN_OWNER_GPIO PIN_OWN_GPIO

static int streq(const char *a, const char *b)
{
    while (*a && *a == *b) {
        a++;
        b++;
    }
    return *a == *b;
}

static int get_pin(lua_State *L, int index, const char **name,
    native_pin_t *pin)
{
    *name = NATIVE_CORE_API->check_string(L, index);
    if (NATIVE_CORE_API->pin_resolve(*name, pin) != 0) {
        return NATIVE_CORE_API->raise_error(L, "gpio:pin");
    }
    return 0;
}

static int claim_pin(lua_State *L, const char *name)
{
    if (NATIVE_CORE_API->pin_claim(name, PIN_OWNER_GPIO) != 0) {
        return NATIVE_CORE_API->raise_error(L, "gpio:busy");
    }
    return 0;
}

static int l_gpio_mode(lua_State *L)
{
    const char *name;
    const char *mode;
    native_pin_t pin;
    GPIO_Regs *port;
    int32_t option;
    unsigned invert;
    mode = NATIVE_CORE_API->opt_string(L, 2, "out");
    if (get_pin(L, 1, &name, &pin) != 0) return -1;
    if (!streq(mode, "out") && !streq(mode, "od") &&
            !streq(mode, "analog") && !streq(mode, "in") &&
            !streq(mode, "in_pu") && !streq(mode, "in_pd")) {
        return NATIVE_CORE_API->raise_error(L, "gpio:mode");
    }
    if (claim_pin(L, name) != 0) return -1;
    port = (GPIO_Regs *)pin.port;
    if (streq(mode, "out")) {
        int32_t initial = NATIVE_CORE_API->opt_integer(L, 3, 0);
        unsigned high_drive =
            (unsigned)NATIVE_CORE_API->opt_integer(L, 4, 0);
        invert = (unsigned)NATIVE_CORE_API->opt_integer(L, 5, 0);
        DL_GPIO_initDigitalOutputFeatures(pin.iomux,
            invert ? DL_GPIO_INVERSION_ENABLE : DL_GPIO_INVERSION_DISABLE,
            DL_GPIO_RESISTOR_NONE,
            high_drive ? DL_GPIO_DRIVE_STRENGTH_HIGH
                       : DL_GPIO_DRIVE_STRENGTH_LOW,
            DL_GPIO_HIZ_DISABLE);
        board_reg_gpio_write(port, pin.pin, initial != 0);
        port->DOESET31_0 = pin.pin;
    } else if (streq(mode, "od")) {
        int32_t initial = NATIVE_CORE_API->opt_integer(L, 3, 1);
        DL_GPIO_initDigitalOutputFeatures(pin.iomux,
            DL_GPIO_INVERSION_DISABLE, DL_GPIO_RESISTOR_PULL_UP,
            DL_GPIO_DRIVE_STRENGTH_LOW, DL_GPIO_HIZ_DISABLE);
        board_reg_gpio_clr(port, pin.pin);
        if (initial) port->DOECLR31_0 = pin.pin;
        else port->DOESET31_0 = pin.pin;
    } else if (streq(mode, "analog")) {
        port->DOECLR31_0 = pin.pin;
        DL_GPIO_initPeripheralAnalogFunction(pin.iomux);
    } else {
        DL_GPIO_RESISTOR pull = DL_GPIO_RESISTOR_NONE;
        unsigned hysteresis;
        port->DOECLR31_0 = pin.pin;
        option = NATIVE_CORE_API->opt_integer(L, 3, 0);
        if (streq(mode, "in_pu") || option > 0) {
            pull = DL_GPIO_RESISTOR_PULL_UP;
        } else if (streq(mode, "in_pd") || option < 0) {
            pull = DL_GPIO_RESISTOR_PULL_DOWN;
        }
        hysteresis = (unsigned)NATIVE_CORE_API->opt_integer(L, 4, 0);
        invert = (unsigned)NATIVE_CORE_API->opt_integer(L, 5, 0);
        DL_GPIO_initDigitalInputFeatures(pin.iomux,
            invert ? DL_GPIO_INVERSION_ENABLE : DL_GPIO_INVERSION_DISABLE,
            pull, hysteresis ? DL_GPIO_HYSTERESIS_ENABLE
                             : DL_GPIO_HYSTERESIS_DISABLE,
            DL_GPIO_WAKEUP_DISABLE);
    }
    return 0;
}

static int ensure_output(lua_State *L, const char *name,
    const native_pin_t *pin)
{
    GPIO_Regs *port = (GPIO_Regs *)pin->port;
    if (claim_pin(L, name) != 0) return -1;
    if ((port->DOE31_0 & pin->pin) == 0u) {
        board_reg_pin_out(port, pin->pin, pin->iomux);
    }
    return 0;
}

static int l_gpio_set(lua_State *L)
{
    const char *name;
    native_pin_t pin;
    if (get_pin(L, 1, &name, &pin) != 0 ||
            ensure_output(L, name, &pin) != 0) return -1;
    board_reg_gpio_write((GPIO_Regs *)pin.port, pin.pin,
        NATIVE_CORE_API->check_integer(L, 2) != 0);
    return 0;
}

static int l_gpio_od_write(lua_State *L)
{
    const char *name;
    native_pin_t pin;
    GPIO_Regs *port;
    if (get_pin(L, 1, &name, &pin) != 0 || claim_pin(L, name) != 0) return -1;
    port = (GPIO_Regs *)pin.port;
    board_reg_gpio_clr(port, pin.pin);
    if (NATIVE_CORE_API->check_integer(L, 2)) port->DOECLR31_0 = pin.pin;
    else port->DOESET31_0 = pin.pin;
    return 0;
}

static int l_gpio_get(lua_State *L)
{
    const char *name;
    native_pin_t pin;
    if (get_pin(L, 1, &name, &pin) != 0) return -1;
    NATIVE_CORE_API->push_integer(L,
        board_reg_gpio_read((GPIO_Regs *)pin.port, pin.pin) ? 1 : 0);
    return 1;
}

static int l_gpio_toggle(lua_State *L)
{
    const char *name;
    native_pin_t pin;
    if (get_pin(L, 1, &name, &pin) != 0 ||
            ensure_output(L, name, &pin) != 0) return -1;
    board_reg_gpio_tog((GPIO_Regs *)pin.port, pin.pin);
    return 0;
}

static int l_gpio_af(lua_State *L)
{
    const char *name;
    native_pin_t pin;
    int32_t pf;
    int32_t input;
    pf = NATIVE_CORE_API->check_integer(L, 2);
    input = NATIVE_CORE_API->opt_integer(L, 3, 0);
    if (get_pin(L, 1, &name, &pin) != 0) return -1;
    if (pf < 0 || pf > 9) {
        return NATIVE_CORE_API->raise_error(L, "gpio:af");
    }
    if (claim_pin(L, name) != 0) return -1;
    if (NATIVE_CORE_API->pin_af(name, (unsigned)pf, input != 0) != 0) {
        NATIVE_CORE_API->pin_release(name, PIN_OWNER_GPIO);
        return NATIVE_CORE_API->raise_error(L, "gpio:af");
    }
    return 0;
}

static int l_gpio_release(lua_State *L)
{
    const char *name;
    native_pin_t pin;
    if (get_pin(L, 1, &name, &pin) != 0) return -1;
    if (NATIVE_CORE_API->pin_owner(name) != PIN_OWNER_GPIO) {
        return NATIVE_CORE_API->raise_error(L, "gpio:owner");
    }
    ((GPIO_Regs *)pin.port)->DOECLR31_0 = pin.pin;
    DL_GPIO_initDigitalInput(pin.iomux);
    NATIVE_CORE_API->pin_release(name, PIN_OWNER_GPIO);
    return 0;
}

static int l_gpio_owner(lua_State *L)
{
    NATIVE_CORE_API->push_integer(L,
        NATIVE_CORE_API->pin_owner(NATIVE_CORE_API->check_string(L, 1)));
    return 1;
}

static int l_gpio_policy(lua_State *L)
{
    NATIVE_CORE_API->push_integer(L, (int32_t)NATIVE_CORE_API->pin_policy(
        NATIVE_CORE_API->check_string(L, 1)));
    return 1;
}

static int l_gpio_valid(lua_State *L)
{
    native_pin_t pin;
    NATIVE_CORE_API->push_boolean(L, NATIVE_CORE_API->pin_resolve(
        NATIVE_CORE_API->check_string(L, 1), &pin) == 0);
    return 1;
}

static const native_lua_reg_t k_gpio_functions[] = {
    {"mode", l_gpio_mode}, {"set", l_gpio_set}, {"write", l_gpio_set},
    {"od_write", l_gpio_od_write}, {"get", l_gpio_get},
    {"read", l_gpio_get}, {"toggle", l_gpio_toggle}, {"af", l_gpio_af},
    {"release", l_gpio_release}, {"owner", l_gpio_owner},
    {"policy", l_gpio_policy}, {"valid", l_gpio_valid}, {0, 0},
};

static int gpio_init(lua_State *L, const native_core_api_t *api)
{
    if (!api || api->magic != NATIVE_CORE_API_MAGIC ||
            api->abi_version != NATIVE_CORE_ABI_VERSION ||
            api->struct_size < sizeof(native_core_api_t)) return -1;
    return api->register_lua_module(L, "gpio", k_gpio_functions);
}

const native_module_header_t g_native_module_header
    __attribute__((section(".module_header"), used, aligned(4))) = {
        NATIVE_MODULE_MAGIC, NATIVE_MODULE_FORMAT, NATIVE_CORE_ABI_VERSION,
        0, 0, sizeof(native_module_header_t), gpio_init, 0, "gpio",
    };
