#include "board_adc.h"
#include "board_irq.h"
#include "board_pins.h"
#include "board_resource.h"
#include "ti_msp_dl_config.h"

#define ADC_DMA_CH 0u

static uint8_t s_ready;
static uint8_t s_hs_ready;
static volatile uint8_t s_dma_done;
static uint32_t s_period_ns;

static const uint32_t k_ch[] = {
    DL_ADC12_INPUT_CHAN_0, DL_ADC12_INPUT_CHAN_1, DL_ADC12_INPUT_CHAN_2,
    DL_ADC12_INPUT_CHAN_3, DL_ADC12_INPUT_CHAN_4, DL_ADC12_INPUT_CHAN_5,
    DL_ADC12_INPUT_CHAN_6, DL_ADC12_INPUT_CHAN_7,
};

void ADC0_IRQHandler(void)
{
    switch (DL_ADC12_getPendingInterrupt(ADC0)) {
        case DL_ADC12_IIDX_DMA_DONE:
            s_dma_done = 1;
            break;
        default:
            break;
    }
}

static void adc_stop_dma(void)
{
    DL_ADC12_disableConversions(ADC0);
    DL_DMA_disableChannel(DMA, ADC_DMA_CH);
    DL_ADC12_disableDMA(ADC0);
    DL_ADC12_disableFIFO(ADC0);
    DL_ADC12_disableInterrupt(ADC0, DL_ADC12_INTERRUPT_DMA_DONE);
    NVIC_DisableIRQ(ADC0_INT_IRQn);
}

void board_adc_init(void)
{
    DL_ADC12_ClockConfig clk = {
        .clockSel = DL_ADC12_CLOCK_ULPCLK,
        .divideRatio = DL_ADC12_CLOCK_DIVIDE_8,
        .freqRange = DL_ADC12_CLOCK_FREQ_RANGE_24_TO_32,
    };
    adc_stop_dma();
    DL_ADC12_reset(ADC0);
    DL_ADC12_enablePower(ADC0);
    delay_cycles(POWER_STARTUP_DELAY);
    DL_ADC12_setClockConfig(ADC0, &clk);
    DL_ADC12_setPowerDownMode(ADC0, DL_ADC12_POWER_DOWN_MODE_MANUAL);
    DL_ADC12_setSampleTime0(ADC0, 500);
    DL_ADC12_enableConversions(ADC0);
    s_ready = 1;
    s_hs_ready = 0;
    /* do not clear s_period_ns: capture leaves last period for Lua */
}

void board_adc_close(void)
{
    adc_stop_dma();
    DL_ADC12_disableConversions(ADC0);
    DL_ADC12_reset(ADC0);
    s_ready = 0;
    s_hs_ready = 0;
    board_pin_release_owner(PIN_OWN_ADC);
    board_resource_release(BOARD_RES_DMA0, PIN_OWN_ADC);
    board_resource_release(BOARD_RES_ADC0, PIN_OWN_ADC);
}

/* ch 0..7 → header pin (ch4 unused on this board) */
static const char *const k_ch_pin[8] = {
    "PA27", "PA26", "PA25", "PA24", NULL, "PB24", "PB20", "PA22",
};

static int adc_setup_ch_pin(uint8_t ch)
{
    const char *pin;
    if (ch >= 8u) {
        return -1;
    }
    pin = k_ch_pin[ch];
    if (!pin) {
        return 0;
    }
    if (board_pin_claim(pin, PIN_OWN_ADC, 0) != 0) {
        return -2;
    }
    if (board_pin_af(pin, 0, 0) != 0) {
        return -2;
    }
    return 0;
}

int board_adc_claim_pin(const char *name)
{
    int ch = board_adc_pin_channel(name);
    int st;
    if (ch < 0) {
        return -1;
    }
    st = board_pin_claim(name, PIN_OWN_ADC, 0);
    if (st != 0) {
        return st == -3 ? -3 : -2;
    }
    if (board_pin_af(name, 0, 0) != 0) {
        board_pin_release_owned(name, PIN_OWN_ADC);
        return -2;
    }
    return ch;
}

