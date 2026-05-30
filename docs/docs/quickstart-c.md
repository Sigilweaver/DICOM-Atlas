---
title: C FFI quickstart
sidebar_label: C FFI quickstart
---

# C FFI quickstart

```sh
cargo build --release -p dicom-map-ffi
# -> target/release/libdicom_map_ffi.{so,a,dylib}
# header: dicom-map-ffi/include/dicom_map.h
```

```c
#include "dicom_map.h"
#include <stdio.h>

int main(void) {
    DmapHandle *h = dmap_open("tags.dmap");
    if (!h) {
        fprintf(stderr, "open failed\n");
        return 1;
    }

    DmapTag *t = dmap_lookup(h, 0x0008, 0x0005, NULL);
    if (t) {
        printf("%s %s\n", t->vr, t->keyword);
        dmap_free_tag(t);
    }

    DmapTag *p = dmap_lookup(h, 0x0021, 0x0008,
                             "Siemens: Thorax/Multix FD Lab Settings");
    if (p) {
        printf("%s %s\n", p->vr, p->name);
        dmap_free_tag(p);
    }

    dmap_close(h);
    return 0;
}
```

Build:

```sh
gcc -Wall -O2 \
    -I dicom-map-ffi/include \
    smoke.c \
    -L target/release -ldicom_map_ffi \
    -o smoke

LD_LIBRARY_PATH=target/release ./smoke
```

## ABI guarantees

The FFI surface is `extern "C"` with C-stable layouts on the
returned `DmapTag` struct. The handle is opaque. All pointers
returned by `dmap_lookup` and `dmap_last_error` must be freed by
the corresponding `dmap_free_*` function. See
[`dicom_map.h`](https://github.com/Sigilweaver/DICOM-Atlas/blob/main/dicom-map-ffi/include/dicom_map.h)
for the authoritative declarations.
