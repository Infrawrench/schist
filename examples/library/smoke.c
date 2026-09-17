#include "schist.h"
#include <assert.h>
#include <stdio.h>
#include <string.h>

int main(void) {
    assert(schist_abi_version() == 1);
    SchistApp *app = schist_create();
    assert(app);
    SchistBuffer out = {0};
    const char *requests[] = {
        "{\"op\":\"create\",\"width\":8,\"height\":8}",
        "{\"op\":\"call\",\"session\":1,\"name\":\"adjust_invert\"}",
        "{\"op\":\"export\",\"session\":1,\"extension\":\"png\"}",
        "{\"op\":\"close\",\"session\":1}"
    };
    for (size_t i = 0; i < sizeof requests / sizeof *requests; ++i) {
        int status = schist_request(app, (const uint8_t *)requests[i], strlen(requests[i]), &out);
        if (status) fwrite(out.data, 1, out.len, stderr);
        assert(status == 0);
        schist_buffer_free(&out);
    }
    schist_destroy(app);
    puts("C ABI smoke passed");
    return 0;
}
