/* Table-driven registration keeps the binding surface auditable and compact. */
#define REG_END {NULL, NULL}

static const luaL_Reg k_global_reg[] = {
    {"delay_ms", l_delay_ms}, {"millis", l_millis}, {"yield", l_yield},
    {"stopped", l_stopped}, {"bytes", l_bytes}, {"byte", l_byte}, REG_END,
};
static const luaL_Reg k_gpio_reg[] = {
    {"mode", l_gpio_mode}, {"set", l_gpio_set}, {"get", l_gpio_get},
    {"toggle", l_gpio_toggle}, {"af", l_gpio_af}, {"owner", l_gpio_owner},
    {"release", l_gpio_release}, REG_END,
};
static const luaL_Reg k_irq_reg[] = {
    {"on", l_irq_on}, {"off", l_irq_off}, {"count", l_irq_count}, REG_END,
};
static const luaL_Reg k_uart_reg[] = {
    {"write", l_uart_write}, {"read", l_uart_read},
#ifdef LUA_BINARY_ONLY
    {"open", l_uart_app_open}, {"close", l_uart_app_close},
    {"tx", l_uart_app_tx}, {"rx", l_uart_app_rx},
#endif
    REG_END,
};
#ifndef LUA_SOURCE_COMPACT
static const luaL_Reg k_adc_reg[] = {
    {"read", l_adc_read}, {"capture", l_adc_capture},
    {"channel", l_adc_channel}, REG_END,
};
static const luaL_Reg k_i2c_reg[] = {
    {"open", l_i2c_open}, {"close", l_i2c_close}, {"write", l_i2c_write},
    {"writev", l_i2c_writev}, {"read", l_i2c_read},
    {"write_read", l_i2c_write_read}, REG_END,
};
static const luaL_Reg k_spi_reg[] = {
    {"open", l_spi_open}, {"close", l_spi_close}, {"cs", l_spi_cs},
    {"xfer", l_spi_xfer}, REG_END,
};
#endif
static const luaL_Reg k_pwm_reg[] = {
    {"open", l_pwm_open}, {"duty", l_pwm_duty}, {"close", l_pwm_close},
    {"comp", l_pwm_comp}, {"comp_duty", l_pwm_comp_duty},
    {"comp_close", l_pwm_comp_close}, REG_END,
};
static const luaL_Reg k_tmr_reg[] = {
    {"every", l_tmr_every}, {"ready", l_tmr_ready}, {"stop", l_tmr_stop},
    REG_END,
};
static const luaL_Reg k_event_reg[] = {
    {"run", l_event_run}, {"poll", l_event_poll}, {"stop", l_event_stop},
    REG_END,
};
static const luaL_Reg k_task_reg[] = {
    {"spawn", l_task_spawn}, {"sleep", l_task_sleep}, {"yield", l_task_yield},
    {"cancel", l_task_cancel}, REG_END,
};
#ifdef LUA_BINARY_ONLY
static const luaL_Reg k_oled_reg[] = {
    {"open", l_oled_open}, {"close", l_oled_close}, {"clear", l_oled_clear},
    {"fill", l_oled_fill},
    {"cursor", l_oled_cursor}, {"print", l_oled_print}, {"num", l_oled_num},
    {"wave", l_oled_wave}, {"ready", l_oled_ready}, {"glyph", l_oled_glyph},
    {"glyph_clear", l_oled_glyph_clear}, {"font", l_oled_font},
    {"has", l_oled_has}, {"text", l_oled_text}, {"font16", l_oled_font16},
    {"cjk", l_oled_cjk}, {"cjk_clear", l_oled_cjk_clear},
    {"has_cjk", l_oled_has_cjk}, REG_END,
};
#endif
static const luaL_Reg k_iq_reg[] = {
    {"from", l_iq_from}, {"from_x10", l_iq_from_x10},
    {"from_x100", l_iq_from_x100}, {"to_x10", l_iq_to_x10},
    {"to_x100", l_iq_to_x100}, {"to_x1000", l_iq_to_x1000},
    {"mul", l_iq_mul}, {"div", l_iq_div}, {"sin_deg", l_iq_sin_deg},
    {"cos_deg", l_iq_cos_deg}, {"atan2_deg", l_iq_atan2_deg}, REG_END,
};
static const luaL_Reg k_pid_reg[] = {
    {"open", l_pid_open}, {"close", l_pid_close}, {"reset", l_pid_reset},
    {"tune", l_pid_tune}, {"tune_iq", l_pid_tune_iq}, {"limit", l_pid_limit},
    {"ilimit", l_pid_ilimit}, {"step", l_pid_step},
    {"cascade", l_pid_cascade}, REG_END,
};
static const luaL_Reg k_filt_reg[] = {
    {"open", l_filt_open}, {"close", l_filt_close}, {"reset", l_filt_reset},
    {"config", l_filt_config}, {"update", l_filt_update}, {"get", l_filt_get},
    REG_END,
};
static const luaL_Reg k_btn_reg[] = {
    {"open", l_btn_open}, {"close", l_btn_close}, {"scan", l_btn_scan},
    {"event", l_btn_event}, {"down", l_btn_down}, {"held_ms", l_btn_held_ms},
    REG_END,
};
static const luaL_Reg k_enc_reg[] = {
    {"open", l_enc_open}, {"close", l_enc_close}, {"set", l_enc_set},
    {"pos", l_enc_pos}, {"delta", l_enc_delta}, {"cps", l_enc_cps},
    {"poll", l_enc_poll}, REG_END,
};
static const luaL_Reg k_wdt_reg[] = {
    {"start", l_wdt_start}, {"feed", l_wdt_feed}, {"active", l_wdt_active},
    REG_END,
};
static const luaL_Reg k_ramp_reg[] = {
    {"open", l_ramp_open}, {"close", l_ramp_close}, {"config", l_ramp_config},
    {"set", l_ramp_set}, {"jump", l_ramp_jump}, {"step", l_ramp_step},
    {"get", l_ramp_get}, {"done", l_ramp_done}, REG_END,
};
static const luaL_Reg k_util_reg[] = {
    {"clamp", l_util_clamp}, {"deadzone", l_util_deadzone},
    {"map", l_util_map}, {"med3", l_util_med3}, {"slew", l_util_slew},
    {"avg", l_util_avg}, {"sign", l_util_sign}, REG_END,
};
static const luaL_Reg k_crc_reg[] = {
    {"crc8", l_crc_crc8}, {"modbus", l_crc_modbus}, REG_END,
};
static const luaL_Reg k_cap_reg[] = {
    {"open", l_cap_open}, {"close", l_cap_close}, {"period", l_cap_period},
    {"hz_x10", l_cap_hz_x10}, {"ready", l_cap_ready}, {"hits", l_cap_hits},
    REG_END,
};
static const luaL_Reg k_qei_reg[] = {
    {"open", l_qei_open}, {"close", l_qei_close}, {"set", l_qei_set},
    {"pos", l_qei_pos}, {"delta", l_qei_delta}, {"dir", l_qei_dir},
    {"active", l_qei_active}, {"stim", l_qei_stim}, REG_END,
};
static const luaL_Reg k_fs_reg[] = {
    {"ready", l_fs_ready}, {"exists", l_fs_exists}, {"read", l_fs_read},
    {"write", l_fs_write}, {"remove", l_fs_remove}, {"capacity", l_fs_size},
    REG_END,
};
static const luaL_Reg k_sys_reg[] = {
    {"mem", l_sys_mem}, {"gc", l_sys_gc}, {"resource", l_sys_resource},
    REG_END,
};

