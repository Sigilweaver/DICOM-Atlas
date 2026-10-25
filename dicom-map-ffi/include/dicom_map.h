/* dicom_map.h - C API for dicom-map.
 *
 * Link against libdicom_map_ffi (shared or static). Call dmap_abi_version()
 * at startup and bail if it does not equal DMAP_ABI_VERSION below.
 * DMAP_ABI_MINOR grows when functions or constants are added.
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
#define DMAP_ABI_MINOR 0u

/* Status codes. New codes may be added; treat unknown non-zero values as
 * failure. */
#define DMAP_OK 0
#define DMAP_ERR_IO 1
#define DMAP_ERR_BAD_MAGIC 2
#define DMAP_ERR_UNSUPPORTED_VERSION 3
#define DMAP_ERR_BAD_ARCHIVE 4
#define DMAP_ERR_TOO_SMALL 5
#define DMAP_ERR_INVALID_ARGUMENT 6
#define DMAP_ERR_OTHER 255

/* Bits of dmap_tag_flags(). Ignore bits you do not know. */
#define DMAP_TAG_RETIRED (1u << 0)
#define DMAP_TAG_BLOCK_OFFSET (1u << 1)

/* Value of dmap_tag_vm_max() meaning "n" (unbounded). */
#define DMAP_VM_UNBOUNDED 0xFFFFu

typedef struct DmapHandle DmapHandle;
typedef struct DmapTag DmapTag;   /* opaque: read with dmap_tag_* */
typedef struct DmapErr DmapErr;   /* opaque: read with dmap_err_* */

/* Opens a .dmap file. Returns DMAP_OK and sets *out, or a DMAP_ERR_* status
 * (and *out = NULL). If err is not NULL it receives the cause on failure;
 * release it with dmap_err_free. */
int32_t dmap_open_ex(const char* path, DmapHandle** out, DmapErr** err);

/* Opens a file; NULL on any failure with no cause. Prefer dmap_open_ex. */
DmapHandle* dmap_open(const char* path);

/* Releases a handle. NULL-safe. */
void dmap_close(DmapHandle* h);

/* Number of entries (public + private). Sets *out and returns DMAP_OK, or
 * returns DMAP_ERR_INVALID_ARGUMENT for NULL arguments. */
int32_t dmap_len(const DmapHandle* h, size_t* out);

/* Look up a tag. Returns:
 *   1  on hit - *out is a new DmapTag; release with dmap_tag_free.
 *   0  on miss - *out is untouched.
 *  -1  on error (NULL args, bad UTF-8 in creator, a record with an embedded
 *      NUL in a string). dmap_last_error() describes it on this thread.
 *
 * `creator` may be NULL for public tags. For a private creator on an odd
 * group the element may be given as written in a file (0x1008) or as the
 * block offset (0x08); only the low byte is used. */
int dmap_lookup(
    const DmapHandle* h,
    uint16_t group,
    uint16_t element,
    const char* creator,
    DmapTag** out
);

/* Releases a tag. NULL-safe. */
void dmap_tag_free(DmapTag* tag);

/* Getters. Numeric getters return 0 for a NULL tag. String getters return
 * NUL-terminated UTF-8 owned by the tag (valid until dmap_tag_free), an empty
 * string for an empty field, and NULL for a NULL tag. */
uint16_t dmap_tag_group(const DmapTag* tag);
uint16_t dmap_tag_element(const DmapTag* tag);
uint32_t dmap_tag_flags(const DmapTag* tag);
uint16_t dmap_tag_vm_min(const DmapTag* tag);
uint16_t dmap_tag_vm_max(const DmapTag* tag);
uint16_t dmap_tag_vm_step(const DmapTag* tag);
/* "US", ..., "US or SS", "OB or OW", "US or OW", "US or SS or OW", or "??"
 * when the source declares no VR. New values may appear. */
const char* dmap_tag_vr(const DmapTag* tag);
const char* dmap_tag_keyword(const DmapTag* tag);
const char* dmap_tag_name(const DmapTag* tag);
const char* dmap_tag_creator(const DmapTag* tag);     /* "" for public tags */
const char* dmap_tag_description(const DmapTag* tag);
const char* dmap_tag_sources(const DmapTag* tag);     /* '|'-delimited */

/* Last lookup error on the calling thread (like errno), or NULL. Set by every
 * dmap_lookup call that returns -1, including NULL arguments. `h` is accepted
 * for compatibility and may be NULL. The string is valid until the next error
 * on the same thread. A handle may be used from several threads at once. */
const char* dmap_last_error(const DmapHandle* h);

/* Open errors. */
int32_t dmap_err_kind(const DmapErr* err);
const char* dmap_err_message(const DmapErr* err);
/* For DMAP_ERR_UNSUPPORTED_VERSION: file major/minor and the major this
 * library reads; returns 1. Returns 0 for other errors. */
int32_t dmap_err_version(const DmapErr* err, uint16_t* got_major,
                         uint16_t* got_minor, uint16_t* supported_major);
void dmap_err_free(DmapErr* err);

/* ABI version. Must equal DMAP_ABI_VERSION at runtime. */
uint32_t dmap_abi_version(void);
uint32_t dmap_abi_minor(void);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* DICOM_MAP_H */
