//! src/lib.rs に対応する PBT (Property-Based Testing)
//!
//! 実機の FDK AAC 共有ライブラリを使って、エンコード → デコードの
//! ラウンドトリップの整合性を任意入力に対して検証する。
//! モックやスタブは使わず、必ず実ライブラリを対象に検証する。

use std::sync::Mutex;

use proptest::prelude::*;

use shiguredo_fdk_aac::{Decoder, Encoder, EncoderConfig, FdkAacLibrary};

// FDK AAC の C ライブラリはグローバル状態を持つため、
// テストの並列実行で double free が発生する。
// Mutex で直列化して安全に実行する。
static TEST_LOCK: Mutex<()> = Mutex::new(());

// テスト用の共有ライブラリをロードする
//
// 環境変数 `FDK_AAC_PATH` でパスを指定できる。
// 未指定の場合はシステムのライブラリ検索パスから `libfdk-aac.so.2` を探索する。
fn load_library() -> FdkAacLibrary {
    let path = std::env::var("FDK_AAC_PATH").unwrap_or_else(|_| "libfdk-aac.so.2".to_string());
    FdkAacLibrary::load(path).expect("ライブラリのロードに失敗した")
}

// FDK AAC エンコーダーがサポートするサンプルレート
const SUPPORTED_SAMPLE_RATES: [u32; 5] = [8000, 16000, 44100, 48000, 96000];

// AAC-LC の 1 フレームあたりのサンプル数（チャンネルあたり）
const FRAME_LEN: usize = 1024;

// エンコーダー設定のストラテジー
//
// サポートされているサンプルレートとチャンネル数から選ぶ。
fn encoder_config_strategy() -> impl Strategy<Value = EncoderConfig> {
    (
        prop::sample::select(&SUPPORTED_SAMPLE_RATES),
        prop::bool::ANY,
    )
        .prop_map(|(sample_rate, is_stereo)| EncoderConfig {
            sample_rate,
            channels: if is_stereo { 2 } else { 1 },
            bitrate: Some(96_000),
        })
}

// 任意の長さの PCM 入力（インターリーブ形式）のストラテジー
fn pcm_strategy() -> impl Strategy<Value = Vec<i16>> {
    prop::collection::vec(any::<i16>(), 0..4096)
}

/// 任意の PCM 入力をエンコード → デコードしても、
/// フレーム構造とサンプル数が整合することを検証する
///
/// - デコードされたサンプル数は入力サンプル数以下であること
/// - デコードされたサンプル数は入力サンプル数より 1 フレーム分以上少なくないこと
///   （末尾のパーシャルフレームはエンコーダーの内部バッファに残り出力されないため）
/// - 各デコードフレームのメタデータが設定と一致すること
#[test]
fn prop_roundtrip() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let lib = load_library();

    let mut runner = proptest::test_runner::TestRunner::new(ProptestConfig {
        cases: 128,
        ..ProptestConfig::default()
    });

    // エンコーダーとデコーダーは設定ごとに生成し直す
    runner
        .run(&(encoder_config_strategy(), pcm_strategy()), |(config, pcm)| {
            // エンコード
            let mut encoder =
                Encoder::new(lib.clone(), config.clone()).expect("エンコーダーの生成に失敗した");
            encoder.encode(&pcm).expect("エンコードに失敗した");
            encoder.finish().expect("finish に失敗した");

            let mut encoded_frames = Vec::new();
            while let Some(frame) = encoder.next_frame() {
                encoded_frames.push(frame);
            }

            // デコード
            let asc = encoder.audio_specific_config();
            assert!(
                !asc.is_empty(),
                "Audio Specific Config が空である"
            );

            let mut decoder =
                Decoder::new(lib.clone(), asc).expect("デコーダーの生成に失敗した");
            for frame in &encoded_frames {
                decoder.decode(&frame.data).expect("デコードに失敗した");
            }
            decoder.finish().expect("finish に失敗した");

            let mut decoded_per_channel = 0_usize;
            while let Some(decoded) = decoder.next_frame().expect("next_frame に失敗した") {
                // デコードフレームのメタデータが設定と一致する
                prop_assert_eq!(decoded.channels, config.channels);
                prop_assert_eq!(decoded.sample_rate, config.sample_rate);
                // AAC-LC のデコードフレームは常に 1 フレーム分のサンプル数になる
                prop_assert_eq!(decoded.samples, FRAME_LEN);
                // データ長はサンプル数 × チャンネル数と一致する
                prop_assert_eq!(
                    decoded.data.len(),
                    decoded.samples * decoded.channels as usize
                );
                decoded_per_channel += decoded.samples;
            }

            // チャンネル数で割り切れない入力を考慮して、チャンネルあたりのサンプル数を求める
            let input_per_channel = pcm.len() / config.channels as usize;

            // エンコーダーが入力以上のサンプル数を出力することはない
            prop_assert!(
                decoded_per_channel <= input_per_channel,
                "デコードされたサンプル数 ({decoded_per_channel}) が入力サンプル数 ({input_per_channel}) を超えている"
            );
            // 末尾のパーシャルフレームは出力されないため、1 フレーム分の差は許容する
            prop_assert!(
                decoded_per_channel + FRAME_LEN >= input_per_channel,
                "デコードされたサンプル数 ({decoded_per_channel}) が入力サンプル数 ({input_per_channel}) より 1 フレーム分以上少ない"
            );

            Ok(())
        })
        .expect("PBT に失敗した");
}

/// サポートされているサンプルレートとチャンネル数なら、
/// エンコーダーの初期化が常に成功することを検証する
///
/// 任意のビットレートを指定してもエンコーダーの初期化に失敗しないことと、
/// Audio Specific Config が空にならないことを確認する。
#[test]
fn prop_encoder_config() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let lib = load_library();

    let mut runner = proptest::test_runner::TestRunner::new(ProptestConfig {
        cases: 128,
        ..ProptestConfig::default()
    });

    // ビットレートは任意の値を指定する
    let bitrate = proptest::num::u32::ANY;

    runner
        .run(
            &(encoder_config_strategy(), bitrate),
            |(config, bitrate)| {
                let config = EncoderConfig {
                    bitrate: Some(bitrate),
                    ..config
                };

                let encoder =
                    Encoder::new(lib.clone(), config).expect("エンコーダーの生成に失敗した");

                // Audio Specific Config が空にならないこと
                prop_assert!(!encoder.audio_specific_config().is_empty());

                Ok(())
            },
        )
        .expect("PBT に失敗した");
}