static __attribute__((noinline)) void register_module(lua_State *L, const char *name,
    const luaL_Reg *reg)
{
    lua_newtable(L);
    luaL_setfuncs(L, reg, 0);
    lua_setglobal(L, name);
}

void lua_bind_reset_runtime(lua_State *L)
{
    int i;
    for (i = 0; i < N_TMR; i++) {
        board_soft_timer_stop((unsigned)i);
        if (L && s_tmr[i].used && s_tmr[i].cb_ref != LUA_NOREF) {
            luaL_unref(L, LUA_REGISTRYINDEX, s_tmr[i].cb_ref);
        }
    }
    for (i = 0; i < N_IRQ_CB; i++) {
        if (L && s_irq_cb[i].used && s_irq_cb[i].cb_ref != LUA_NOREF) {
            luaL_unref(L, LUA_REGISTRYINDEX, s_irq_cb[i].cb_ref);
        }
    }
    board_gpio_irq_reset();
    for (i = 0; i < N_TASK; i++) {
        if (L && s_task[i].state != TASK_FREE && s_task[i].ref != LUA_NOREF) {
            luaL_unref(L, LUA_REGISTRYINDEX, s_task[i].ref);
        }
    }

    for (i = 0; i < BOARD_PID_MAX; i++) board_pid_close(i);
    for (i = 0; i < BOARD_FILT_MAX; i++) board_filt_close(i);
    for (i = 0; i < BOARD_BTN_MAX; i++) board_btn_close(i);
    for (i = 0; i < BOARD_ENC_MAX; i++) board_enc_close(i);
    for (i = 0; i < BOARD_RAMP_MAX; i++) board_ramp_close(i);
    board_cap_close();
    board_qei_close();
    board_adc_close();
    board_pwm_close_all();
    board_pwm_comp_close_all();
    memset(s_comp, 0, sizeof(s_comp));
#ifdef LUA_BINARY_ONLY
    board_oled_close();
    board_i2c1_close(&s_i2c1);
    board_spi0_close(&s_spi0);
    for (i = 0; i < 3; i++) board_uart_app_close(&s_uart_app[i]);
#endif
    board_i2c_close(&s_i2c);
    board_spi_close(&s_spi);

    board_pin_reset_app_owners();
    board_resource_reset_app();
    memset(s_pwm, 0, sizeof(s_pwm));
    memset(s_tmr, 0, sizeof(s_tmr));
    memset(s_irq_cb, 0, sizeof(s_irq_cb));
    memset(s_task, 0, sizeof(s_task));
    for (i = 0; i < N_TMR; i++) s_tmr[i].cb_ref = LUA_NOREF;
    for (i = 0; i < N_IRQ_CB; i++) s_irq_cb[i].cb_ref = LUA_NOREF;
    for (i = 0; i < N_TASK; i++) s_task[i].ref = LUA_NOREF;
    s_stop = 0;
    s_event_stop = 0;
}

