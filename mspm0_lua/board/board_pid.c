#include "board_pid.h"
#include "board_iq.h"

#include <string.h>

typedef struct {
    uint8_t used;
    uint8_t mode;
    iq16_t kp, ki, kd;
    int32_t umin, umax;
    int32_t ilim; /* 0 = only out-limit anti-windup */
    /* positional */
    iq16_t integ;
    int32_t e_prev;
    /* incremental */
    int32_t e1, e2;
    iq16_t u_iq; /* high-res plant command */
    uint8_t primed;
} pid_slot_t;

static pid_slot_t s_pid[BOARD_PID_MAX];

static int32_t clamp_i32(int32_t v, int32_t lo, int32_t hi)
{
    if (v < lo) {
        return lo;
    }
    if (v > hi) {
        return hi;
    }
    return v;
}

static iq16_t err_to_iq(int32_t e)
{
    /* map int error → IQ; sat to ±32767 */
    if (e > 32767) {
        e = 32767;
    }
    if (e < -32768) {
        e = -32768;
    }
    return iq16_from_i(e);
}

int board_pid_open(int mode)
{
    int i;
    if (mode != BOARD_PID_INC) {
        mode = BOARD_PID_POS;
    }
    for (i = 0; i < BOARD_PID_MAX; i++) {
        if (!s_pid[i].used) {
            memset(&s_pid[i], 0, sizeof(s_pid[i]));
            s_pid[i].used = 1;
            s_pid[i].mode = (uint8_t)mode;
            s_pid[i].kp = IQ16_ONE;
            s_pid[i].ki = 0;
            s_pid[i].kd = 0;
            s_pid[i].umin = -32768;
            s_pid[i].umax = 32767;
            s_pid[i].u_iq = 0;
            return i;
        }
    }
    return -1;
}

void board_pid_close(int id)
{
    if (id >= 0 && id < BOARD_PID_MAX) {
        s_pid[id].used = 0;
    }
}

void board_pid_reset(int id)
{
    pid_slot_t *p;
    if (id < 0 || id >= BOARD_PID_MAX || !s_pid[id].used) {
        return;
    }
    p = &s_pid[id];
    p->integ = 0;
    p->e_prev = 0;
    p->e1 = p->e2 = 0;
    p->u_iq = 0;
    p->primed = 0;
}

void board_pid_tune(int id, int32_t kp, int32_t ki, int32_t kd)
{
    if (id < 0 || id >= BOARD_PID_MAX || !s_pid[id].used) {
        return;
    }
    s_pid[id].kp = (iq16_t)kp;
    s_pid[id].ki = (iq16_t)ki;
    s_pid[id].kd = (iq16_t)kd;
}

void board_pid_out_limit(int id, int32_t umin, int32_t umax)
{
    if (id < 0 || id >= BOARD_PID_MAX || !s_pid[id].used) {
        return;
    }
    if (umin > umax) {
        int32_t t = umin;
        umin = umax;
        umax = t;
    }
    s_pid[id].umin = umin;
    s_pid[id].umax = umax;
    s_pid[id].u_iq = iq16_from_i(clamp_i32(iq16_to_i(s_pid[id].u_iq), umin, umax));
}

void board_pid_i_limit(int id, int32_t imax_abs)
{
    if (id < 0 || id >= BOARD_PID_MAX || !s_pid[id].used) {
        return;
    }
    if (imax_abs < 0) {
        imax_abs = -imax_abs;
    }
    s_pid[id].ilim = imax_abs;
}

static int32_t step_pos(pid_slot_t *p, int32_t err, uint32_t dt_ms)
{
    iq16_t e = err_to_iq(err);
    /* dt in seconds: dt_ms/1000 → IQ */
    iq16_t dts = iq16_div(iq16_from_i((int32_t)dt_ms), iq16_from_i(1000));
    iq16_t pterm, iterm, dterm, uq;
    int32_t de, u;

    if (dts <= 0) {
        dts = iq16_from_x1000(1); /* 1 ms */
    }

    pterm = iq16_mul(p->kp, e);

    /* integral += e * dt; ki * integral */
    p->integ = (iq16_t)(p->integ + iq16_mul(e, dts));
    if (p->ilim > 0) {
        iq16_t lim = iq16_from_i(p->ilim);
        p->integ = iq16_sat(p->integ, lim);
    }
    iterm = iq16_mul(p->ki, p->integ);

    dterm = 0;
    if (p->primed) {
        de = err - p->e_prev;
        dterm = iq16_mul(p->kd, iq16_div(err_to_iq(de), dts));
    }
    p->e_prev = err;
    p->primed = 1;

    uq = (iq16_t)(pterm + iterm + dterm);
    u = iq16_to_i(uq);
    u = clamp_i32(u, p->umin, p->umax);

    /* anti-windup: undo integ step when still pushing into saturation */
    if ((u == p->umax && err > 0) || (u == p->umin && err < 0)) {
        p->integ = (iq16_t)(p->integ - iq16_mul(e, dts));
    }
    p->u_iq = iq16_from_i(u);
    return u;
}

static int32_t step_inc(pid_slot_t *p, int32_t err, uint32_t dt_ms)
{
    iq16_t dts = iq16_div(iq16_from_i((int32_t)dt_ms), iq16_from_i(1000));
    iq16_t de, dde, duq, uq;

    if (dts <= 0) {
        dts = iq16_from_x1000(1);
    }
    if (!p->primed) {
        p->e1 = err;
        p->e2 = err;
        p->primed = 1;
        p->u_iq = 0;
        return 0;
    }

    /* du = Kp*(e-e1) + Ki*e*dt + Kd*((e-e1)-(e1-e2))/dt  (all IQ) */
    de = err_to_iq(err - p->e1);
    dde = err_to_iq((err - p->e1) - (p->e1 - p->e2));
    duq = iq16_mul(p->kp, de);
    duq = (iq16_t)(duq + iq16_mul(p->ki, iq16_mul(err_to_iq(err), dts)));
    duq = (iq16_t)(duq + iq16_mul(p->kd, iq16_div(dde, dts)));

    uq = (iq16_t)(p->u_iq + duq);
    /* clamp in IQ so sub-LSB accumulation is kept */
    {
        iq16_t ulo = iq16_from_i(p->umin);
        iq16_t uhi = iq16_from_i(p->umax);
        if (uq < ulo) {
            uq = ulo;
        }
        if (uq > uhi) {
            uq = uhi;
        }
    }
    p->u_iq = uq;
    p->e2 = p->e1;
    p->e1 = err;
    return iq16_to_i(uq);
}

int32_t board_pid_step_err(int id, int32_t err, uint32_t dt_ms)
{
    pid_slot_t *p;
    if (id < 0 || id >= BOARD_PID_MAX || !s_pid[id].used) {
        return 0;
    }
    if (dt_ms < 1u) {
        dt_ms = 1u;
    }
    if (dt_ms > 1000u) {
        dt_ms = 1000u;
    }
    p = &s_pid[id];
    if (p->mode == BOARD_PID_INC) {
        return step_inc(p, err, dt_ms);
    }
    return step_pos(p, err, dt_ms);
}

int32_t board_pid_step(int id, int32_t sp, int32_t fb, uint32_t dt_ms)
{
    return board_pid_step_err(id, sp - fb, dt_ms);
}

int32_t board_pid_cascade(int outer_id, int inner_id,
    int32_t sp, int32_t fb_out, int32_t fb_in, uint32_t dt_ms)
{
    int32_t inner_sp = board_pid_step(outer_id, sp, fb_out, dt_ms);
    return board_pid_step(inner_id, inner_sp, fb_in, dt_ms);
}
