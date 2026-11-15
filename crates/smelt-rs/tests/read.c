/* Write what the ORE named on the command line holds to stdout, read in
 * 4 KiB pieces as a drive thread would: smelt.h, from C. */

#include <stdio.h>
#include <stdlib.h>

#include "smelt.h"

int main(int argc, char **argv) {
    smelt_ingot *ingot = NULL;
    uint64_t size = 0, at = 0;
    unsigned char piece[4096];

    if (argc != 2) {
        return 2;
    }
    if (smelt_open(argv[1], NULL, &ingot) != SMELT_OK ||
        smelt_logical_size(ingot, &size) != SMELT_OK) {
        fprintf(stderr, "%s\n", smelt_last_error());
        return 1;
    }
    for (at = 0; at < size; at += sizeof piece) {
        size_t len = size - at < sizeof piece ? (size_t)(size - at) : sizeof piece;
        if (smelt_read(ingot, at, piece, len) != SMELT_OK) {
            fprintf(stderr, "%s\n", smelt_last_error());
            smelt_close(ingot);
            return 1;
        }
        fwrite(piece, 1, len, stdout);
    }
    smelt_close(ingot);
    return 0;
}
