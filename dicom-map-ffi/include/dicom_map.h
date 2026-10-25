/* dicom_map.h - C API for dicom-map.
 *
 * Link against libdicom_map_ffi (shared or static). Call dmap_abi_version()
 * at startup and bail if it does not equal DMAP_ABI_VERSION below.
 */
#ifndef DICOM_MAP_H
#define DICOM_MAP_H

#include <stddef.h>
#include <stdint.h>
#include <stdbool.h>

#ifdef __cplusplus
extern "C" {
#endif

#define DMAP_ABI_VERSION 2u

typedef struct DmapHandle DmapHandle;

typedef struct DmapTag {
    uint16_t group;
    uint16_t element;
    bool     block_offset;
    bool     retired;
    char     vr[16];         /* NUL-terminated, zero-padded: "US", ...,
                                "US or SS", "OB or OW", "US or OW",
                                "US or SS or OW", or "??" (source declares no VR) */
    const char* keyword;     /* owned by this struct; release with dmap_free_tag */
    const char* name;
    const char* creator;     /* empty string for public tags (never NULL) */
    const char* description;
} DmapTag;

/* Opens a .dmap file. Returns NULL on failure. */
DmapHandle* dmap_open(const char* path);

/* Releases a handle. NULL-safe. */
void dmap_close(DmapHandle* h);

/* Number of entries (public + private). Returns 0 for NULL. */
size_t dmap_len(const DmapHandle* h);

/* Look up a tag. Returns:
 *   1  on hit - `*out` populated, caller must dmap_free_tag(out) when done.
 *   0  on miss - `*out` is untouched.
 *  -1  on error (NULL args, bad UTF-8 in creator, a record with an embedded
 *      NUL in a string, etc.). dmap_last_error() describes it.
 *
 * `creator` may be NULL for public tags.
 */
int dmap_lookup(
    const DmapHandle* h,
    uint16_t group,
    uint16_t element,
    const char* creator,
    DmapTag* out
);

/* Releases strings inside a DmapTag populated by dmap_lookup.
 * Safe to call on a zeroed struct. */
void dmap_free_tag(DmapTag* tag);

/* Last error string for this handle, or NULL. Owned by the handle. */
const char* dmap_last_error(const DmapHandle* h);

/* ABI version. Must equal DMAP_ABI_VERSION at runtime. */
uint32_t dmap_abi_version(void);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* DICOM_MAP_H */