void lua_bind_register(lua_State *L)
{
    lua_sethook(L, stop_hook, LUA_MASKCOUNT, 1024);
    lua_pushglobaltable(L);
    luaL_setfuncs(L, k_global_reg, 0);
    lua_pop(L, 1);

    register_module(L, "gpio", k_gpio_reg);
    register_module(L, "irq", k_irq_reg);
    register_module(L, "uart", k_uart_reg);
#ifndef LUA_SOURCE_COMPACT
    register_module(L, "adc", k_adc_reg);
    register_module(L, "i2c", k_i2c_reg);
    register_module(L, "spi", k_spi_reg);
#endif
    register_module(L, "pwm", k_pwm_reg);
    register_module(L, "tmr", k_tmr_reg);
    register_module(L, "event", k_event_reg);
    register_module(L, "task", k_task_reg);
#ifdef LUA_BINARY_ONLY
    register_module(L, "oled", k_oled_reg);
#endif
    register_module(L, "iq", k_iq_reg);
    lua_getglobal(L, "iq");
    lua_pushinteger(L, IQ16_ONE);
    lua_setfield(L, -2, "ONE");
    lua_pop(L, 1);
    register_module(L, "pid", k_pid_reg);
    register_module(L, "filt", k_filt_reg);
    register_module(L, "btn", k_btn_reg);
    register_module(L, "enc", k_enc_reg);
    register_module(L, "wdt", k_wdt_reg);
    register_module(L, "ramp", k_ramp_reg);
    register_module(L, "util", k_util_reg);
    register_module(L, "crc", k_crc_reg);
    register_module(L, "cap", k_cap_reg);
    register_module(L, "qei", k_qei_reg);
    register_module(L, "fs", k_fs_reg);
    register_module(L, "sys", k_sys_reg);
}

#undef REG_END
