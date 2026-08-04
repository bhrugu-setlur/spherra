#include "codec.hpp"

#include <cmath>
#include <cstdlib>
#include <iostream>
#include <vector>

namespace {

void expect_near(float actual, float expected, float tolerance, const char* label) {
    if (std::fabs(actual - expected) > tolerance) {
        std::cerr << label << ": expected " << expected << ", got " << actual << '\n';
        std::exit(1);
    }
}

void test_hyperspherical_decode_matches_known_vector() {
    const std::vector<float> angles{static_cast<float>(M_PI_2), 0.0F};
    const auto decoded = spherra_bench::decode_hyperspherical(angles);

    expect_near(decoded.at(0), 0.0F, 1e-6F, "x");
    expect_near(decoded.at(1), 1.0F, 1e-6F, "y");
    expect_near(decoded.at(2), 0.0F, 1e-6F, "z");
}

void test_recursive_score_matches_decoded_dot_product() {
    const std::vector<float> angles{1.1F, 2.2F, 0.7F};
    const std::vector<float> query{0.25F, -0.5F, 0.75F, 0.125F};
    const auto decoded = spherra_bench::decode_hyperspherical(angles);

    float expected = 0.0F;
    for (std::size_t i = 0; i < query.size(); ++i) {
        expected += decoded.at(i) * query.at(i);
    }

    const float actual = spherra_bench::score_hyperspherical(angles, query);
    expect_near(actual, expected, 1e-6F, "recursive score");
}

void test_nibbles_round_trip() {
    const std::vector<std::uint8_t> codes{0, 1, 7, 8, 14, 15};
    const auto packed = spherra_bench::pack_nibbles(codes);
    const auto unpacked = spherra_bench::unpack_nibbles(packed, codes.size());

    if (unpacked != codes) {
        std::cerr << "nibble round trip failed\n";
        std::exit(1);
    }
}

void test_quantized_angle_score_uses_recursive_prefix() {
    spherra_bench::AngleTables tables(2);
    tables.set(0, 0, 0.0F, 1.0F);
    tables.set(1, 1, 1.0F, 0.0F);

    const std::vector<std::uint8_t> codes{0, 1};
    const std::vector<float> query{2.0F, 3.0F, 4.0F};
    const float score = spherra_bench::score_quantized_angles(codes, query, tables);
    expect_near(score, 3.0F, 1e-6F, "quantized angle score");
}

void test_cartesian_int4_score_maps_endpoints_to_unit_range() {
    const std::vector<std::uint8_t> codes{15, 0};
    const std::vector<float> query{2.0F, 3.0F};
    const float score = spherra_bench::score_cartesian_int4(codes, query);
    expect_near(score, -1.0F, 1e-6F, "cartesian int4 score");
}

void test_packed_scores_match_unpacked_scores() {
    spherra_bench::AngleTables tables(2);
    tables.set(0, 0, 0.0F, 1.0F);
    tables.set(1, 1, 1.0F, 0.0F);

    const std::vector<std::uint8_t> angle_codes{0, 1};
    const auto packed_angles = spherra_bench::pack_nibbles(angle_codes);
    const std::vector<float> angle_query{2.0F, 3.0F, 4.0F};
    expect_near(
        spherra_bench::score_packed_quantized_angles(
            packed_angles, angle_codes.size(), angle_query, tables),
        3.0F,
        1e-6F,
        "packed angle score");

    const std::vector<std::uint8_t> cartesian_codes{15, 0};
    const auto packed_cartesian = spherra_bench::pack_nibbles(cartesian_codes);
    const std::vector<float> cartesian_query{2.0F, 3.0F};
    expect_near(
        spherra_bench::score_packed_cartesian_int4(
            packed_cartesian, cartesian_codes.size(), cartesian_query),
        -1.0F,
        1e-6F,
        "packed cartesian score");
}

void test_soa_scans_match_individual_scores() {
    spherra_bench::AngleTables tables(2);
    tables.set(0, 0, 0.0F, 1.0F);
    tables.set(0, 1, 1.0F, 0.0F);
    tables.set(1, 0, 1.0F, 0.0F);
    tables.set(1, 1, 0.0F, 1.0F);

    const std::vector<std::vector<std::uint8_t>> angle_codes{{0, 0}, {1, 1}};
    const auto packed_angles = spherra_bench::pack_soa_nibbles(angle_codes);
    const std::vector<float> angle_query{2.0F, 3.0F, 4.0F};
    const auto angle_scores = spherra_bench::scan_packed_angles_soa(
        packed_angles, angle_codes.size(), 2, angle_query, tables);
    expect_near(angle_scores.at(0), 3.0F, 1e-6F, "soa angle score zero");
    expect_near(angle_scores.at(1), 2.0F, 1e-6F, "soa angle score one");

    const std::vector<std::vector<std::uint8_t>> cartesian_codes{{15, 0}, {0, 15}};
    const auto packed_cartesian = spherra_bench::pack_soa_nibbles(cartesian_codes);
    const std::vector<float> cartesian_query{2.0F, 3.0F};
    const auto cartesian_scores = spherra_bench::scan_packed_cartesian_soa(
        packed_cartesian, cartesian_codes.size(), 2, cartesian_query);
    expect_near(cartesian_scores.at(0), -1.0F, 1e-6F, "soa cartesian score zero");
    expect_near(cartesian_scores.at(1), 1.0F, 1e-6F, "soa cartesian score one");
}

void test_tiled_soa_supports_scan_and_random_access() {
    const std::vector<std::vector<std::uint8_t>> codes{
        {15, 0}, {0, 15}, {15, 15}, {0, 0}, {8, 7}};
    const std::vector<float> query{2.0F, 3.0F};
    constexpr std::size_t tile_size = 4;
    const auto packed = spherra_bench::pack_tiled_soa_nibbles(codes, tile_size);
    const auto scores = spherra_bench::scan_packed_cartesian_tiled_soa(
        packed, codes.size(), 2, tile_size, query);

    for (std::size_t index = 0; index < codes.size(); ++index) {
        const float expected = spherra_bench::score_cartesian_int4(codes[index], query);
        expect_near(scores.at(index), expected, 1e-6F, "tiled scan score");
        expect_near(
            spherra_bench::score_packed_cartesian_tiled_soa_at(
                packed, codes.size(), 2, tile_size, index, query),
            expected,
            1e-6F,
            "tiled random score");
    }
}

}  // namespace

int main() {
    test_hyperspherical_decode_matches_known_vector();
    test_recursive_score_matches_decoded_dot_product();
    test_nibbles_round_trip();
    test_quantized_angle_score_uses_recursive_prefix();
    test_cartesian_int4_score_maps_endpoints_to_unit_range();
    test_packed_scores_match_unpacked_scores();
    test_soa_scans_match_individual_scores();
    test_tiled_soa_supports_scan_and_random_access();
    std::cout << "codec tests passed\n";
    return 0;
}
