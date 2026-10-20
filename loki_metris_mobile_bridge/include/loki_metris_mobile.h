#ifndef LOKI_METRIS_MOBILE_H
#define LOKI_METRIS_MOBILE_H
#include <stddef.h>
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif
#define LOKI_MOBILE_RESULT_CAPACITY (192u * 1024u)
/* 输入/输出是UTF-8字节范围；不用JNI modified-UTF8，不带终止NUL。 */
uint64_t loki_mobile_create(const uint8_t *identity, size_t identity_len);
/* 固定容量不足时不执行操作。所有缓冲须有效、不重叠，length须对齐。 */
int32_t loki_mobile_call(uint64_t handle, const uint8_t *input, size_t input_len,
                       uint8_t *output, size_t output_capacity, size_t *length);
void loki_mobile_destroy(uint64_t handle);
int32_t loki_mobile_asset_valid(const uint8_t *descriptor, size_t descriptor_len,
                              const uint8_t *bytes, size_t bytes_len);
#ifdef __cplusplus
}
#endif
#endif
