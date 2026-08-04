#include "codec.hpp"

#include <algorithm>
#include <chrono>
#include <cmath>
#include <cstddef>
#include <cstdint>
#include <cstdlib>
#include <iomanip>
#include <iostream>
#include <numeric>
#include <random>
#include <span>
#include <string_view>
#include <vector>

namespace {

constexpr std::size_t kDimension = 768;
constexpr std::size_t kAngleCount = kDimension - 1;

struct Measurement {
    double seconds;
    double vectors_per_second;
    float checksum;
};

template <typename Function>
Measurement measure(std::size_t vector_count, Function&& function) {
    function();
    constexpr int repetitions = 5;
    double best_seconds = std::numeric_limits<double>::max();
    float checksum = 0.0F;
    for (int repetition = 0; repetition < repetitions; ++repetition) {
        const auto start = std::chrono::steady_clock::now();
        checksum += function();
        const auto stop = std::chrono::steady_clock::now();
        const double seconds = std::chrono::duration<double>(stop - start).count();
        best_seconds = std::min(best_seconds, seconds);
    }
    return {best_seconds, static_cast<double>(vector_count) / best_seconds, checksum};
}

spherra_bench::AngleTables make_angle_tables() {
    spherra_bench::AngleTables tables(kAngleCount);
    for (std::size_t angle = 0; angle < kAngleCount; ++angle) {
        const float range = (angle + 1 == kAngleCount)
            ? static_cast<float>(2.0 * M_PI)
            : static_cast<float>(M_PI);
        for (std::uint8_t code = 0; code < 16; ++code) {
            const float theta = (static_cast<float>(code) + 0.5F) * range / 16.0F;
            tables.set(angle, code, std::cos(theta), std::sin(theta));
        }
    }
    return tables;
}

std::vector<float> make_query(std::mt19937& random) {
    std::normal_distribution<float> normal(0.0F, 1.0F);
    std::vector<float> query(kDimension);
    float norm_squared = 0.0F;
    for (float& value : query) {
        value = normal(random);
        norm_squared += value * value;
    }
    const float inverse_norm = 1.0F / std::sqrt(norm_squared);
    for (float& value : query) {
        value *= inverse_norm;
    }
    return query;
}

std::vector<std::vector<std::uint8_t>> make_angle_codes(
    std::size_t vector_count,
    std::mt19937& random) {
    std::uniform_int_distribution<int> near_equator(6, 9);
    std::uniform_int_distribution<int> last_angle(0, 15);
    std::vector<std::vector<std::uint8_t>> codes(
        vector_count, std::vector<std::uint8_t>(kAngleCount));
    for (auto& vector : codes) {
        for (std::size_t angle = 0; angle < kAngleCount; ++angle) {
            vector[angle] = static_cast<std::uint8_t>(
                angle + 1 == kAngleCount ? last_angle(random) : near_equator(random));
        }
    }
    return codes;
}

std::vector<std::vector<std::uint8_t>> make_cartesian_codes(
    std::size_t vector_count,
    std::mt19937& random) {
    std::uniform_int_distribution<int> code(0, 15);
    std::vector<std::vector<std::uint8_t>> codes(
        vector_count, std::vector<std::uint8_t>(kDimension));
    for (auto& vector : codes) {
        for (std::uint8_t& value : vector) {
            value = static_cast<std::uint8_t>(code(random));
        }
    }
    return codes;
}

std::vector<std::uint8_t> pack_aos(
    const std::vector<std::vector<std::uint8_t>>& vectors) {
    const std::size_t bytes_per_vector = (vectors.front().size() + 1) / 2;
    std::vector<std::uint8_t> packed(vectors.size() * bytes_per_vector);
    for (std::size_t i = 0; i < vectors.size(); ++i) {
        const auto one = spherra_bench::pack_nibbles(vectors[i]);
        std::copy(one.begin(), one.end(), packed.begin() + i * bytes_per_vector);
    }
    return packed;
}

void print_measurement(std::string_view name, const Measurement& measurement) {
    std::cout << std::left << std::setw(32) << name
              << " seconds=" << std::fixed << std::setprecision(6) << measurement.seconds
              << " vectors_per_second=" << std::setprecision(0)
              << measurement.vectors_per_second
              << " checksum=" << std::setprecision(6) << measurement.checksum << '\n';
}

}  // namespace

