#include "board_filt.h"

#include <string.h>

typedef struct {
    uint8_t used;
    uint8_t kind;
    uint8_t primed;
    uint16_t param; /* LP alpha 1..256; MA window */
    int32_t y;
    int32_t sum;
    uint8_t wi;
    int32_t ring[BOARD_FILT_MA_MAX];
} filt_slot_t;

static filt_slot_t s_f[BOARD_FILT_MAX];

int board_filt_open(int kind)
{
    int i;
    if (kind != BOARD_FILT_MA) {
        kind = BOARD_FILT_LP;
    }
    for (i = 0; i < BOARD_FILT_MAX; i++) {
        if (!s_f[i].used) {
            memset(&s_f[i], 0, sizeof(s_f[i]));
            s_f[i].used = 1;
            s_f[i].kind = (uint8_t)kind;
            s_f[i].param = (kind == BOARD_FILT_MA) ? 8u : 64u;
            return i;
        }
    }
    return -1;
}

void board_filt_close(int id)
{
    if (id >= 0 && id < BOARD_FILT_MAX) {
        s_f[id].used = 0;
    }
}

void board_filt_reset(int id)
{
    if (id < 0 || id >= BOARD_FILT_MAX || !s_f[id].used) {
        return;
    }
    s_f[id].primed = 0;
    s_f[id].y = 0;
    s_f[id].sum = 0;
    s_f[id].wi = 0;
    memset(s_f[id].ring, 0, sizeof(s_f[id].ring));
}

void board_filt_config(int id, int param)
{
    filt_slot_t *f;
    if (id < 0 || id >= BOARD_FILT_MAX || !s_f[id].used) {
        return;
    }
    f = &s_f[id];
    if (f->kind == BOARD_FILT_LP) {
        if (param < 1) {
            param = 1;
        }
        if (param > 256) {
            param = 256;
        }
        f->param = (uint16_t)param;
    } else {
        if (param < 2) {
            param = 2;
        }
        if (param > BOARD_FILT_MA_MAX) {
            param = BOARD_FILT_MA_MAX;
        }
        f->param = (uint16_t)param;
        f->primed = 0;
        f->sum = 0;
        f->wi = 0;
        memset(f->ring, 0, sizeof(f->ring));
    }
}

int32_t board_filt_update(int id, int32_t x)
{
    filt_slot_t *f;
    if (id < 0 || id >= BOARD_FILT_MAX || !s_f[id].used) {
        return x;
    }
    f = &s_f[id];
    if (f->kind == BOARD_FILT_LP) {
        if (!f->primed) {
            f->y = x;
            f->primed = 1;
            return x;
        }
        /* y += a*(x-y)/256 */
        f->y += (int32_t)(((int64_t)(x - f->y) * (int32_t)f->param) / 256);
        return f->y;
    }
    /* moving average */
    {
        uint16_t n = f->param;
        if (!f->primed) {
            uint16_t i;
            for (i = 0; i < n; i++) {
                f->ring[i] = x;
            }
            f->sum = (int32_t)x * (int32_t)n;
            f->wi = 0;
            f->primed = 1;
            f->y = x;
            return x;
        }
        f->sum -= f->ring[f->wi];
        f->ring[f->wi] = x;
        f->sum += x;
        f->wi++;
        if (f->wi >= n) {
            f->wi = 0;
        }
        f->y = f->sum / (int32_t)n;
        return f->y;
    }
}

int32_t board_filt_get(int id)
{
    if (id < 0 || id >= BOARD_FILT_MAX || !s_f[id].used) {
        return 0;
    }
    return s_f[id].y;
}
