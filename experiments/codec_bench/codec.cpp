#include "codec.hpp"

#include <cmath>
#include <stdexcept>

namespace spherra_bench {

AngleTables::AngleTables(std::size_t angle_count)
    : angle_count_(angle_count), cosines_(angle_count * 16, 0.0F),
      sines_(angle_count * 16, 0.0F) {}

void AngleTables::set(
    std::size_t angle_index,
    std::uint8_t code,
    float cosine,
    float sine) {
    if (angle_index >= angle_count_ || code > 15) {
        throw std::out_of_range("angle table index is out of range");
    }
    const std::size_t offset = angle_index * 16 + code;
    cosines_[offset] = cosine;
    sines_[offset] = sine;
}

float AngleTables::cosine(std::size_t angle_index, std::uint8_t code) const {
    if (angle_index >= angle_count_ || code > 15) {
        throw std::out_of_range("angle table index is out of range");
    }
    return cosines_[angle_index * 16 + code];
}

float AngleTables::sine(std::size_t angle_index, std::uint8_t code) const {
    if (angle_index >= angle_count_ || code > 15) {
        throw std::out_of_range("angle table index is out of range");
    }
    return sines_[angle_index * 16 + code];
}

std::size_t AngleTables::angle_count() const noexcept {
    return angle_count_;
}

std::vector<float> decode_hyperspherical(std::span<const float> angles) {
    if (angles.empty()) {
        throw std::invalid_argument("at least one angle is required");
    }

    std::vector<float> decoded(angles.size() + 1, 0.0F);
    float prefix = 1.0F;
    for (std::size_t i = 0; i + 1 < angles.size(); ++i) {
        decoded[i] = prefix * std::cos(angles[i]);
        prefix *= std::sin(angles[i]);
    }

    const float last_angle = angles.back();
    decoded[decoded.size() - 2] = prefix * std::cos(last_angle);
    decoded.back() = prefix * std::sin(last_angle);
    return decoded;
}

float score_hyperspherical(
    std::span<const float> angles,
    std::span<const float> query) {
    if (query.size() != angles.size() + 1) {
        throw std::invalid_argument("query dimension must be angle count plus one");
    }

    float score = 0.0F;
    float prefix = 1.0F;
    for (std::size_t i = 0; i + 1 < angles.size(); ++i) {
        score += query[i] * prefix * std::cos(angles[i]);
        prefix *= std::sin(angles[i]);
    }

    const float last_angle = angles.back();
    score += query[query.size() - 2] * prefix * std::cos(last_angle);
    score += query.back() * prefix * std::sin(last_angle);
    return score;
}

float score_quantized_angles(
    std::span<const std::uint8_t> codes,
    std::span<const float> query,
    const AngleTables& tables) {
    if (codes.size() != tables.angle_count() || query.size() != codes.size() + 1) {
        throw std::invalid_argument("quantized angle dimensions do not match");
    }

    float score = 0.0F;
    float prefix = 1.0F;
    for (std::size_t i = 0; i + 1 < codes.size(); ++i) {
        score += query[i] * prefix * tables.cosine(i, codes[i]);
        prefix *= tables.sine(i, codes[i]);
    }
    const std::size_t last = codes.size() - 1;
    score += query[last] * prefix * tables.cosine(last, codes[last]);
    score += query[last + 1] * prefix * tables.sine(last, codes[last]);
    return score;
}

float score_cartesian_int4(
    std::span<const std::uint8_t> codes,
    std::span<const float> query) {
    if (codes.size() != query.size()) {
        throw std::invalid_argument("cartesian code dimensions do not match");
    }

    float score = 0.0F;
    for (std::size_t i = 0; i < codes.size(); ++i) {
        if (codes[i] > 15) {
            throw std::invalid_argument("cartesian int4 code exceeds 15");
        }
        const float value = (static_cast<float>(codes[i]) - 7.5F) / 7.5F;
        score += query[i] * value;
    }
    return score;
}

namespace {

std::uint8_t nibble_at(std::span<const std::uint8_t> packed, std::size_t index) {
    const std::uint8_t byte = packed[index / 2];
    const unsigned shift = (index % 2 == 0) ? 0U : 4U;
    return static_cast<std::uint8_t>((byte >> shift) & 0x0FU);
}

void require_packed_size(std::span<const std::uint8_t> packed, std::size_t count) {
    if (packed.size() < (count + 1) / 2) {
        throw std::invalid_argument("packed input is too short");
    }
}

}  // namespace

float score_packed_quantized_angles(
    std::span<const std::uint8_t> packed_codes,
    std::size_t angle_count,
    std::span<const float> query,
    const AngleTables& tables) {
    require_packed_size(packed_codes, angle_count);
    if (angle_count != tables.angle_count() || query.size() != angle_count + 1) {
        throw std::invalid_argument("packed angle dimensions do not match");
    }

    float score = 0.0F;
    float prefix = 1.0F;
    for (std::size_t i = 0; i + 1 < angle_count; ++i) {
        const std::uint8_t code = nibble_at(packed_codes, i);
        score += query[i] * prefix * tables.cosine(i, code);
        prefix *= tables.sine(i, code);
    }
    const std::size_t last = angle_count - 1;
    const std::uint8_t code = nibble_at(packed_codes, last);
    score += query[last] * prefix * tables.cosine(last, code);
    score += query[last + 1] * prefix * tables.sine(last, code);
    return score;
}

float score_packed_cartesian_int4(
    std::span<const std::uint8_t> packed_codes,
    std::size_t dimension,
    std::span<const float> query) {
    require_packed_size(packed_codes, dimension);
    if (query.size() != dimension) {
        throw std::invalid_argument("packed cartesian dimensions do not match");
    }

    float score = 0.0F;
    for (std::size_t i = 0; i < dimension; ++i) {
        const float value = (static_cast<float>(nibble_at(packed_codes, i)) - 7.5F) / 7.5F;
        score += query[i] * value;
    }
    return score;
}

std::vector<std::uint8_t> pack_soa_nibbles(
    const std::vector<std::vector<std::uint8_t>>& vectors) {
    if (vectors.empty() || vectors.front().empty()) {
        throw std::invalid_argument("SoA input must contain vectors and dimensions");
    }

    const std::size_t dimension = vectors.front().size();
    const std::size_t bytes_per_dimension = (vectors.size() + 1) / 2;
    std::vector<std::uint8_t> packed(dimension * bytes_per_dimension, 0);
    for (std::size_t vector_index = 0; vector_index < vectors.size(); ++vector_index) {
        if (vectors[vector_index].size() != dimension) {
            throw std::invalid_argument("SoA vectors must have equal dimensions");
        }
        for (std::size_t dimension_index = 0; dimension_index < dimension; ++dimension_index) {
            const std::uint8_t code = vectors[vector_index][dimension_index];
            if (code > 15) {
                throw std::invalid_argument("SoA nibble code exceeds 15");
            }
            const std::size_t offset = dimension_index * bytes_per_dimension + vector_index / 2;
            const unsigned shift = (vector_index % 2 == 0) ? 0U : 4U;
            packed[offset] |= static_cast<std::uint8_t>(code << shift);
        }
    }
    return packed;
}

namespace {

std::uint8_t soa_code_at(
    std::span<const std::uint8_t> packed,
    std::size_t vector_count,
    std::size_t vector_index,
    std::size_t dimension_index) {
    const std::size_t bytes_per_dimension = (vector_count + 1) / 2;
    const std::uint8_t byte = packed[dimension_index * bytes_per_dimension + vector_index / 2];
    const unsigned shift = (vector_index % 2 == 0) ? 0U : 4U;
    return static_cast<std::uint8_t>((byte >> shift) & 0x0FU);
}

void require_soa_size(
    std::span<const std::uint8_t> packed,
    std::size_t vector_count,
    std::size_t dimension) {
    const std::size_t required = dimension * ((vector_count + 1) / 2);
    if (packed.size() < required) {
        throw std::invalid_argument("packed SoA input is too short");
    }
}

}  // namespace

std::vector<float> scan_packed_angles_soa(
    std::span<const std::uint8_t> packed_codes,
    std::size_t vector_count,
    std::size_t angle_count,
    std::span<const float> query,
    const AngleTables& tables) {
    require_soa_size(packed_codes, vector_count, angle_count);
    if (angle_count != tables.angle_count() || query.size() != angle_count + 1) {
        throw std::invalid_argument("SoA angle dimensions do not match");
    }

    std::vector<float> scores(vector_count, 0.0F);
    std::vector<float> prefixes(vector_count, 1.0F);
    for (std::size_t angle = 0; angle + 1 < angle_count; ++angle) {
        for (std::size_t vector = 0; vector < vector_count; ++vector) {
            const std::uint8_t code = soa_code_at(packed_codes, vector_count, vector, angle);
            scores[vector] += query[angle] * prefixes[vector] * tables.cosine(angle, code);
            prefixes[vector] *= tables.sine(angle, code);
        }
    }
    const std::size_t last = angle_count - 1;
    for (std::size_t vector = 0; vector < vector_count; ++vector) {
        const std::uint8_t code = soa_code_at(packed_codes, vector_count, vector, last);
        scores[vector] += query[last] * prefixes[vector] * tables.cosine(last, code);
        scores[vector] += query[last + 1] * prefixes[vector] * tables.sine(last, code);
    }
    return scores;
}

std::vector<float> scan_packed_cartesian_soa(
    std::span<const std::uint8_t> packed_codes,
    std::size_t vector_count,
    std::size_t dimension,
    std::span<const float> query) {
    require_soa_size(packed_codes, vector_count, dimension);
    if (query.size() != dimension) {
        throw std::invalid_argument("SoA cartesian dimensions do not match");
    }

    std::vector<float> scores(vector_count, 0.0F);
    for (std::size_t dimension_index = 0; dimension_index < dimension; ++dimension_index) {
        for (std::size_t vector = 0; vector < vector_count; ++vector) {
            const std::uint8_t code = soa_code_at(
                packed_codes, vector_count, vector, dimension_index);
            const float value = (static_cast<float>(code) - 7.5F) / 7.5F;
            scores[vector] += query[dimension_index] * value;
        }
    }
    return scores;
}

namespace {

std::size_t tiled_soa_required_size(
    std::size_t vector_count,
    std::size_t dimension,
    std::size_t tile_size) {
    if (tile_size == 0) {
        throw std::invalid_argument("tile size must be positive");
    }
    const std::size_t tile_count = (vector_count + tile_size - 1) / tile_size;
    return tile_count * dimension * ((tile_size + 1) / 2);
}

std::uint8_t tiled_soa_code_at(
    std::span<const std::uint8_t> packed,
    std::size_t dimension,
    std::size_t tile_size,
    std::size_t vector_index,
    std::size_t dimension_index) {
    const std::size_t bytes_per_dimension = (tile_size + 1) / 2;
    const std::size_t tile_stride = dimension * bytes_per_dimension;
    const std::size_t tile_index = vector_index / tile_size;
    const std::size_t lane = vector_index % tile_size;
    const std::size_t offset = tile_index * tile_stride
        + dimension_index * bytes_per_dimension + lane / 2;
    const unsigned shift = (lane % 2 == 0) ? 0U : 4U;
    return static_cast<std::uint8_t>((packed[offset] >> shift) & 0x0FU);
}

void require_tiled_soa_size(
    std::span<const std::uint8_t> packed,
    std::size_t vector_count,
    std::size_t dimension,
    std::size_t tile_size) {
    if (packed.size() < tiled_soa_required_size(vector_count, dimension, tile_size)) {
        throw std::invalid_argument("packed tiled SoA input is too short");
    }
}

}  // namespace

std::vector<std::uint8_t> pack_tiled_soa_nibbles(
    const std::vector<std::vector<std::uint8_t>>& vectors,
    std::size_t tile_size) {
    if (vectors.empty() || vectors.front().empty()) {
        throw std::invalid_argument("tiled SoA input must contain vectors and dimensions");
    }
    const std::size_t dimension = vectors.front().size();
    std::vector<std::uint8_t> packed(
        tiled_soa_required_size(vectors.size(), dimension, tile_size), 0);
    const std::size_t bytes_per_dimension = (tile_size + 1) / 2;
    const std::size_t tile_stride = dimension * bytes_per_dimension;
    for (std::size_t vector_index = 0; vector_index < vectors.size(); ++vector_index) {
        if (vectors[vector_index].size() != dimension) {
            throw std::invalid_argument("tiled SoA vectors must have equal dimensions");
        }
        const std::size_t tile_index = vector_index / tile_size;
        const std::size_t lane = vector_index % tile_size;
        for (std::size_t dimension_index = 0; dimension_index < dimension; ++dimension_index) {
            const std::uint8_t code = vectors[vector_index][dimension_index];
            if (code > 15) {
                throw std::invalid_argument("tiled SoA nibble code exceeds 15");
            }
            const std::size_t offset = tile_index * tile_stride
                + dimension_index * bytes_per_dimension + lane / 2;
            const unsigned shift = (lane % 2 == 0) ? 0U : 4U;
            packed[offset] |= static_cast<std::uint8_t>(code << shift);
        }
    }
    return packed;
}

std::vector<float> scan_packed_cartesian_tiled_soa(
    std::span<const std::uint8_t> packed_codes,
    std::size_t vector_count,
    std::size_t dimension,
    std::size_t tile_size,
    std::span<const float> query) {
    require_tiled_soa_size(packed_codes, vector_count, dimension, tile_size);
    if (query.size() != dimension) {
        throw std::invalid_argument("tiled SoA cartesian dimensions do not match");
    }
    std::vector<float> scores(vector_count, 0.0F);
    for (std::size_t tile_begin = 0; tile_begin < vector_count; tile_begin += tile_size) {
        const std::size_t tile_end = std::min(vector_count, tile_begin + tile_size);
        for (std::size_t dimension_index = 0; dimension_index < dimension; ++dimension_index) {
            for (std::size_t vector_index = tile_begin; vector_index < tile_end; ++vector_index) {
                const std::uint8_t code = tiled_soa_code_at(
                    packed_codes, dimension, tile_size, vector_index, dimension_index);
                const float value = (static_cast<float>(code) - 7.5F) / 7.5F;
                scores[vector_index] += query[dimension_index] * value;
            }
        }
    }
    return scores;
}

float score_packed_cartesian_tiled_soa_at(
    std::span<const std::uint8_t> packed_codes,
    std::size_t vector_count,
    std::size_t dimension,
    std::size_t tile_size,
    std::size_t vector_index,
    std::span<const float> query) {
    require_tiled_soa_size(packed_codes, vector_count, dimension, tile_size);
    if (query.size() != dimension || vector_index >= vector_count) {
        throw std::invalid_argument("tiled SoA random access dimensions do not match");
    }
    float score = 0.0F;
    for (std::size_t dimension_index = 0; dimension_index < dimension; ++dimension_index) {
        const std::uint8_t code = tiled_soa_code_at(
            packed_codes, dimension, tile_size, vector_index, dimension_index);
        const float value = (static_cast<float>(code) - 7.5F) / 7.5F;
        score += query[dimension_index] * value;
    }
    return score;
}

std::vector<std::uint8_t> pack_nibbles(std::span<const std::uint8_t> codes) {
    std::vector<std::uint8_t> packed((codes.size() + 1) / 2, 0);
    for (std::size_t i = 0; i < codes.size(); ++i) {
        if (codes[i] > 15) {
            throw std::invalid_argument("nibble code exceeds 15");
        }
        const std::size_t byte_index = i / 2;
        const unsigned shift = (i % 2 == 0) ? 0U : 4U;
        packed[byte_index] |= static_cast<std::uint8_t>(codes[i] << shift);
    }
    return packed;
}

std::vector<std::uint8_t> unpack_nibbles(
    std::span<const std::uint8_t> packed,
    std::size_t code_count) {
    if (packed.size() < (code_count + 1) / 2) {
        throw std::invalid_argument("packed input is too short");
    }

    std::vector<std::uint8_t> codes(code_count, 0);
    for (std::size_t i = 0; i < code_count; ++i) {
        const std::uint8_t byte = packed[i / 2];
        const unsigned shift = (i % 2 == 0) ? 0U : 4U;
        codes[i] = static_cast<std::uint8_t>((byte >> shift) & 0x0FU);
    }
    return codes;
}

}  // namespace spherra_bench
