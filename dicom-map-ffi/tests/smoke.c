#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "dicom_map.h"

static int fail = 0;
#define CHECK(cond, msg) do { if (!(cond)) { fprintf(stderr, "FAIL: %s\n", msg); fail = 1; } } while (0)

int main(void) {
    CHECK(dmap_abi_version() == DMAP_ABI_VERSION, "ABI version");

    DmapHandle* h = dmap_open("tags.dmap");
    CHECK(h != NULL, "dmap_open");
    if (!h) return 1;

    CHECK(dmap_len(h) > 9000, "dmap_len");

    DmapTag t = {0};
    int rc = dmap_lookup(h, 0x0008, 0x0005, NULL, &t);
    CHECK(rc == 1, "public lookup rc");
    CHECK(t.keyword && strcmp(t.keyword, "SpecificCharacterSet") == 0, "keyword");
    CHECK(strcmp(t.vr, "CS") == 0, "vr");
    dmap_free_tag(&t);

    memset(&t, 0, sizeof(t));
    rc = dmap_lookup(h, 0x0021, 0x0008, "Siemens: Thorax/Multix FD Lab Settings", &t);
    CHECK(rc == 1, "private lookup rc");
    CHECK(t.name && strcmp(t.name, "Auto Window Flag") == 0, "private name");
    CHECK(t.block_offset == true, "block_offset");
    dmap_free_tag(&t);

    memset(&t, 0, sizeof(t));
    rc = dmap_lookup(h, 0xFFFF, 0xFFFE, NULL, &t);
    CHECK(rc == 0, "miss rc");

    dmap_close(h);
    if (fail) { fprintf(stderr, "C FFI smoke test FAILED\n"); return 1; }
    fprintf(stdout, "C FFI smoke test OK\n");
    return 0;
}
