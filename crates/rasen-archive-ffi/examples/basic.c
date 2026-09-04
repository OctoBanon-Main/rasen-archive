/*
Build from repository root.

Windows, Visual Studio Developer Command Prompt:
  cargo build -p rasen-archive-ffi --release
  cl /nologo /W4 /I crates\rasen-archive-ffi\include crates\rasen-archive-ffi\examples\basic.c /Fe:target\release\rasen-ffi-example.exe /link /LIBPATH:target\release rasen_archive_ffi.dll.lib
  target\release\rasen-ffi-example.exe

Linux:
  cargo build -p rasen-archive-ffi --release
  cc -std=c11 -Wall -Wextra -Werror crates/rasen-archive-ffi/examples/basic.c -I crates/rasen-archive-ffi/include -L target/release -lrasen_archive_ffi -Wl,-rpath,'$ORIGIN' -o target/release/rasen-ffi-example
  target/release/rasen-ffi-example
*/

#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

#include "../include/rasen_archive.h"

#define TRY(expression)                                                        \
    do {                                                                       \
        status = (expression);                                                 \
        if (status != RASEN_OK) {                                              \
            fprintf(stderr, "%s failed (%d): %s\n", #expression,             \
                    (int)status, rasen_last_error_message());                  \
            exit_code = 1;                                                     \
            goto cleanup;                                                      \
        }                                                                      \
    } while (0)

int main(void) {
    static const uint8_t key[] = "example-key";
    static const uint8_t greeting[] = "Hello from the Rasen C API!\n";
    static const uint8_t numbers[] = {1, 2, 3, 5, 8, 13, 21};
    static const char greeting_path[] = "text/greeting.txt";
    static const char numbers_path[] = "data/numbers.bin";

    const RasenInputFile files[] = {
        {(const uint8_t *)greeting_path, sizeof(greeting_path) - 1, greeting,
         sizeof(greeting) - 1},
        {(const uint8_t *)numbers_path, sizeof(numbers_path) - 1, numbers,
         sizeof(numbers)},
    };
    RasenPackOptions options;
    RasenPackSummary summary = {0};
    RasenBuffer packed = {0};
    RasenBuffer asset = {0};
    RasenArchive *archive = NULL;
    RasenArchiveInfo info;
    RasenStatus status;
    uint64_t numbers_id = 0;
    int exit_code = 0;

    TRY(rasen_pack_options_default(&options));
    options.protection = RASEN_PROTECTION_AEAD;

    TRY(rasen_pack_memory(files, sizeof(files) / sizeof(files[0]), key,
                          sizeof(key) - 1, &options, &packed, &summary));
    printf("Created archive: %" PRIu64 " bytes, %" PRIu32
           " entries, %" PRIu32 " chunks\n",
           summary.archive_len, summary.entry_count, summary.chunk_count);

    TRY(rasen_archive_open_memory(packed.data, packed.len, key, sizeof(key) - 1,
                                  NULL, &archive));
    /* open_memory copies input, so packed bytes are no longer needed. */
    rasen_buffer_free(&packed);

    TRY(rasen_archive_info(archive, &info));
    printf("Archive entries:\n");
    for (size_t i = 0; i < info.entry_count; ++i) {
        RasenEntry entry;
        TRY(rasen_archive_entry_at(archive, i, &entry));
        printf("  ");
        if (entry.path != NULL) {
            fwrite(entry.path, 1, entry.path_len, stdout);
        } else {
            printf("<stripped path>");
        }
        printf(" (%" PRIu64 " bytes)\n", entry.original_size);
    }

    TRY(rasen_archive_read(archive, (const uint8_t *)greeting_path,
                           sizeof(greeting_path) - 1, &asset));
    printf("\nRead by path: ");
    fwrite(asset.data, 1, asset.len, stdout);
    rasen_buffer_free(&asset);

    TRY(rasen_hash_path((const uint8_t *)numbers_path,
                        sizeof(numbers_path) - 1, &numbers_id));
    TRY(rasen_archive_read_by_id(archive, numbers_id, &asset));
    printf("Read by id %016" PRIx64 ":", numbers_id);
    for (size_t i = 0; i < asset.len; ++i) {
        printf(" %u", (unsigned)asset.data[i]);
    }
    printf("\n");

cleanup:
    rasen_buffer_free(&asset);
    rasen_buffer_free(&packed);
    rasen_archive_free(archive);
    return exit_code;
}
