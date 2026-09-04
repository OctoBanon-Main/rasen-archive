#ifndef RASEN_ARCHIVE_H
#define RASEN_ARCHIVE_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define RASEN_ABI_VERSION 1u
#define RASEN_FORMAT_VERSION 1u
#define RASEN_HEADER_SIZE 60u
#define RASEN_DEFAULT_CHUNK_SIZE 65536u
#define RASEN_DEFAULT_ALIGNMENT 16u
#define RASEN_MAGIC "RPAK"
#define RASEN_TOC_MAGIC "TOC2"
#define RASEN_PACK_MODE_DEBUG 0u
#define RASEN_PACK_MODE_PRODUCTION 1u
#define RASEN_PROTECTION_XOR 0u
#define RASEN_PROTECTION_AEAD 1u

typedef enum RasenStatus {
    RASEN_OK = 0,
    RASEN_NULL_POINTER = 1,
    RASEN_INVALID_UTF8 = 2,
    RASEN_INVALID_VALUE = 3,
    RASEN_OUT_OF_RANGE = 4,
    RASEN_CALLBACK_FAILED = 5,
    RASEN_PANIC = 6,
    RASEN_IO = 100,
    RASEN_BAD_MAGIC = 101,
    RASEN_BAD_TOC_MAGIC = 102,
    RASEN_UNSUPPORTED_VERSION = 103,
    RASEN_UNSUPPORTED_FLAGS = 104,
    RASEN_UNSUPPORTED_HEADER_SIZE = 105,
    RASEN_EMPTY_KEY = 106,
    RASEN_CRYPTO = 107,
    RASEN_INCOMPLETE_PACK = 108,
    RASEN_NON_EMPTY_DESTINATION = 109,
    RASEN_INVALID_PATH = 110,
    RASEN_INVALID_CHUNK_SIZE = 111,
    RASEN_INVALID_ALIGNMENT = 112,
    RASEN_INVALID_RANGE = 113,
    RASEN_BUFFER_SIZE_MISMATCH = 114,
    RASEN_CHUNK_OUT_OF_RANGE = 115,
    RASEN_DUPLICATE_PATH = 116,
    RASEN_ASSET_TOO_LARGE = 117,
    RASEN_ARCHIVE_TOO_LARGE = 118,
    RASEN_TOO_MANY_ENTRIES = 119,
    RASEN_TOO_MANY_CHUNKS = 120,
    RASEN_METADATA_LIMIT_EXCEEDED = 121,
    RASEN_CORRUPT = 122,
    RASEN_TOO_LARGE = 123,
    RASEN_LZ4 = 124,
    RASEN_NOT_FOUND = 125,
    RASEN_HASH_COLLISION = 126,
    RASEN_CHECKSUM_MISMATCH = 127
} RasenStatus;

typedef struct RasenArchive RasenArchive;
typedef struct RasenScratch RasenScratch;

typedef struct RasenBuffer {
    uint8_t *data;
    size_t len;
} RasenBuffer;

typedef struct RasenArchiveLimits {
    uint64_t max_toc_stored_bytes;
    uint64_t max_toc_raw_bytes;
    uint32_t max_entries;
    uint32_t max_chunks;
    uint32_t max_chunks_per_operation;
    size_t max_path_bytes;
    uint64_t max_total_path_bytes;
    uint64_t max_total_decompressed_bytes;
    uint64_t max_single_asset_bytes;
    uint64_t max_metadata_bytes;
} RasenArchiveLimits;

typedef struct RasenArchiveInfo {
    size_t entry_count;
    uint32_t chunk_size;
    uint32_t alignment;
    uint8_t paths_stripped;
    uint32_t protection;
} RasenArchiveInfo;

/* path points into the archive and remains valid until rasen_archive_free. */
typedef struct RasenEntry {
    const uint8_t *path;
    size_t path_len;
    uint64_t path_hash;
    uint64_t original_size;
    uint64_t stored_size;
    uint32_t first_chunk;
    uint32_t chunk_count;
} RasenEntry;

