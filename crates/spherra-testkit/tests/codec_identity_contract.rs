use spherra_testkit::harness::{M1_CODEC_ID, codec_id_hex};

#[test]
fn legacy_codec_identity_name_is_the_same_static_and_keeps_its_hex() {
    assert!(std::ptr::eq(&M1_CODEC_ID, &spherra_codec::CODEC_ID));
    assert_eq!(
        codec_id_hex(),
        "0ceb4208a53bf92938a513426e8a0528c429a9f91ca345ce9ad679df6114df73"
    );
}
