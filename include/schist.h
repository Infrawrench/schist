#ifndef SCHIST_H
#define SCHIST_H
#include <stddef.h>
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif

typedef struct SchistApp SchistApp;
typedef struct { uint8_t *data; size_t len; } SchistBuffer;

/* ABI 1. Calls on one handle must be serialized. Different handles may run
 * concurrently. The library does not install process hooks or start services.
 * Every output must be freed, including errors. Outputs are length-delimited,
 * not NUL-terminated. Initialize output storage to {0}; free it before reuse.
 * Status: 0 success, 1 invalid input/operation, 2 panic (destroy the handle).
 * Input spans must be valid for their lengths and must not alias output storage.
 * A null input pointer is valid only for a zero-length span.
 */
uint32_t schist_abi_version(void);
SchistApp *schist_create(void);
void schist_destroy(SchistApp *app);
void schist_buffer_free(SchistBuffer *buffer);
int32_t schist_request(SchistApp *app, const uint8_t *json, size_t len, SchistBuffer *out);
int32_t schist_import(SchistApp *app, const uint8_t *name, size_t name_len,
                     const uint8_t *data, size_t len, SchistBuffer *out);
int32_t schist_load_model(SchistApp *app, const uint8_t *id, size_t id_len,
                         const uint8_t *data, size_t len, SchistBuffer *out);

#ifdef __cplusplus
}
#endif
#endif
