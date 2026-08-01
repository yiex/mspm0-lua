#include "board_ramp.h"

#include <string.h>

typedef struct {
    uint8_t used;
    int32_t cur;
    int32_t tgt;
    int32_t rate; /* max abs change per step; min 1 */
} ramp_slot_t;

static ramp_slot_t s_r[BOARD_RAMP_MAX];

int board_ramp_open(void)
{
    int i;
    for (i = 0; i < BOARD_RAMP_MAX; i++) {
        if (!s_r[i].used) {
            memset(&s_r[i], 0, sizeof(s_r[i]));
            s_r[i].used = 1;
            s_r[i].rate = 1;
            return i;
        }
    }
    return -1;
}

void board_ramp_close(int id)
{
    if (id >= 0 && id < BOARD_RAMP_MAX) {
        s_r[id].used = 0;
    }
}

void board_ramp_config(int id, int32_t rate)
{
    if (id < 0 || id >= BOARD_RAMP_MAX || !s_r[id].used) {
        return;
    }
    if (rate < 1) {
        rate = 1;
    }
    s_r[id].rate = rate;
}

void board_ramp_set(int id, int32_t target)
{
    if (id < 0 || id >= BOARD_RAMP_MAX || !s_r[id].used) {
        return;
    }
    s_r[id].tgt = target;
}

void board_ramp_jump(int id, int32_t value)
{
    if (id < 0 || id >= BOARD_RAMP_MAX || !s_r[id].used) {
        return;
    }
    s_r[id].cur = value;
    s_r[id].tgt = value;
}

int32_t board_ramp_step(int id)
{
    ramp_slot_t *r;
    int32_t d, a;
    if (id < 0 || id >= BOARD_RAMP_MAX || !s_r[id].used) {
        return 0;
    }
    r = &s_r[id];
    d = r->tgt - r->cur;
    if (d == 0) {
        return r->cur;
    }
    a = r->rate;
    if (d > 0) {
        r->cur += (d > a) ? a : d;
    } else {
        r->cur -= ((-d) > a) ? a : (-d);
    }
    return r->cur;
}

int32_t board_ramp_get(int id)
{
    if (id < 0 || id >= BOARD_RAMP_MAX || !s_r[id].used) {
        return 0;
    }
    return s_r[id].cur;
}

int board_ramp_done(int id)
{
    if (id < 0 || id >= BOARD_RAMP_MAX || !s_r[id].used) {
        return 1;
    }
    return s_r[id].cur == s_r[id].tgt;
}
