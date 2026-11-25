/*
 * smelt for C++: smelt.h's ingot as a class that closes it when it goes, and
 * errors as exceptions. Header-only, C++11.
 *
 * A smelt::ingot belongs to one thread at a time, as a smelt_ingot does;
 * clone() gives another thread one of its own.
 */

#ifndef SMELT_HPP
#define SMELT_HPP

#include "smelt.h"

#include <cstddef>
#include <cstdint>
#include <stdexcept>
#include <string>
#include <vector>

namespace smelt {

/* A call that failed: its code, and smelt_last_error's text. */
class error : public std::runtime_error {
public:
    explicit error(smelt_err code) : std::runtime_error(smelt_last_error()), code_(code) {}
    smelt_err code() const noexcept { return code_; }

private:
    smelt_err code_;
};

inline void check(smelt_err code) {
    if (code != SMELT_OK) {
        throw error(code);
    }
}

/* An open ORE. */
class ingot {
public:
    explicit ingot(const std::string &path) { check(smelt_open(path.c_str(), nullptr, &raw_)); }
    ingot(ingot &&other) noexcept : raw_(other.raw_) { other.raw_ = nullptr; }
    ingot &operator=(ingot &&other) noexcept {
        if (this != &other) {
            smelt_close(raw_);
            raw_ = other.raw_;
            other.raw_ = nullptr;
        }
        return *this;
    }
    ingot(const ingot &) = delete;
    ingot &operator=(const ingot &) = delete;
    ~ingot() { smelt_close(raw_); }

    /* Another ingot over the same ORE, for another thread. */
    ingot clone() const {
        smelt_ingot *out = nullptr;
        check(smelt_clone(raw_, &out));
        return ingot(out);
    }

    /* The size in bytes of what the ORE holds. */
    std::uint64_t size() const {
        std::uint64_t out = 0;
        check(smelt_logical_size(raw_, &out));
        return out;
    }

    /* The platform it was written as: "gamecube", "ps2", "snes"... */
    std::string platform() const {
        std::size_t len = 0;
        ask(smelt_platform(raw_, nullptr, &len));
        std::string out(len, '\0');
        check(smelt_platform(raw_, &out[0], &len));
        out.resize(len - 1);
        return out;
    }

    /* The file it was written from, a ROM's or a floppy's image's; throws
       for a disc, a CD or a title. */
    std::string file_name() const {
        std::size_t len = 0;
        ask(smelt_file_name(raw_, nullptr, &len));
        std::string out(len, '\0');
        check(smelt_file_name(raw_, &out[0], &len));
        out.resize(len - 1);
        return out;
    }

    /* The hash `scheme` recorded of what it holds: "sha-1", "md5"... */
    std::vector<std::uint8_t> identity(const char *scheme) const {
        std::size_t len = 0;
        ask(smelt_identity(raw_, scheme, nullptr, &len));
        std::vector<std::uint8_t> out(len);
        check(smelt_identity(raw_, scheme, out.data(), &len));
        return out;
    }

    /* Read `len` bytes at `offset` into `dst`, all or nothing. */
    void read(std::uint64_t offset, void *dst, std::size_t len) {
        check(smelt_read(raw_, offset, static_cast<std::uint8_t *>(dst), len));
    }

    /* A CD's tracks; none for what is not a CD. */
    std::vector<smelt_track> tracks() const {
        std::size_t count = 0;
        check(smelt_track_count(raw_, &count));
        std::vector<smelt_track> out(count);
        for (std::size_t i = 0; i < count; ++i) {
            check(smelt_track_at(raw_, i, &out[i]));
        }
        return out;
    }

    /* Read `n` whole CD sectors from `lba`, SMELT_SECTOR bytes each. */
    void read_sectors(std::uint32_t lba, std::uint32_t n, void *dst) {
        check(smelt_read_sectors(raw_, lba, n, static_cast<std::uint8_t *>(dst)));
    }

    /* The SBI's Q for the sector at `lba` into `q`, 10 bytes: whether it
     * replaces the one its position generates. */
    bool subq_replacement(std::uint32_t lba, std::uint8_t q[10]) const {
        std::uint8_t replaced = 0;
        check(smelt_subq_replacement(raw_, lba, q, &replaced));
        return replaced != 0;
    }

    /* A Wii U title's file: its name, and where it lies in the byte view. */
    struct file {
        std::string name;
        std::uint64_t offset;
        std::uint64_t size;
    };

    /* A Wii U title's files but its ticket; none for what is not a title. */
    std::vector<file> files() const {
        std::size_t count = 0;
        check(smelt_file_count(raw_, &count));
        std::vector<file> out(count);
        for (std::size_t i = 0; i < count; ++i) {
            std::size_t len = 0;
            ask(smelt_file_at(raw_, i, nullptr, &len, &out[i].offset, &out[i].size));
            out[i].name.assign(len, '\0');
            check(smelt_file_at(raw_, i, &out[i].name[0], &len, &out[i].offset, &out[i].size));
            out[i].name.resize(len - 1);
        }
        return out;
    }

    /* A Wii U title's ticket; throws for one written without it. */
    std::vector<std::uint8_t> ticket() const {
        std::size_t len = 0;
        ask(smelt_ticket(raw_, nullptr, &len));
        std::vector<std::uint8_t> out(len);
        check(smelt_ticket(raw_, out.data(), &len));
        return out;
    }

    smelt_ingot *get() noexcept { return raw_; }

private:
    explicit ingot(smelt_ingot *raw) noexcept : raw_(raw) {}

    /* A length asked for comes back as SMELT_ERR_BUFFER_TOO_SMALL */
    static void ask(smelt_err code) {
        if (code != SMELT_ERR_BUFFER_TOO_SMALL) {
            check(code);
        }
    }

    smelt_ingot *raw_ = nullptr;
};

} // namespace smelt

#endif /* SMELT_HPP */