int board_adc_read(uint8_t ch)
{
    uint32_t t0;
    int v;
    if (ch >= 8 || adc_setup_ch_pin(ch) != 0) {
        return -1;
    }
    if (board_resource_claim(BOARD_RES_ADC0, PIN_OWN_ADC) != 0) {
        board_pin_release_owned(k_ch_pin[ch], PIN_OWN_ADC);
        return -1;
    }
    if (s_hs_ready || !s_ready) {
        board_adc_init();
    }
    DL_ADC12_configConversionMem(ADC0, DL_ADC12_MEM_IDX_0, k_ch[ch],
        DL_ADC12_REFERENCE_VOLTAGE_VDDA, DL_ADC12_SAMPLE_TIMER_SOURCE_SCOMP0,
        DL_ADC12_AVERAGING_MODE_DISABLED, DL_ADC12_BURN_OUT_SOURCE_DISABLED,
        DL_ADC12_TRIGGER_MODE_AUTO_NEXT, DL_ADC12_WINDOWS_COMP_MODE_DISABLED);
    DL_ADC12_enableConversions(ADC0);
    DL_ADC12_clearInterruptStatus(ADC0, DL_ADC12_INTERRUPT_MEM0_RESULT_LOADED);
    DL_ADC12_startConversion(ADC0);
    t0 = board_millis();
    while (DL_ADC12_getRawInterruptStatus(ADC0, DL_ADC12_INTERRUPT_MEM0_RESULT_LOADED) == 0) {
        if ((uint32_t)(board_millis() - t0) > 20) {
            DL_ADC12_enableConversions(ADC0);
            return -1;
        }
    }
    v = (int)DL_ADC12_getMemResult(ADC0, DL_ADC12_MEM_IDX_0);
    DL_ADC12_enableConversions(ADC0);
    return v;
}

/*
 * High-speed path: SYSOSC/1, sample time 2, FIFO+DMA, repeat mode.
 * Two 12-bit samples pack per DMA word (FIFO).
 */