typedef struct RasenPackOptions {
    size_t chunk_size;
    uint32_t alignment;
    uint32_t mode;
    uint32_t protection;
} RasenPackOptions;

typedef struct RasenPackSummary {
    uint64_t archive_len;
    uint32_t entry_count;
    uint32_t chunk_count;
} RasenPackSummary;

typedef struct RasenInputFile {
    const uint8_t *path;
    size_t path_len;
    const uint8_t *data;
    size_t data_len;
} RasenInputFile;

typedef int32_t (*RasenReadAtFn)(void *user_data, uint64_t offset,
                                uint8_t *dst, size_t len);
typedef void (*RasenDestroyFn)(void *user_data);

/* Ownership of user_data transfers to the archive, including on open failure. */
typedef struct RasenSource {
    void *user_data;
    uint64_t len;
    RasenReadAtFn read_at;
    RasenDestroyFn destroy;
} RasenSource;

typedef int32_t (*RasenReadFn)(void *user_data, uint8_t *dst,
                              size_t capacity, size_t *out_read);

typedef struct RasenStreamInput {
    const uint8_t *path;
    size_t path_len;
    void *user_data;
    RasenReadFn read;
} RasenStreamInput;

/* write must consume all bytes. seek uses an absolute position. */
typedef int32_t (*RasenWriteFn)(void *user_data, const uint8_t *src, size_t len);
typedef int32_t (*RasenSeekFn)(void *user_data, uint64_t position);
typedef int32_t (*RasenLenFn)(void *user_data, uint64_t *out_len);

typedef struct RasenWriter {
    void *user_data;
    RasenWriteFn write;
    RasenSeekFn seek;
    RasenLenFn len;
} RasenWriter;

uint32_t rasen_abi_version(void);
const char *rasen_version_string(void);
/* Pointer is thread-local and remains valid until next FFI error on this thread. */
const char *rasen_last_error_message(void);

/* Output buffers must not already own memory. Functions leave outputs unchanged on error. */
/* Frees Rust-owned output and resets it. NULL and empty buffers are accepted. */
void rasen_buffer_free(RasenBuffer *buffer);

RasenStatus rasen_archive_limits_runtime(RasenArchiveLimits *out);
RasenStatus rasen_archive_limits_tooling(RasenArchiveLimits *out);
RasenStatus rasen_pack_options_default(RasenPackOptions *out);
RasenStatus rasen_hash_path(const uint8_t *path, size_t path_len,
                            uint64_t *out_hash);
RasenStatus rasen_normalize_path(const uint8_t *path, size_t path_len,
                                 RasenBuffer *out);

/* Memory input is copied; caller may release it after return. NULL limits = runtime defaults. */
RasenStatus rasen_archive_open_memory(const uint8_t *data, size_t data_len,
                                      const uint8_t *key, size_t key_len,
                                      const RasenArchiveLimits *limits,
                                      RasenArchive **out);
RasenStatus rasen_archive_open_file(const uint8_t *path, size_t path_len,
                                    const uint8_t *key, size_t key_len,
                                    const RasenArchiveLimits *limits,
                                    RasenArchive **out);
RasenStatus rasen_archive_open_source(RasenSource source,
                                      const uint8_t *key, size_t key_len,
                                      const RasenArchiveLimits *limits,
                                      RasenArchive **out);
void rasen_archive_free(RasenArchive *archive);

RasenStatus rasen_archive_info(const RasenArchive *archive, RasenArchiveInfo *out);
RasenStatus rasen_archive_entry_at(const RasenArchive *archive, size_t index,
                                   RasenEntry *out);
RasenStatus rasen_archive_entry(const RasenArchive *archive,
                                const uint8_t *path, size_t path_len,
                                RasenEntry *out);
