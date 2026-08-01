#include "board_can.h"

#include <string.h>
#include <ti/driverlib/driverlib.h>

#include "board_irq.h"
#include "board_pins.h"
#include "ti_msp_dl_config.h"

static uint8_t s_can_open;

static int wait_mode(uint32_t mode)
{
    uint32_t spins = 800000u;
    while (DL_MCAN_getOpMode(CANFD0) != mode) {
        if (!--spins) return -1;
    }
    return 0;
}

int board_can_open(uint32_t bitrate, int loopback)
{
    DL_MCAN_ClockConfig clk = {
        .clockSel = DL_MCAN_FCLK_HFCLK,
        .divider = DL_MCAN_FCLK_DIV_1,
    };
    DL_MCAN_InitParams init = {
        .fdMode = false,
        .brsEnable = false,
        .txpEnable = false,
        .efbi = false,
        .pxhddisable = false,
        .darEnable = false,
        .wkupReqEnable = false,
        .autoWkupEnable = false,
        .emulationEnable = true,
        .wdcPreload = 255,
        .tdcEnable = false,
    };
    DL_MCAN_ConfigParams cfg = {
        .monEnable = false,
        .asmEnable = false,
        .tsPrescalar = 15,
        .tsSelect = 0,
        .timeoutSelect = DL_MCAN_TIMEOUT_SELECT_CONT,
        .timeoutPreload = 65535,
        .timeoutCntEnable = false,
        .filterConfig = {.rrfe = 1, .rrfs = 1, .anfe = 2, .anfs = 0},
    };
    DL_MCAN_BitTimingParams timing = {
        .nomRatePrescalar = 1,
        .nomTimeSeg1 = 33,
        .nomTimeSeg2 = 4,
        .nomSynchJumpWidth = 4,
        .dataRatePrescalar = 1,
        .dataTimeSeg1 = 13,
        .dataTimeSeg2 = 4,
        .dataSynchJumpWidth = 4,
    };
    DL_MCAN_MsgRAMConfigParams ram = {
        .flssa = 0, .lss = 0, .flesa = 48, .lse = 0,
        .txStartAddr = 148, .txBufNum = 1, .txFIFOSize = 0,
        .txBufMode = 0, .txBufElemSize = DL_MCAN_ELEM_SIZE_8BYTES,
        .txEventFIFOStartAddr = 164, .txEventFIFOSize = 0,
        .txEventFIFOWaterMark = 0,
        .rxFIFO0startAddr = 172, .rxFIFO0size = 3, .rxFIFO0waterMark = 0,
        .rxFIFO0OpMode = 0,
        .rxFIFO1startAddr = 192, .rxFIFO1size = 0, .rxFIFO1waterMark = 0,
        .rxFIFO1OpMode = 0,
        .rxBufStartAddr = 208, .rxBufElemSize = DL_MCAN_ELEM_SIZE_8BYTES,
        .rxFIFO0ElemSize = DL_MCAN_ELEM_SIZE_8BYTES,
        .rxFIFO1ElemSize = DL_MCAN_ELEM_SIZE_8BYTES,
    };
    DL_MCAN_RevisionId revision;
    uint32_t ready_wait = 1600000u;
    uint32_t mem_wait = 1600000u;
    uint32_t clock_wait = 160000u;
    if (!g_hfxt_ok || (bitrate != 125000u && bitrate != 250000u &&
            bitrate != 500000u && bitrate != 1000000u)) return -1;
    timing.nomRatePrescalar = 1000000u / bitrate - 1u;
    if (board_pin_af("PA26", 6, 0) || board_pin_af("PA27", 6, 1)) return -2;
    /* MCAN's RAM/clock island needs substantially longer than the generic
     * 16-cycle peripheral startup delay when it is powered at run time. */
    DL_MCAN_disableModuleClock(CANFD0);
    DL_MCAN_disablePower(CANFD0);
    delay_cycles(g_cpuclk_hz / 1000u);
    DL_MCAN_reset(CANFD0);
    DL_MCAN_enablePower(CANFD0);
    delay_cycles(g_cpuclk_hz / 1000u);
    DL_MCAN_enableModuleClock(CANFD0);
    DL_MCAN_setClockConfig(CANFD0, &clk);
    /* board_can_close() can leave both wrapper STOPREQ and M_CAN CSR set.
     * A peripheral reset does not reliably clear that low-power handshake
     * when CAN is opened again at run time.  Release both requests before
     * waiting for message RAM initialization. */
    DL_MCAN_disableClockStopGateRequest(CANFD0);
    DL_MCAN_addClockStopRequest(CANFD0, false);
    while (!DL_MCAN_isModuleClockEnabled(CANFD0) ||
            !DL_MCAN_getControllerClockRequestStatus(CANFD0)) {
        DL_MCAN_enableModuleClock(CANFD0);
        if (!--clock_wait) return -12;
    }
    while (!DL_MCAN_isReady(DL_MCAN_INSTANCE_0)) {
        if (!--ready_wait) return -10;
    }
    if (DL_MCAN_isClockStopGateRequestEnabled(CANFD0) ||
            DL_MCAN_getClockStopAck(CANFD0)) return -11;
    DL_MCAN_getRevisionId(CANFD0, &revision);
    while (!DL_MCAN_isMemInitDone(CANFD0)) {
        if (!--mem_wait) return -3;
    }
    DL_MCAN_setOpMode(CANFD0, DL_MCAN_OPERATION_MODE_SW_INIT);
    if (wait_mode(DL_MCAN_OPERATION_MODE_SW_INIT)) return -4;
    if (DL_MCAN_init(CANFD0, &init)) return -5;
    if (DL_MCAN_config(CANFD0, &cfg)) return -6;
    if (DL_MCAN_setBitTime(CANFD0, &timing)) return -7;
    if (DL_MCAN_msgRAMConfig(CANFD0, &ram)) return -8;
    DL_MCAN_setExtIDAndMask(CANFD0, 0x1FFFFFFFu);
    if (loopback) DL_MCAN_lpbkModeEnable(CANFD0,
        DL_MCAN_LPBK_MODE_INTERNAL, true);
    DL_MCAN_setOpMode(CANFD0, DL_MCAN_OPERATION_MODE_NORMAL);
    if (wait_mode(DL_MCAN_OPERATION_MODE_NORMAL)) return -9;
    s_can_open = 1;
    return 0;
}

