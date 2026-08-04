#pragma once

#include <cstddef>
#include <cstdint>
#include <span>
#include <vector>

namespace spherra_bench {

class AngleTables {
public:
    explicit AngleTables(std::size_t angle_count);

    void set(
        std::size_t angle_index,
        std::uint8_t code,
        float cosine,
        float sine);

    [[nodiscard]] float cosine(std::size_t angle_index, std::uint8_t code) const;
    [[nodiscard]] float sine(std::size_t angle_index, std::uint8_t code) const;
    [[nodiscard]] std::size_t angle_count() const noexcept;

private:
    std::size_t angle_count_;
    std::vector<float> cosines_;
    std::vector<float> sines_;
};

std::vector<float> decode_hyperspherical(std::span<const float> angles);

float score_hyperspherical(
    std::span<const float> angles,
    std::span<const float> query);

float score_quantized_angles(
    std::span<const std::uint8_t> codes,
    std::span<const float> query,
    const AngleTables& tables);

float score_cartesian_int4(
    std::span<const std::uint8_t> codes,
    std::span<const float> query);

float score_packed_quantized_angles(
    std::span<const std::uint8_t> packed_codes,
    std::size_t angle_count,
    std::span<const float> query,
    const AngleTables& tables);

float score_packed_cartesian_int4(
    std::span<const std::uint8_t> packed_codes,
    std::size_t dimension,
    std::span<const float> query);

std::vector<std::uint8_t> pack_soa_nibbles(
    const std::vector<std::vector<std::uint8_t>>& vectors);

std::vector<float> scan_packed_angles_soa(
    std::span<const std::uint8_t> packed_codes,
    std::size_t vector_count,
    std::size_t angle_count,
    std::span<const float> query,
    const AngleTables& tables);

std::vector<float> scan_packed_cartesian_soa(
    std::span<const std::uint8_t> packed_codes,
    std::size_t vector_count,
    std::size_t dimension,
    std::span<const float> query);

std::vector<std::uint8_t> pack_tiled_soa_nibbles(
    const std::vector<std::vector<std::uint8_t>>& vectors,
    std::size_t tile_size);

std::vector<float> scan_packed_cartesian_tiled_soa(
    std::span<const std::uint8_t> packed_codes,
    std::size_t vector_count,
    std::size_t dimension,
    std::size_t tile_size,
    std::span<const float> query);

float score_packed_cartesian_tiled_soa_at(
    std::span<const std::uint8_t> packed_codes,
    std::size_t vector_count,
    std::size_t dimension,
    std::size_t tile_size,
    std::size_t vector_index,
    std::span<const float> query);

std::vector<std::uint8_t> pack_nibbles(std::span<const std::uint8_t> codes);

std::vector<std::uint8_t> unpack_nibbles(
    std::span<const std::uint8_t> packed,
    std::size_t code_count);

}  // namespace spherra_bench
