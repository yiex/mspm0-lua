#include "native_module.h"

#include "board_pins.h"
#include "board_resource.h"
#include <ti/driverlib/driverlib.h>

typedef struct {
    uint8_t active;
} rtc_state_t;

extern const native_module_header_t g_native_module_header;

static unsigned rtc_slot(void)
{
    uintptr_t address = (uintptr_t)&g_native_module_header;
    if (address < NATIVE_MODULE_SLOT_ADDR ||
            address >= NATIVE_MODULE_SLOT_ADDR +
                NATIVE_MODULE_SLOT_COUNT * NATIVE_MODULE_SLOT_SIZE) {
        return NATIVE_MODULE_SLOT_COUNT;
    }
    return (unsigned)((address - NATIVE_MODULE_SLOT_ADDR) /
        NATIVE_MODULE_SLOT_SIZE);
}

static rtc_state_t *rtc_state(void)
{
    return (rtc_state_t *)NATIVE_CORE_API->module_state(
        rtc_slot(), sizeof(rtc_state_t));
}

static int l_rtc_open(lua_State *L)
{
    rtc_state_t *state = rtc_state();
    (void)L;
    if (!state) return NATIVE_CORE_API->raise_error(L, "rtc:state");
    if (state->active) return 0;
    if (NATIVE_CORE_API->resource_claim(BOARD_RES_RTC, PIN_OWN_RTC) != 0) {
        return NATIVE_CORE_API->raise_error(L, "rtc:busy");
    }
    DL_RTC_enablePower(RTC);
    delay_cycles(16);
    DL_RTC_enableClockControl(RTC);
    state->active = 1;
    return 0;
}

static unsigned month_days(unsigned year, unsigned month)
{
    static const uint8_t days[12] = {
        31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31,
    };
    unsigned result = days[month - 1u];
    if (month == 2u && ((year % 4u == 0u && year % 100u != 0u) ||
            year % 400u == 0u)) result++;
    return result;
}

static int l_rtc_set(lua_State *L)
{
    rtc_state_t *state = rtc_state();
    int32_t year = NATIVE_CORE_API->check_integer(L, 1);
    int32_t month = NATIVE_CORE_API->check_integer(L, 2);
    int32_t day = NATIVE_CORE_API->check_integer(L, 3);
    int32_t dow = NATIVE_CORE_API->check_integer(L, 4);
    int32_t hour = NATIVE_CORE_API->check_integer(L, 5);
    int32_t minute = NATIVE_CORE_API->check_integer(L, 6);
    int32_t second = NATIVE_CORE_API->check_integer(L, 7);
    DL_RTC_Common_Calendar calendar;
    if (!state || !state->active) {
        return NATIVE_CORE_API->raise_error(L, "rtc:closed");
    }
    if (year < 0 || year > 4095 || month < 1 || month > 12 ||
            day < 1 || day > (int32_t)month_days((unsigned)year, (unsigned)month) ||
            dow < 0 || dow > 6 || hour < 0 || hour > 23 ||
            minute < 0 || minute > 59 || second < 0 || second > 59) {
        return NATIVE_CORE_API->raise_error(L, "rtc:date");
    }
    calendar.year = (uint16_t)year;
    calendar.month = (uint8_t)month;
    calendar.dayOfMonth = (uint8_t)day;
    calendar.dayOfWeek = (uint8_t)dow;
    calendar.hours = (uint8_t)hour;
    calendar.minutes = (uint8_t)minute;
    calendar.seconds = (uint8_t)second;
    DL_RTC_initCalendar(RTC, calendar, DL_RTC_FORMAT_BINARY);
    return 0;
}

static int l_rtc_get(lua_State *L)
{
    rtc_state_t *state = rtc_state();
    uint32_t started;
    DL_RTC_Common_Calendar calendar;
    if (!state || !state->active) {
        return NATIVE_CORE_API->raise_error(L, "rtc:closed");
    }
    started = NATIVE_CORE_API->millis();
    while (!DL_RTC_isSafeToRead(RTC)) {
        if ((uint32_t)(NATIVE_CORE_API->millis() - started) >= 20u) {
            return NATIVE_CORE_API->raise_error(L, "rtc:timeout");
        }
    }
    calendar = DL_RTC_getCalendarTime(RTC);
    NATIVE_CORE_API->push_integer(L, calendar.year);
    NATIVE_CORE_API->push_integer(L, calendar.month);
    NATIVE_CORE_API->push_integer(L, calendar.dayOfMonth);
    NATIVE_CORE_API->push_integer(L, calendar.dayOfWeek);
    NATIVE_CORE_API->push_integer(L, calendar.hours);
    NATIVE_CORE_API->push_integer(L, calendar.minutes);
    NATIVE_CORE_API->push_integer(L, calendar.seconds);
    return 7;
}

static int l_rtc_close(lua_State *L)
{
    rtc_state_t *state = rtc_state();
    (void)L;
    if (state && state->active) {
        DL_RTC_disableClockControl(RTC);
        DL_RTC_disablePower(RTC);
        NATIVE_CORE_API->resource_release(BOARD_RES_RTC, PIN_OWN_RTC);
        state->active = 0;
    }
    return 0;
}

static const native_lua_reg_t k_rtc_functions[] = {
    {"open", l_rtc_open}, {"set", l_rtc_set},
    {"get", l_rtc_get}, {"close", l_rtc_close}, {0, 0},
};

static int rtc_init(lua_State *L, const native_core_api_t *api)
{
    rtc_state_t *state;
    if (!api || api->magic != NATIVE_CORE_API_MAGIC ||
            api->abi_version != NATIVE_CORE_ABI_VERSION ||
            api->struct_size < sizeof(native_core_api_t)) return -1;
    state = (rtc_state_t *)api->module_state(rtc_slot(), sizeof(*state));
    if (!state) return -1;
    state->active = 0;
    return api->register_lua_module(L, "rtc", k_rtc_functions);
}

static void rtc_deinit(void)
{
    rtc_state_t *state = rtc_state();
    if (state && state->active) {
        DL_RTC_disableClockControl(RTC);
        DL_RTC_disablePower(RTC);
        NATIVE_CORE_API->resource_release(BOARD_RES_RTC, PIN_OWN_RTC);
        state->active = 0;
    }
}

const native_module_header_t g_native_module_header
    __attribute__((section(".module_header"), used, aligned(4))) = {
        NATIVE_MODULE_MAGIC, NATIVE_MODULE_FORMAT, NATIVE_CORE_ABI_VERSION,
        0, 0, sizeof(native_module_header_t), rtc_init, rtc_deinit, "rtc",
    };