RasenStatus rasen_archive_entry_by_id(const RasenArchive *archive,
                                      uint64_t asset_id, RasenEntry *out);
RasenStatus rasen_archive_verify(const RasenArchive *archive);
RasenStatus rasen_archive_contains(const RasenArchive *archive,
                                   const uint8_t *path, size_t path_len,
                                   uint8_t *out_contains);
RasenStatus rasen_archive_contains_id(const RasenArchive *archive,
                                      uint64_t asset_id, uint8_t *out_contains);

RasenStatus rasen_archive_read(const RasenArchive *archive,
                               const uint8_t *path, size_t path_len,
                               RasenBuffer *out);
RasenStatus rasen_archive_read_by_id(const RasenArchive *archive,
                                     uint64_t asset_id, RasenBuffer *out);
RasenStatus rasen_archive_read_into(const RasenArchive *archive,
                                    const uint8_t *path, size_t path_len,
                                    uint8_t *dst, size_t dst_len);
RasenStatus rasen_archive_read_chunk(const RasenArchive *archive,
                                     const uint8_t *path, size_t path_len,
                                     uint32_t chunk_index, RasenBuffer *out);
RasenStatus rasen_archive_read_chunk_into(const RasenArchive *archive,
                                          const uint8_t *path, size_t path_len,
                                          uint32_t chunk_index,
                                          uint8_t *dst, size_t dst_len);
RasenStatus rasen_archive_read_range(const RasenArchive *archive,
                                     const uint8_t *path, size_t path_len,
                                     uint64_t offset, size_t len,
                                     RasenBuffer *out);
RasenStatus rasen_archive_read_range_exact(const RasenArchive *archive,
                                           const uint8_t *path, size_t path_len,
                                           uint64_t offset, size_t len,
                                           RasenBuffer *out);
RasenStatus rasen_archive_read_range_into(const RasenArchive *archive,
                                          const uint8_t *path, size_t path_len,
                                          uint64_t offset,
                                          uint8_t *dst, size_t dst_len,
                                          size_t *out_read);

RasenStatus rasen_scratch_new(RasenScratch **out);
void rasen_scratch_free(RasenScratch *scratch);
RasenStatus rasen_archive_read_into_with_scratch(
    const RasenArchive *archive, const uint8_t *path, size_t path_len,
    uint8_t *dst, size_t dst_len, RasenScratch *scratch);
RasenStatus rasen_archive_read_chunk_into_with_scratch(
    const RasenArchive *archive, const uint8_t *path, size_t path_len,
    uint32_t chunk_index, uint8_t *dst, size_t dst_len, RasenScratch *scratch);
RasenStatus rasen_archive_read_range_with_scratch(
    const RasenArchive *archive, const uint8_t *path, size_t path_len,
    uint64_t offset, uint8_t *dst, size_t dst_len, RasenScratch *scratch,
    size_t *out_read);

/* NULL options = defaults. NULL out_summary is accepted. */
RasenStatus rasen_pack_memory(const RasenInputFile *files, size_t file_count,
                              const uint8_t *key, size_t key_len,
                              const RasenPackOptions *options,
                              RasenBuffer *out_archive,
                              RasenPackSummary *out_summary);
/* Creates a new file and refuses to replace an existing path. */
RasenStatus rasen_pack_file(const uint8_t *output_path, size_t output_path_len,
                            const RasenInputFile *files, size_t file_count,
                            const uint8_t *key, size_t key_len,
                            const RasenPackOptions *options,
                            RasenPackSummary *out_summary);
/* Inputs, writer, and user_data are borrowed only for this call. */
RasenStatus rasen_pack_streams(const RasenStreamInput *inputs,
                               size_t input_count,
                               const uint8_t *key, size_t key_len,
                               const RasenPackOptions *options,
                               const RasenWriter *writer,
                               RasenPackSummary *out_summary);

#ifdef __cplusplus
}
#endif

#endif