void board_can_close(void)
{
    if (s_can_open) {
        DL_MCAN_setOpMode(CANFD0, DL_MCAN_OPERATION_MODE_SW_INIT);
        (void)wait_mode(DL_MCAN_OPERATION_MODE_SW_INIT);
        DL_MCAN_disableModuleClock(CANFD0);
        DL_MCAN_disablePower(CANFD0);
        s_can_open = 0;
    }
}

int board_can_send(uint16_t id, const uint8_t *data, size_t n,
    uint32_t timeout_ms)
{
    DL_MCAN_TxBufElement msg;
    uint32_t start = board_millis();
    if (!s_can_open || id > 0x7FFu || n > 8u || (!data && n) ||
            (DL_MCAN_getTxBufReqPend(CANFD0) & 1u)) return -1;
    memset(&msg, 0, sizeof(msg));
    msg.id = (uint32_t)id << 18u;
    msg.dlc = (uint32_t)n;
    memcpy(msg.data, data, n);
    DL_MCAN_writeMsgRam(CANFD0, DL_MCAN_MEM_TYPE_BUF, 0, &msg);
    if (DL_MCAN_TXBufAddReq(CANFD0, 0)) return -1;
    while (DL_MCAN_getTxBufReqPend(CANFD0) & 1u) {
        if ((uint32_t)(board_millis() - start) >= timeout_ms) {
            DL_MCAN_txBufCancellationReq(CANFD0, 0);
            return -1;
        }
    }
    return (DL_MCAN_getTxBufTransmissionStatus(CANFD0) & 1u) ? 0 : -1;
}

int board_can_recv(uint16_t *id, uint8_t *data, size_t *n,
    uint32_t timeout_ms)
{
    DL_MCAN_RxFIFOStatus fs = {.num = DL_MCAN_RX_FIFO_NUM_0};
    DL_MCAN_RxBufElement msg;
    uint32_t start = board_millis();
    if (!s_can_open || !id || !data || !n) return -1;
    do {
        DL_MCAN_getRxFIFOStatus(CANFD0, &fs);
        if (fs.fillLvl) break;
    } while ((uint32_t)(board_millis() - start) < timeout_ms);
    if (!fs.fillLvl) return 0;
    DL_MCAN_readMsgRam(CANFD0, DL_MCAN_MEM_TYPE_FIFO, 0,
        DL_MCAN_RX_FIFO_NUM_0, &msg);
    DL_MCAN_writeRxFIFOAck(CANFD0, DL_MCAN_RX_FIFO_NUM_0, fs.getIdx);
    if (msg.xtd || msg.rtr) return -1;
    *id = (uint16_t)((msg.id >> 18u) & 0x7FFu);
    *n = msg.dlc <= 8u ? msg.dlc : 8u;
    memcpy(data, msg.data, *n);
    return 1;
}
