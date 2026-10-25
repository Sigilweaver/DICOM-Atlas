#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "dicom_map.h"

static int fail = 0;
#define CHECK(cond, msg) do { if (!(cond)) { fprintf(stderr, "FAIL: %s\n", msg); fail = 1; } } while (0)

int main(void) {
    CHECK(dmap_abi_version() == DMAP_ABI_VERSION, "ABI version");
    CHECK(dmap_abi_minor() >= DMAP_ABI_MINOR, "ABI minor");

    DmapHandle* h = NULL;
    DmapErr* err = NULL;
    int32_t rc0 = dmap_open_ex("does-not-exist.dmap", &h, &err);
    CHECK(rc0 == DMAP_ERR_IO && h == NULL && err != NULL, "open_ex reports IO");
    CHECK(dmap_err_kind(err) == DMAP_ERR_IO && strlen(dmap_err_message(err)) > 0, "err kind and message");
    dmap_err_free(err);

    rc0 = dmap_open_ex("tags.dmap", &h, &err);
    CHECK(rc0 == DMAP_OK && h != NULL && err == NULL, "dmap_open_ex");
    if (!h) return 1;

    size_t n = 0;
    CHECK(dmap_len(h, &n) == DMAP_OK && n > 9000, "dmap_len");
    CHECK(dmap_len(NULL, &n) == DMAP_ERR_INVALID_ARGUMENT, "dmap_len NULL");

    DmapTag* t = NULL;
    int rc = dmap_lookup(h, 0x0008, 0x0005, NULL, &t);
    CHECK(rc == 1 && t, "public lookup rc");
    CHECK(strcmp(dmap_tag_keyword(t), "SpecificCharacterSet") == 0, "keyword");
    CHECK(strcmp(dmap_tag_vr(t), "CS") == 0, "vr");
    dmap_tag_free(t);

    t = NULL;
    rc = dmap_lookup(h, 0x0021, 0x0008, "Siemens: Thorax/Multix FD Lab Settings", &t);
    CHECK(rc == 1, "private lookup rc");
    CHECK(strcmp(dmap_tag_name(t), "Auto Window Flag") == 0, "private name");
    CHECK((dmap_tag_flags(t) & DMAP_TAG_BLOCK_OFFSET) != 0, "block_offset");
    dmap_tag_free(t);

    t = NULL;
    rc = dmap_lookup(h, 0x7FE0, 0x0010, NULL, &t);
    CHECK(rc == 1, "pixel data rc");
    CHECK(strcmp(dmap_tag_vr(t), "OB or OW") == 0, "multi-VR is not truncated");
    dmap_tag_free(t);

    t = NULL;
    rc = dmap_lookup(h, 0xFFFF, 0xFFFE, NULL, &t);
    CHECK(rc == 0 && t == NULL, "miss rc");

    dmap_close(h);
    if (fail) { fprintf(stderr, "C FFI smoke test FAILED\n"); return 1; }
    fprintf(stdout, "C FFI smoke test OK\n");
    return 0;
}
