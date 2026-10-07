// Plain C entry point into the vendored LZHAM alpha decompressor.
#include "lzham_core.h"
#include "lzham_decomp.h"

extern "C" lzham_decompress_status_t tf_lzham_decompress_memory(const lzham_decompress_params *params,
    lzham_uint8 *dst, size_t *dst_len, const lzham_uint8 *src, size_t src_len, lzham_uint32 *adler32)
{
    return lzham::lzham_lib_decompress_memory(params, dst, dst_len, src, src_len, adler32);
}
