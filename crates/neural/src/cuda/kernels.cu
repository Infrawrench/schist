// Schist FP32 kernels. No CUDA runtime or device-library dependencies.
// Regenerate kernels.ptx with `make neural-cuda-ptx` (LLVM 21).
// Dimensions/strides are u32, scalar coefficients are IEEE-754 bit patterns.
#ifdef SCHIST_CUDA_HOST
#include <algorithm>
#include <cmath>
#define D inline
#define K
using std::ceil;
using std::exp;
using std::fabs;
using std::floor;
using std::log;
using std::pow;
using std::sqrt;
using std::trunc;
D float bits(unsigned x) { return __builtin_bit_cast(float, x); }
D void mark(const float *const *xs) { *const_cast<float *>(xs[15]) = 1; }
#else
#define D __attribute__((device)) __attribute__((always_inline)) inline
#define K __attribute__((global))
D float bits(unsigned x) { return __builtin_bit_cast(float, x); }
D float sqrt(float x) {
  float y;
  asm("sqrt.rn.f32 %0, %1;" : "=f"(y) : "f"(x));
  return y;
}
D float exp(float x) {
  float y;
  x *= 1.4426950408889634f;
  asm("ex2.approx.ftz.f32 %0, %1;" : "=f"(y) : "f"(x));
  return y;
}
D float log(float x) {
  float y;
  asm("lg2.approx.ftz.f32 %0, %1;" : "=f"(y) : "f"(x));
  return y * 0.6931471805599453f;
}
D float floor(float x) {
  float y;
  asm("cvt.rmi.f32.f32 %0, %1;" : "=f"(y) : "f"(x));
  return y;
}
D float ceil(float x) {
  float y;
  asm("cvt.rpi.f32.f32 %0, %1;" : "=f"(y) : "f"(x));
  return y;
}
D float trunc(float x) {
  float y;
  asm("cvt.rzi.f32.f32 %0, %1;" : "=f"(y) : "f"(x));
  return y;
}
D float fabs(float x) { return __builtin_fabsf(x); }
D float pow(float x, float y) {
  if (y == 0)
    return 1;
  if (x == 0 && y > 0)
    return 0;
  return exp(log(x) * y);
}
D unsigned lane() {
  unsigned x;
  asm("mov.u32 %0, %%tid.x;" : "=r"(x));
  return x;
}
D unsigned group() {
  unsigned x;
  asm("mov.u32 %0, %%ctaid.x;" : "=r"(x));
  return x;
}
D void barrier() { asm volatile("bar.sync 0;" ::: "memory"); }
D void mark(const float *const *xs) {
  unsigned old;
  asm volatile("atom.global.exch.b32 %0, [%1], 1065353216;"
               : "=r"(old)
               : "l"(xs[15])
               : "memory");
}
#endif
D unsigned min(unsigned a, unsigned b) { return a < b ? a : b; }
D int min(int a, int b) { return a < b ? a : b; }
D int max(int a, int b) { return a > b ? a : b; }
D float min(float a, float b) { return a < b ? a : b; }
D float max(float a, float b) { return a > b ? a : b; }
D float clamp(float x, float lo, float hi) { return min(max(x, lo), hi); }
D float unary(float x, unsigned op, const unsigned *p) {
  switch (op) {
  case 0:
    return x;
  case 1:
    return max(x, 0.f);
  case 2:
    return x < 0 ? x * bits(p[2]) : x;
  case 3:
    return 1.f / (1.f + exp(-x));
  case 4: {
    float t = exp(-2.f * fabs(x));
    float v = (1.f - t) / (1.f + t);
    return x < 0 ? -v : v;
  }
  case 5:
    return sqrt(x);
  case 6:
    return exp(x);
  case 7:
    return log(x);
  case 8:
    return fabs(x);
  case 9:
    return -x;
  case 10:
    return 1.f / x;
  case 11: {
    float t = 1.f / (1.f + 0.3275911f * fabs(x));
    float v =
        1.f - (((((1.061405429f * t - 1.453152027f) * t + 1.421413741f) * t -
                 0.284496736f) *
                    t +
                0.254829592f) *
               t) *
                  exp(-x * x);
    return x < 0 ? -v : v;
  }
  case 12:
    return floor(x);
  case 13:
    return ceil(x);
  case 14:
    return clamp(x, bits(p[2]), bits(p[3]));
  case 15:
    return x * clamp(x / 6.f + 0.5f, 0.f, 1.f);
  case 16:
    return clamp(bits(p[2]) * x + bits(p[3]), 0.f, 1.f);
  case 17:
    return trunc(x);
  case 18:
    return 1.f / sqrt(x);
  case 19:
    return x * x;
  default:
    return bits(0x7fc00000);
  }
}
D float binary(float a, float b, unsigned op) {
  switch (op) {
  case 0:
    return a + b;
  case 1:
    return a - b;
  case 2:
    return a * b;
  case 3:
    return a / b;
  case 4: {
    float v = pow(fabs(a), b);
    if (a < 0) {
      if (b != trunc(b))
        return bits(0x7fc00000);
      if (fabs(b) - 2.f * floor(fabs(b) / 2.f) == 1.f)
        v = -v;
    }
    return v;
  }
  case 5:
    return min(a, b);
  case 6:
    return max(a, b);
  case 7:
    return a < 0 ? a * b : a;
  default:
    return bits(0x7fc00000);
  }
}
D unsigned offset(unsigned index, const unsigned *axes, unsigned rank,
                  unsigned width, unsigned column) {
  unsigned out = 0;
  for (int k = int(rank) - 1; k >= 0; --k) {
    const unsigned *a = axes + k * width;
    out += (index % a[0]) * a[column];
    index /= a[0];
  }
  return out;
}
D float at(const float *src, int y, int x, unsigned h, unsigned w,
           unsigned plane) {
  y = max(0, min(y, int(h) - 1));
  x = max(0, min(x, int(w) - 1));
  return src[(plane * h + unsigned(y)) * w + unsigned(x)];
}
D float coordinate(unsigned i, unsigned mode, unsigned n, unsigned out,
                   float scale) {
  if (mode == 0)
    return float(i) / scale;
  if (mode == 2 && out == 1)
    return 0;
  if (mode == 3)
    return out > 1 ? float(i) * float(n - 1) / float(out - 1) : 0;
  if (mode == 4)
    return (float(i) + 0.5f) / scale;
  return (float(i) + 0.5f) / scale - 0.5f;
}
D float cubic_weight(float s, float a) {
  s = fabs(s);
  if (s <= 1.f)
    return (a + 2.f) * s * s * s - (a + 3.f) * s * s + 1.f;
  if (s <= 2.f)
    return a * s * s * s - 5.f * a * s * s + 8.f * a * s - 4.f * a;
  return 0;
}
D float reduce_identity(unsigned kind) {
  return kind == 2 ? -3.402823466e38f : kind == 3 ? 3.402823466e38f : 0;
}
D float reduce_combine(float a, float b, unsigned kind) {
  return kind == 2 ? max(a, b) : kind == 3 ? min(a, b) : a + b;
}
// Same indexing and bounded partials in the host oracle and GPU reduction.
D float reduce_lane(const float *a, const unsigned *p, unsigned i,
                    unsigned lane, unsigned step) {
  const unsigned *axes = p + 4;
  unsigned start = 0, end = p[3];
  if (p[0] == 14) {
    const unsigned *chunk = axes + p[2] * 3;
    start = (i % chunk[1]) * chunk[0];
    end = min(end, start + chunk[0]);
    i /= chunk[1];
  }
  unsigned base = 0;
  for (int k = int(p[2]) - 1; k >= 0; --k)
    if (!axes[k * 3 + 2]) {
      base += (i % axes[k * 3]) * axes[k * 3 + 1];
      i /= axes[k * 3];
    }
  float sum = reduce_identity(p[1]);
  for (unsigned j = start + lane; j < end; j += step) {
    unsigned k = j, idx = base;
    for (int d = int(p[2]) - 1; d >= 0; --d)
      if (axes[d * 3 + 2]) {
        idx += (k % axes[d * 3]) * axes[d * 3 + 1];
        k /= axes[d * 3];
      }
    sum = reduce_combine(sum, a[idx], p[1]);
  }
  return sum;
}
D void element(const float *const *xs, float *dst, const unsigned *p,
               unsigned i) {
  const float *a = xs[0];
  const float *b = xs[1];
  switch (p[0]) {
  case 0:
    dst[i] = unary(a[i], p[1], p);
    break;
  case 1: {
    float aa = a[offset(i, p + 3, p[2], 3, 1)],
          bb = b[offset(i, p + 3, p[2], 3, 2)];
    if (p[1] < 8) {
      dst[i] = binary(aa, bb, p[1]);
      break;
    }
    // Runtime integer arithmetic uses i64, with exact-float storage only
    // after checking range. Never round a pixel offset or divide by zero.
    if (!(fabs(aa) <= 16777216.f && fabs(bb) <= 16777216.f && aa == trunc(aa) &&
          bb == trunc(bb)) ||
        (p[1] == 11 && bb == 0)) {
      mark(xs);
      dst[i] = 0;
      break;
    }
    long long x = static_cast<long long>(aa), y = static_cast<long long>(bb),
              v = 0;
    switch (p[1]) {
    case 8:
      v = x + y;
      break;
    case 9:
      v = x - y;
      break;
    case 10:
      v = x * y;
      break;
    case 11:
      v = x / y;
      break;
    case 12:
      v = x < y ? x : y;
      break;
    case 13:
      v = x > y ? x : y;
      break;
    }
    if (v < -16777216 || v > 16777216) {
      mark(xs);
      dst[i] = 0;
    } else
      dst[i] = float(v);
    break;
  }
  case 2: { // arbitrary strided copy, including slices, transposes and
            // broadcasts
    unsigned rem = i;
    long long index = 0;
    for (int k = int(p[1]) - 1; k >= 0; --k) {
      const unsigned *q = p + 2 + k * 4;
      index +=
          (static_cast<long long>(rem % q[0]) * int(q[3]) + int(q[2])) * q[1];
      rem /= q[0];
    }
    dst[i] = a[index];
    break;
  }
  case 3: { // grouped NCHW convolution, exact padding/dilation
    unsigned ic = p[1], ih = p[2], iw = p[3], oc = p[4], oh = p[5], ow = p[6],
             kh = p[7], kw = p[8];
    unsigned ci = ic / p[15], co = oc / p[15], x = i % ow, y = i / ow % oh,
             c = i / (ow * oh) % oc, n = i / (ow * oh * oc), g = c / co;
    float sum = xs[2][c];
    for (unsigned k = 0; k < ci; ++k)
      for (unsigned dy = 0; dy < kh; ++dy)
        for (unsigned dx = 0; dx < kw; ++dx) {
          int sy = int(y * p[9] + dy * p[11]) - int(p[13]),
              sx = int(x * p[10] + dx * p[12]) - int(p[14]);
          if (sy >= 0 && sx >= 0 && sy < int(ih) && sx < int(iw))
            sum += a[((n * ic + g * ci + k) * ih + unsigned(sy)) * iw +
                     unsigned(sx)] *
                   b[((c * ci + k) * kh + dy) * kw + dx];
        }
    dst[i] = sum;
    break;
  }
  case 4: { // arbitrary einsum; tiled matrix entry uses the same axis metadata
    unsigned ar = offset(i, p + 4, p[1], 3, 1),
             br = offset(i, p + 4, p[1], 3, 2);
    float sum = 0;
    const unsigned *red = p + 4 + p[1] * 3;
    for (unsigned k = 0; k < p[3]; ++k)
      sum += a[ar + offset(k, red, p[2], 3, 1)] *
             b[br + offset(k, red, p[2], 3, 2)];
    dst[i] = sum;
    break;
  }
  case 5:
  case 14:
    dst[i] = reduce_lane(a, p, i, 0, 1);
    break;
  case 15: { // NCHW sum/average pooling, including explicit padding.
    unsigned ih = p[1], iw = p[2], oh = p[3], ow = p[4],
             plane = i / (oh * ow), count = 0;
    float sum = 0;
    for (unsigned ky = 0; ky < p[5]; ++ky)
      for (unsigned kx = 0; kx < p[6]; ++kx) {
        int y = int(i / ow % oh * p[7] + ky * p[9]) - int(p[11]),
            x = int(i % ow * p[8] + kx * p[10]) - int(p[12]);
        if (y >= 0 && x >= 0 && y < int(ih) && x < int(iw)) {
          sum += a[(plane * ih + unsigned(y)) * iw + unsigned(x)];
          ++count;
        }
      }
    unsigned divisor = p[14] ? p[5] * p[6] : count;
    dst[i] = p[13] ? sum * (1.f / float(divisor)) : sum;
    break;
  }
  case 6: {
    unsigned count = p[1], stride = p[2],
             base = i / (count * stride) * (count * stride) + i % stride;
    float top = a[base];
    for (unsigned j = 1; j < count; ++j)
      top = max(top, a[base + j * stride]);
    float sum = 0;
    for (unsigned j = 0; j < count; ++j)
      sum += exp(a[base + j * stride] - top);
    dst[i] = exp(a[i] - top) / sum;
    break;
  }
  case 7: { // constant/edge/reflect padding
    unsigned rem = i, idx = 0;
    bool outside = false;
    for (int k = int(p[1]) - 1; k >= 0; --k) {
      const unsigned *q = p + 4 + k * 4;
      int c = int(rem % q[0]) - int(q[3]);
      rem /= q[0];
      int dim = int(q[1]);
      if (c < 0 || c >= dim) {
        outside = true;
        if (p[2] == 2 && dim > 1) {
          int period = 2 * (dim - 1);
          c = ((c % period) + period) % period;
          c = min(c, period - c);
        } else
          c = max(0, min(c, dim - 1));
      }
      idx += unsigned(c) * q[2];
    }
    dst[i] = outside && p[2] == 0 ? bits(p[3]) : a[idx];
    break;
  }
  case 8: { // concatenation as a sequence of two-input joins
    unsigned al = p[1] * p[2], bl = p[1] * p[3], outer = i / (al + bl),
             j = i % (al + bl);
    dst[i] = j < al ? a[outer * al + j] : b[outer * bl + j - al];
    break;
  }
  case 9: { // static Resize, nearest or bilinear
    unsigned iw = p[1], ih = p[2], ow = p[3], oh = p[4], plane = i / (ow * oh);
    float x = coordinate(i % ow, p[7], iw, ow, bits(p[5])),
          y = coordinate(i / ow % oh, p[7], ih, oh, bits(p[6]));
    if (p[8] == 0) {
      float xx = floor(x), yy = floor(y);
      if (p[9] == 1) {
        xx = ceil(x);
        yy = ceil(y);
      }
      if (p[9] == 2) {
        xx = ceil(x - .5f);
        yy = ceil(y - .5f);
      }
      if (p[9] == 3) {
        xx = floor(x + .5f);
        yy = floor(y + .5f);
      }
      dst[i] = at(a, int(yy), int(xx), ih, iw, plane);
    } else if (p[8] == 2) {
      // Match tract's separable axis order: vertical, then horizontal.
      int xx = int(ceil(x) - 1.f), yy = int(ceil(y) - 1.f);
      float sum = 0, coeff = bits(p[10]);
      for (int kx = -1; kx <= 2; ++kx) {
        float column = 0;
        for (int ky = -1; ky <= 2; ++ky)
          column += at(a, yy + ky, xx + kx, ih, iw, plane) *
                    cubic_weight(float(ky) - (y - float(yy)), coeff);
        sum += column * cubic_weight(float(kx) - (x - float(xx)), coeff);
      }
      dst[i] = sum;
    } else {
      int xx = int(floor(x)), yy = int(floor(y));
      float tx = x - float(xx), ty = y - float(yy);
      float top = at(a, yy, xx, ih, iw, plane) * (1 - tx) +
                  at(a, yy, xx + 1, ih, iw, plane) * tx;
      float bot = at(a, yy + 1, xx, ih, iw, plane) * (1 - tx) +
                  at(a, yy + 1, xx + 1, ih, iw, plane) * tx;
      dst[i] = top * (1 - ty) + bot * ty;
    }
    break;
  }
  case 10: { // integer conversion: only exact pixel indices, never shape
             // arithmetic
    float v = trunc(a[i]);
    if (!(v >= -16777216.f && v <= 16777216.f)) {
      mark(xs);
      v = 0;
    }
    dst[i] = v;
    break;
  }
  case 11: { // Gather with checked dynamic indices
    unsigned inner = p[1], dim = p[2], count = p[3];
    float f = b[i / inner % count];
    if (!(f >= -float(dim) && f < float(dim) && f == trunc(f))) {
      mark(xs);
      dst[i] = 0;
      break;
    }
    int index = int(f);
    if (index < 0)
      index += int(dim);
    if (!(f == trunc(f)) || index < 0 || index >= int(dim)) {
      mark(xs);
      dst[i] = 0;
    } else
      dst[i] =
          a[(i / (inner * count) * dim + unsigned(index)) * inner + i % inner];
    break;
  }
  case 12: { // checked GatherND; batch dimensions are flattened
    unsigned tail = p[1], tuples = p[2], data_batch = p[3], tuple = p[4],
             batch = i / (tail * tuples), point = i / tail % tuples;
    unsigned idx = batch * data_batch + i % tail;
    for (unsigned k = 0; k < tuple; ++k) {
      float f = b[(batch * tuples + point) * tuple + k];
      int dim = int(p[5 + k * 2]);
      if (!(f >= -float(dim) && f < float(dim) && f == trunc(f))) {
        mark(xs);
        dst[i] = 0;
        return;
      }
      int v = int(f);
      if (v < 0)
        v += dim;
      if (!(f == trunc(f)) || v < 0 || v >= dim) {
        mark(xs);
        dst[i] = 0;
        return;
      }
      idx += unsigned(v) * p[6 + k * 2];
    }
    dst[i] = a[idx];
    break;
  }
  case 13: { // fused deformable sampling, same corner order as the CPU
    unsigned channels = p[1], ih = p[2], iw = p[3], oh = p[4], ow = p[5],
             kh = p[6], kw = p[7], tuples = oh * ow * kh * kw;
    unsigned bg = i / (channels * tuples), c = i / tuples % channels,
             j = i % tuples, dy = j / (ow * kw), dx = j % (ow * kw);
    unsigned t =
        bg * tuples + ((dy % kh * kw + dx % kw) * oh + dy / kh) * ow + dx / kw;
    float sum = 0;
    for (unsigned corner = 0; corner < 4; ++corner) {
      float yf = xs[1 + corner][t * 2], xf = xs[1 + corner][t * 2 + 1];
      if (!(yf >= -float(ih) && yf < float(ih) && xf >= -float(iw) &&
            xf < float(iw) && yf == trunc(yf) && xf == trunc(xf))) {
        mark(xs);
        dst[i] = 0;
        return;
      }
      int y = int(yf), x = int(xf);
      if (y < 0)
        y += int(ih);
      if (x < 0)
        x += int(iw);
      float term =
          xs[5 + corner][t] *
          a[((bg * channels + c) * ih + unsigned(y)) * iw + unsigned(x)];
      sum = corner == 0 ? term : sum + term;
    }
    dst[i] = sum * xs[9][t];
    break;
  }
  default:
    mark(xs);
    dst[i] = 0;
  }
}
#ifndef SCHIST_CUDA_HOST
extern "C" K void tensor(const float *const *xs, float *, float *dst,
                         const unsigned *p, unsigned len) {
  unsigned i = group() * 256 + lane();
  if (i < len)
    element(xs, dst, p, i);
}
// Cooperatively stage a 16x16 matrix tile. Axis metadata permits transposed,
// batched and broadcast operands without staging through host memory.
extern "C" K void matrix(const float *const *xs, float *, float *dst,
                         const unsigned *p, unsigned len) {
  __attribute__((shared)) float a[256], b[256];
  unsigned row = lane() / 16, col = lane() % 16, m = p[1], n = p[2], k = p[3];
  unsigned nc = (n + 15) / 16, mc = (m + 15) / 16, tile = group(),
           batch = tile / (nc * mc);
  tile %= nc * mc;
  unsigned r = tile / nc * 16 + row, c = tile % nc * 16 + col;
  unsigned ra = p[4], ca = p[5], ba = p[6], ka = p[7];
  const unsigned *rows = p + 8;
  const unsigned *cols = rows + ra * 4;
  const unsigned *batches = cols + ca * 4;
  const unsigned *red = batches + ba * 4;
  const unsigned *split = red + ka * 3;
  unsigned parts = split[0], output_len = split[1],
           batches_count = output_len / (m * n), part = batch / batches_count;
  batch %= batches_count;
  unsigned chunk = parts == 1 ? k : 4096,
           start = part * chunk, end = min(k, start + chunk);
  unsigned ab = offset(batch, batches, ba, 4, 1),
           bb = offset(batch, batches, ba, 4, 2),
           ob = offset(batch, batches, ba, 4, 3);
  float sum = 0;
  for (unsigned base = start; base < end; base += 16) {
    unsigned ak = base + col, bk = base + row;
    a[lane()] =
        (r < m && ak < end)
            ? xs[0][ab + offset(r, rows, ra, 4, 1) + offset(ak, red, ka, 3, 1)]
            : 0;
    b[lane()] =
        (c < n && bk < end)
            ? xs[1][bb + offset(c, cols, ca, 4, 2) + offset(bk, red, ka, 3, 2)]
            : 0;
    barrier();
    for (unsigned j = 0; j < 16; ++j)
      sum += a[row * 16 + j] * b[j * 16 + col];
    barrier();
  }
  if (r < m && c < n)
    dst[part * output_len + ob + offset(r, rows, ra, 4, 3) + offset(c, cols, ca, 4, 3)] = sum;
}
extern "C" K void convolution(const float *const *xs, float *, float *dst,
                              const unsigned *p, unsigned len) {
  __attribute__((shared)) float weights[256], pixels[256];
  unsigned row = lane() / 16, col = lane() % 16, ic = p[1], ih = p[2],
           iw = p[3], oc = p[4], oh = p[5], ow = p[6], kh = p[7], kw = p[8];
  unsigned ci = ic / p[15], co = oc / p[15], spatial = oh * ow,
           tiles = (spatial + 15) / 16, ct = (co + 15) / 16;
  unsigned t = group(), batch = t / (tiles * ct * p[15]);
  t %= tiles * ct * p[15];
  unsigned g = t / (tiles * ct);
  t %= tiles * ct;
  unsigned c = t / tiles * 16 + row, pos = t % tiles * 16 + col,
           count = ci * kh * kw;
  float sum = 0;
  for (unsigned base = 0; base < count; base += 16) {
    unsigned wk = base + col, k = base + row;
    weights[lane()] =
        (c < co && wk < count) ? xs[1][(g * co + c) * count + wk] : 0;
    float v = 0;
    if (pos < spatial && k < count) {
      int y = int(pos / ow * p[9] + k / kw % kh * p[11]) - int(p[13]),
          x = int(pos % ow * p[10] + k % kw * p[12]) - int(p[14]);
      if (y >= 0 && x >= 0 && y < int(ih) && x < int(iw))
        v = xs[0]
              [((batch * ic + g * ci + k / (kh * kw)) * ih + unsigned(y)) * iw +
               unsigned(x)];
    }
    pixels[lane()] = v;
    barrier();
    for (unsigned j = 0; j < 16; ++j)
      sum += weights[row * 16 + j] * pixels[j * 16 + col];
    barrier();
  }
  if (c < co && pos < spatial)
    dst[((batch * oc + g * co + c) * spatial) + pos] = sum + xs[2][g * co + c];
}
extern "C" K void softmax(const float *const *xs, float *, float *dst,
                          const unsigned *p, unsigned len) {
  __attribute__((shared)) float reduce[256];
  unsigned l = lane(), n = p[1], stride = p[2],
           base = group() / stride * (n * stride) + group() % stride;
  float top = -3.402823466e38f;
  for (unsigned j = l; j < n; j += 256)
    top = max(top, xs[0][base + j * stride]);
  reduce[l] = top;
  barrier();
  for (unsigned k = 128; k; k /= 2) {
    if (l < k)
      reduce[l] = max(reduce[l], reduce[l + k]);
    barrier();
  }
  top = reduce[0];
  // Every warp must read the maximum before lane zero reuses this storage
  // for its partial sum (rows shorter than a block have uneven work).
  barrier();
  float sum = 0;
  for (unsigned j = l; j < n; j += 256)
    sum += exp(xs[0][base + j * stride] - top);
  reduce[l] = sum;
  barrier();
  for (unsigned k = 128; k; k /= 2) {
    if (l < k)
      reduce[l] += reduce[l + k];
    barrier();
  }
  sum = reduce[0];
  for (unsigned j = l; j < n; j += 256)
    dst[base + j * stride] = exp(xs[0][base + j * stride] - top) / sum;
}
extern "C" K void reduction(const float *const *xs, float *, float *dst,
                            const unsigned *p, unsigned len) {
  __attribute__((shared)) float partial[256];
  unsigned l = lane();
  partial[l] = reduce_lane(xs[0], p, group(), l, 256);
  barrier();
  for (unsigned k = 128; k; k /= 2) {
    if (l < k)
      partial[l] = reduce_combine(partial[l], partial[l + k], p[1]);
    barrier();
  }
  if (l == 0)
    dst[group()] = partial[0];
}
#endif