int main(int argc, char** argv) {
    const std::size_t vector_count = argc > 1
        ? static_cast<std::size_t>(std::strtoull(argv[1], nullptr, 10))
        : 131072;
    if (vector_count == 0) {
        std::cerr << "vector count must be positive\n";
        return 1;
    }

    std::mt19937 random(20260804);
    const auto query = make_query(random);
    const auto tables = make_angle_tables();
    auto angle_codes = make_angle_codes(vector_count, random);
    auto cartesian_codes = make_cartesian_codes(vector_count, random);
    const auto angle_aos = pack_aos(angle_codes);
    const auto cartesian_aos = pack_aos(cartesian_codes);
    const auto angle_soa = spherra_bench::pack_soa_nibbles(angle_codes);
    const auto cartesian_soa = spherra_bench::pack_soa_nibbles(cartesian_codes);
    const auto cartesian_tiled_32 = spherra_bench::pack_tiled_soa_nibbles(
        cartesian_codes, 32);
    const auto cartesian_tiled_64 = spherra_bench::pack_tiled_soa_nibbles(
        cartesian_codes, 64);
    angle_codes.clear();
    angle_codes.shrink_to_fit();
    cartesian_codes.clear();
    cartesian_codes.shrink_to_fit();

    std::vector<std::size_t> random_order(vector_count);
    std::iota(random_order.begin(), random_order.end(), 0);
    std::shuffle(random_order.begin(), random_order.end(), random);

    const std::size_t angle_bytes = (kAngleCount + 1) / 2;
    const std::size_t cartesian_bytes = (kDimension + 1) / 2;

    std::cout << "vectors=" << vector_count << " dimension=" << kDimension << '\n';

    print_measurement("angular_aos_sequential", measure(vector_count, [&] {
        float checksum = 0.0F;
        for (std::size_t i = 0; i < vector_count; ++i) {
            const std::span<const std::uint8_t> one(
                angle_aos.data() + i * angle_bytes, angle_bytes);
            checksum += spherra_bench::score_packed_quantized_angles(
                one, kAngleCount, query, tables);
        }
        return checksum;
    }));

    print_measurement("cartesian_aos_sequential", measure(vector_count, [&] {
        float checksum = 0.0F;
        for (std::size_t i = 0; i < vector_count; ++i) {
            const std::span<const std::uint8_t> one(
                cartesian_aos.data() + i * cartesian_bytes, cartesian_bytes);
            checksum += spherra_bench::score_packed_cartesian_int4(
                one, kDimension, query);
        }
        return checksum;
    }));

    print_measurement("angular_aos_random", measure(vector_count, [&] {
        float checksum = 0.0F;
        for (const std::size_t i : random_order) {
            const std::span<const std::uint8_t> one(
                angle_aos.data() + i * angle_bytes, angle_bytes);
            checksum += spherra_bench::score_packed_quantized_angles(
                one, kAngleCount, query, tables);
        }
        return checksum;
    }));

    print_measurement("cartesian_aos_random", measure(vector_count, [&] {
        float checksum = 0.0F;
        for (const std::size_t i : random_order) {
            const std::span<const std::uint8_t> one(
                cartesian_aos.data() + i * cartesian_bytes, cartesian_bytes);
            checksum += spherra_bench::score_packed_cartesian_int4(
                one, kDimension, query);
        }
        return checksum;
    }));

    print_measurement("angular_soa_scan", measure(vector_count, [&] {
        const auto scores = spherra_bench::scan_packed_angles_soa(
            angle_soa, vector_count, kAngleCount, query, tables);
        return std::accumulate(scores.begin(), scores.end(), 0.0F);
    }));

    print_measurement("cartesian_soa_scan", measure(vector_count, [&] {
        const auto scores = spherra_bench::scan_packed_cartesian_soa(
            cartesian_soa, vector_count, kDimension, query);
        return std::accumulate(scores.begin(), scores.end(), 0.0F);
    }));

    print_measurement("cartesian_tiled32_scan", measure(vector_count, [&] {
        const auto scores = spherra_bench::scan_packed_cartesian_tiled_soa(
            cartesian_tiled_32, vector_count, kDimension, 32, query);
        return std::accumulate(scores.begin(), scores.end(), 0.0F);
    }));

    print_measurement("cartesian_tiled64_scan", measure(vector_count, [&] {
        const auto scores = spherra_bench::scan_packed_cartesian_tiled_soa(
            cartesian_tiled_64, vector_count, kDimension, 64, query);
        return std::accumulate(scores.begin(), scores.end(), 0.0F);
    }));

    print_measurement("cartesian_tiled32_random", measure(vector_count, [&] {
        float checksum = 0.0F;
        for (const std::size_t i : random_order) {
            checksum += spherra_bench::score_packed_cartesian_tiled_soa_at(
                cartesian_tiled_32, vector_count, kDimension, 32, i, query);
        }
        return checksum;
    }));

    print_measurement("cartesian_tiled64_random", measure(vector_count, [&] {
        float checksum = 0.0F;
        for (const std::size_t i : random_order) {
            checksum += spherra_bench::score_packed_cartesian_tiled_soa_at(
                cartesian_tiled_64, vector_count, kDimension, 64, i, query);
        }
        return checksum;
    }));

    return 0;
}
