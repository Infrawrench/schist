// Maintainer-only arithmetic oracle: compiles the exact scalar CUDA kernels
// for the host so Rust can compare them with tract without an NVIDIA device.
// It does not validate GPU scheduling, synchronization, or PTX execution.
#define SCHIST_CUDA_HOST
#include "../crates/neural/src/cuda/kernels.cu"
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <vector>
static unsigned word() {
  unsigned v;
  if (std::fread(&v, 4, 1, stdin) != 1)
    std::exit(2);
  return v;
}
static void words(void *p, size_t n) {
  if (std::fread(p, 4, n, stdin) != n)
    std::exit(2);
}
int main() {
  unsigned values = word();
  if (values > 50000)
    return 2;
  std::vector<std::vector<float>> tensors(values);
  unsigned initial = word();
  for (unsigned i = 0; i < initial; ++i) {
    unsigned id = word(), len = word();
    if (id >= values || len > 268435456)
      return 2;
    tensors[id].resize(len);
    words(tensors[id].data(), len);
  }
  unsigned steps = word();
  float error = 0;
  for (unsigned s = 0; s < steps; ++s) {
    unsigned kernel = word(), out = word(), len = word(), n = word();
    if (out >= values || n > 15 || len > 268435456)
      return 2;
    const float *xs[16] = {};
    for (unsigned j = 0; j < n; ++j) {
      unsigned id = word();
      if (id >= values || tensors[id].empty())
        return 2;
      xs[j] = tensors[id].data();
    }
    xs[15] = &error;
    unsigned plen = word();
    if (plen > 4096)
      return 2;
    std::vector<unsigned> p(plen);
    words(p.data(), plen);
    tensors[out].resize(len);
    float *dst = tensors[out].data();
    if (kernel == 1) {
      unsigned m = p[1], n = p[2], k = p[3], ra = p[4], ca = p[5], ba = p[6],
               ka = p[7];
      const unsigned *rows = p.data() + 8;
      const unsigned *cols = rows + ra * 4;
      const unsigned *batches = cols + ca * 4;
      const unsigned *red = batches + ba * 4;
      for (unsigned batch = 0; batch < len / (m * n); ++batch)
        for (unsigned r = 0; r < m; ++r)
          for (unsigned c = 0; c < n; ++c) {
            float sum = 0;
            unsigned a = offset(batch, batches, ba, 4, 1) +
                         offset(r, rows, ra, 4, 1),
                     b = offset(batch, batches, ba, 4, 2) +
                         offset(c, cols, ca, 4, 2);
            for (unsigned j = 0; j < k; ++j)
              sum += xs[0][a + offset(j, red, ka, 3, 1)] *
                     xs[1][b + offset(j, red, ka, 3, 2)];
            dst[offset(batch, batches, ba, 4, 3) + offset(r, rows, ra, 4, 3) +
                offset(c, cols, ca, 4, 3)] = sum;
          }
    } else {
      for (unsigned i = 0; i < len; ++i)
        element(xs, dst, p.data(), i);
    }
  }
  if (error != 0)
    return 3;
  unsigned outputs = word();
  for (unsigned i = 0; i < outputs; ++i) {
    unsigned id = word();
    if (id >= values)
      return 2;
    auto &t = tensors[id];
    if (std::fwrite(t.data(), 4, t.size(), stdout) != t.size())
      return 2;
  }
  return 0;
}
