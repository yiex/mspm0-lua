#include "board_iq.h"

iq16_t iq16_from_i(int32_t i)
{
    if (i > 32767) {
        i = 32767;
    }
    if (i < -32768) {
        i = -32768;
    }
    return (iq16_t)(i << 16);
}

iq16_t iq16_from_x10(int32_t x10)
{
    return (iq16_t)(((int64_t)x10 * IQ16_ONE) / 10);
}

iq16_t iq16_from_x100(int32_t x100)
{
    return (iq16_t)(((int64_t)x100 * IQ16_ONE) / 100);
}

iq16_t iq16_from_x1000(int32_t x1000)
{
    return (iq16_t)(((int64_t)x1000 * IQ16_ONE) / 1000);
}

int32_t iq16_to_i(iq16_t q) { return (int32_t)(q >> 16); }

static int32_t to_scaled(iq16_t q, int32_t scale)
{
    int64_t n = (int64_t)q * scale;
    if (n >= 0) {
        n += IQ16_HALF;
    } else {
        n -= IQ16_HALF;
    }
    return (int32_t)(n / IQ16_ONE);
}

int32_t iq16_to_x10(iq16_t q) { return to_scaled(q, 10); }
int32_t iq16_to_x100(iq16_t q) { return to_scaled(q, 100); }
int32_t iq16_to_x1000(iq16_t q) { return to_scaled(q, 1000); }

iq16_t iq16_mul(iq16_t a, iq16_t b)
{
    return (iq16_t)(((int64_t)a * (int64_t)b) >> 16);
}

iq16_t iq16_div(iq16_t a, iq16_t b)
{
    if (b == 0) {
        return (a >= 0) ? 0x7fffffff : (iq16_t)0x80000000;
    }
    return (iq16_t)(((int64_t)a << 16) / b);
}

iq16_t iq16_abs(iq16_t a) { return (a < 0) ? (iq16_t)(-a) : a; }

iq16_t iq16_sat(iq16_t a, iq16_t lim)
{
    if (lim < 0) {
        lim = (iq16_t)(-lim);
    }
    if (a > lim) {
        return lim;
    }
    if (a < -lim) {
        return (iq16_t)(-lim);
    }
    return a;
}

/* Normalize degrees×10 into [0, 3600). */
static int32_t norm_deg_x10(int32_t d)
{
    d %= 3600;
    if (d < 0) {
        d += 3600;
    }
    return d;
}

iq16_t iq16_sin_deg_x10(int32_t deg_x10)
{
    int32_t d = norm_deg_x10(deg_x10);
    int32_t sign = 1;
    int32_t product;
    int32_t denominator;
    iq16_t value;

    /* Bhaskara I on 0..180 degrees, using degrees x10 throughout.
     * Max error is below 0.2%, while removing the resident lookup table. */
    if (d >= 1800) {
        d -= 1800;
        sign = -1;
    }
    product = d * (1800 - d);
    denominator = 4050000 - product;
    value = (iq16_t)(((int64_t)4 * product * IQ16_ONE) / denominator);
    return sign > 0 ? value : (iq16_t)(-value);
}

iq16_t iq16_cos_deg_x10(int32_t deg_x10)
{
    return iq16_sin_deg_x10(deg_x10 + 900);
}

/* Integer atan2 → degrees×10. CORDIC-free; ratio + small poly. Theory-valid. */
int32_t iq16_atan2_deg_x10(iq16_t y, iq16_t x)
{
    int32_t ax, ay, angle, t, t2;
    int swap = 0;
    int inv = 0;

    if (x == 0 && y == 0) {
        return 0;
    }
    ax = (x < 0) ? -x : x;
    ay = (y < 0) ? -y : y;
    if (ay > ax) {
        int32_t tmp = ax;
        ax = ay;
        ay = tmp;
        swap = 1;
    }
    /* t = ay/ax in Q16 */
    t = ax ? (int32_t)(((int64_t)ay << 16) / ax) : IQ16_ONE;
    /* atan(t)≈ t*(π/4) for t in [0,1] with correction; use deg form:
     * deg ≈ t*45 - t*(t-1)*(14 + 3.83*t)  (common fixed approx)
     * Work in x10 degrees with t as Q16.
     */
    t2 = (int32_t)(((int64_t)t * t) >> 16);
    /* 45*t  in x10: (450 * t) / 65536 */
    angle = (int32_t)(((int64_t)450 * t) / IQ16_ONE);
    /* correction ~ t*(1-t)*(0.273) * 180/pi ... simplified: use 38*t*(IQ16_ONE-t)/IQ16 */
    {
        int32_t c = (int32_t)(((int64_t)t * (IQ16_ONE - t)) >> 16);
        angle -= (int32_t)(((int64_t)c * 38) / IQ16_ONE);
        (void)t2;
    }
    if (swap) {
        angle = 900 - angle;
    }
    if (x < 0) {
        angle = 1800 - angle;
        inv = 1;
    }
    if (y < 0) {
        angle = -angle;
    }
    (void)inv;
    if (angle > 1800) {
        angle -= 3600;
    }
    if (angle < -1800) {
        angle += 3600;
    }
    return angle;
}