int board_adc_capture(uint8_t ch, uint16_t *buf, size_t n,
    uint32_t timeout_ms, uint8_t rate)
{
    DL_ADC12_ClockConfig clk = {
        .clockSel = DL_ADC12_CLOCK_SYSOSC,
        .divideRatio = DL_ADC12_CLOCK_DIVIDE_1,
        .freqRange = DL_ADC12_CLOCK_FREQ_RANGE_24_TO_32,
    };
    DL_DMA_Config dcfg = {
        .transferMode = DL_DMA_SINGLE_TRANSFER_MODE,
        .extendedMode = DL_DMA_NORMAL_MODE,
        .destIncrement = DL_DMA_ADDR_INCREMENT,
        .srcIncrement = DL_DMA_ADDR_UNCHANGED,
        .destWidth = DL_DMA_WIDTH_WORD,
        .srcWidth = DL_DMA_WIDTH_WORD,
        .trigger = DMA_ADC0_EVT_GEN_BD_TRIG,
        .triggerType = DL_DMA_TRIGGER_TYPE_EXTERNAL,
    };
    uint32_t t0;
    uint32_t period_ns = 10438u;
    uint16_t sample_cycles = 320u;
    size_t fifo_words;
    size_t i;

    if (!buf || n < 2u || n > BOARD_ADC_DMA_MAX || ch >= 8u) {
        return -1;
    }
    if (n & 1u) {
        n--;
    }
    if (adc_setup_ch_pin(ch) != 0) {
        return -1;
    }
    if (board_resource_claim(BOARD_RES_ADC0, PIN_OWN_ADC) != 0) {
        board_pin_release_owned(k_ch_pin[ch], PIN_OWN_ADC);
        return -1;
    }
    if (board_resource_claim(BOARD_RES_DMA0, PIN_OWN_ADC) != 0) {
        board_adc_close();
        return -1;
    }
    if (timeout_ms == 0) {
        timeout_ms = 200;
    }
    if (rate == 0u) {
        sample_cycles = 2u;
        period_ns = 500u;
    } else if (rate >= 2u) {
        clk.clockSel = DL_ADC12_CLOCK_ULPCLK;
        clk.divideRatio = DL_ADC12_CLOCK_DIVIDE_8;
        sample_cycles = 500u;
        period_ns = 128500u;
    }
    /* even count for FIFO packing */
    fifo_words = n >> 1;

    adc_stop_dma();
    DL_ADC12_reset(ADC0);
    DL_ADC12_enablePower(ADC0);
    delay_cycles(POWER_STARTUP_DELAY);
    DL_ADC12_setClockConfig(ADC0, &clk);
    DL_ADC12_initSingleSample(ADC0, DL_ADC12_REPEAT_MODE_ENABLED,
        DL_ADC12_SAMPLING_SOURCE_AUTO, DL_ADC12_TRIG_SRC_SOFTWARE,
        DL_ADC12_SAMP_CONV_RES_12_BIT, DL_ADC12_SAMP_CONV_DATA_FORMAT_UNSIGNED);
    DL_ADC12_configConversionMem(ADC0, DL_ADC12_MEM_IDX_0, k_ch[ch],
        DL_ADC12_REFERENCE_VOLTAGE_VDDA, DL_ADC12_SAMPLE_TIMER_SOURCE_SCOMP0,
        DL_ADC12_AVERAGING_MODE_DISABLED, DL_ADC12_BURN_OUT_SOURCE_DISABLED,
        DL_ADC12_TRIGGER_MODE_AUTO_NEXT, DL_ADC12_WINDOWS_COMP_MODE_DISABLED);
    DL_ADC12_enableFIFO(ADC0);
    DL_ADC12_setPowerDownMode(ADC0, DL_ADC12_POWER_DOWN_MODE_MANUAL);
    DL_ADC12_setSampleTime0(ADC0, sample_cycles);
    DL_ADC12_enableDMA(ADC0);
    DL_ADC12_setDMASamplesCnt(ADC0, 6);
    DL_ADC12_enableDMATrigger(ADC0, DL_ADC12_DMA_MEM10_RESULT_LOADED);
    DL_ADC12_clearInterruptStatus(ADC0, DL_ADC12_INTERRUPT_DMA_DONE);
    DL_ADC12_enableInterrupt(ADC0, DL_ADC12_INTERRUPT_DMA_DONE);

    DL_DMA_initChannel(DMA, ADC_DMA_CH, &dcfg);
    DL_DMA_setSrcAddr(DMA, ADC_DMA_CH, (uint32_t)DL_ADC12_getFIFOAddress(ADC0));
    DL_DMA_setDestAddr(DMA, ADC_DMA_CH, (uint32_t)buf);
    DL_DMA_setTransferSize(DMA, ADC_DMA_CH, (uint16_t)fifo_words);
    DL_DMA_enableChannel(DMA, ADC_DMA_CH);

    s_dma_done = 0;
    s_hs_ready = 1;
    s_ready = 0;
    s_period_ns = period_ns;

    NVIC_ClearPendingIRQ(ADC0_INT_IRQn);
    NVIC_EnableIRQ(ADC0_INT_IRQn);
    DL_ADC12_enableConversions(ADC0);
    DL_ADC12_startConversion(ADC0);

    t0 = board_millis();
    while (!s_dma_done) {
        if ((uint32_t)(board_millis() - t0) > timeout_ms) {
            adc_stop_dma();
            board_adc_init();
            return -1;
        }
    }

    adc_stop_dma();
    {
        uint32_t keep = s_period_ns;
        board_adc_init();
        s_period_ns = keep;
    }

    /* FIFO packs two 12-bit samples per word; unpack if driver packed as LE halfwords */
    for (i = 0; i < n; i++) {
        buf[i] &= 0x0FFFu;
    }
    return (int)n;
}

uint32_t board_adc_capture_period_ns(void)
{
    return s_period_ns;
}
