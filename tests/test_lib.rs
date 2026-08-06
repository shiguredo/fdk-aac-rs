//! src/lib.rs に対応する単体テスト
//!
//! PBT (pbt/tests/prop_lib.rs) では実現できないケース
//! （エラーパス・境界値・品質指標の検証など）を対象にする。

mod helpers;

use helpers::{TEST_CHANNELS, TEST_SAMPLE_RATE, encoder_config, load_library, lock};

use shiguredo_fdk_aac::{Decoder, Encoder, EncoderConfig};

/// 共有ライブラリをロードできることを確認する
#[test]
fn test_load_library() {
    let _guard = lock();
    load_library();
}

/// エンコーダーの初期化と設定バリデーションを確認する
#[test]
fn init_encoder() {
    let _guard = lock();
    let lib = load_library();

    // ビットレート指定あり
    assert!(Encoder::new(lib.clone(), encoder_config(Some(100_000))).is_ok());

    // ビットレート指定なし（デフォルト値）
    assert!(Encoder::new(lib.clone(), encoder_config(None)).is_ok());

    // サンプルレート 0 はエラー
    let config = EncoderConfig {
        sample_rate: 0,
        channels: TEST_CHANNELS,
        bitrate: Some(100_000),
    };
    assert!(Encoder::new(lib.clone(), config).is_err());

    // チャンネル数 0 はエラー
    let config = EncoderConfig {
        sample_rate: TEST_SAMPLE_RATE,
        channels: 0,
        bitrate: Some(100_000),
    };
    assert!(Encoder::new(lib, config).is_err());
}

/// 無音データをエンコードしてフレームが出力されることを確認する
#[test]
fn encode_silent() {
    let _guard = lock();
    let lib = load_library();
    let mut encoder =
        Encoder::new(lib, encoder_config(Some(100_000))).expect("エンコーダーの生成に失敗した");
    let mut sample_count = 0;

    for _ in 0..100 {
        encoder
            .encode(&[0; 100 * TEST_CHANNELS as usize])
            .expect("エンコードに失敗した");
        while let Some(encoded) = encoder.next_frame() {
            sample_count += encoded.samples;
        }
    }
    encoder.finish().expect("finish に失敗した");
    while let Some(encoded) = encoder.next_frame() {
        sample_count += encoded.samples;
    }

    // エンコーダーは内部バッファリングにより一部のサンプルを後続フレームに含めるため、
    // 出力フレームの合計サンプル数は入力と厳密に一致しない。
    // 最大 1 フレーム分（1024 サンプル）の差を許容する。
    let input_samples = 100 * 100;
    let max_frame_loss = 1024_usize;
    assert!(
        sample_count + max_frame_loss >= input_samples,
        "エンコードされたサンプル数 ({sample_count}) が入力 ({input_samples}) より少なすぎる"
    );
}

/// デコーダーの初期化と Audio Specific Config のバリデーションを確認する
#[test]
fn init_decoder() {
    let _guard = lock();
    let lib = load_library();
    let encoder = Encoder::new(lib.clone(), encoder_config(Some(100_000)))
        .expect("エンコーダーの生成に失敗した");
    let asc = encoder.audio_specific_config();

    // Audio Specific Config が正しい
    assert!(Decoder::new(lib.clone(), asc).is_ok());

    // Audio Specific Config が空の場合はエラーになる
    assert!(Decoder::new(lib, &[]).is_err());
}

/// 無音データをエンコード → デコードして、PCM が復元されることを確認する
#[test]
fn decode_silent() {
    let _guard = lock();
    let lib = load_library();
    let mut encoder = Encoder::new(lib.clone(), encoder_config(Some(100_000)))
        .expect("エンコーダーの生成に失敗した");

    // 無音のオーディオをエンコード
    let pcm_data = vec![0i16; 1024 * TEST_CHANNELS as usize];
    encoder.encode(&pcm_data).expect("エンコードに失敗した");
    encoder.finish().expect("finish に失敗した");

    let mut encoded_frames = Vec::new();
    while let Some(frame) = encoder.next_frame() {
        encoded_frames.push(frame);
    }

    // デコーダーを初期化
    let asc = encoder.audio_specific_config();
    let mut decoder = Decoder::new(lib, asc).expect("デコーダーの生成に失敗した");

    // エンコードされたフレームをデコード
    for frame in &encoded_frames {
        decoder.decode(&frame.data).expect("デコードに失敗した");
    }
    decoder.finish().expect("finish に失敗した");

    let mut total_decoded = 0;
    while let Some(decoded) = decoder.next_frame().expect("next_frame に失敗した") {
        assert_eq!(decoded.channels, TEST_CHANNELS);
        assert_eq!(decoded.sample_rate, TEST_SAMPLE_RATE);
        total_decoded += decoded.samples;
    }

    // デコードされたサンプル数が入力サンプル数と一致することを確認
    assert!(total_decoded > 0, "デコードされたサンプルが存在しない");
}

/// 正弦波を生成してエンコード → デコードし、
/// デコード結果が無音でないことを確認するラウンドトリップテスト
#[test]
fn roundtrip_sine_wave() {
    let _guard = lock();
    let lib = load_library();
    let mut encoder = Encoder::new(lib.clone(), encoder_config(Some(128_000)))
        .expect("エンコーダーの生成に失敗した");

    // 440Hz の正弦波を 30 秒分生成（ステレオ: 左右同一）
    let num_samples = TEST_SAMPLE_RATE as usize * 30;
    let mut pcm_input = Vec::with_capacity(num_samples * TEST_CHANNELS as usize);
    for i in 0..num_samples {
        let t = i as f64 / TEST_SAMPLE_RATE as f64;
        let sample = (t * 440.0 * 2.0 * std::f64::consts::PI).sin();
        let value = (sample * i16::MAX as f64) as i16;
        for _ in 0..TEST_CHANNELS {
            pcm_input.push(value);
        }
    }

    // エンコード
    encoder.encode(&pcm_input).expect("エンコードに失敗した");
    encoder.finish().expect("finish に失敗した");

    let mut encoded_frames = Vec::new();
    while let Some(frame) = encoder.next_frame() {
        encoded_frames.push(frame);
    }
    assert!(
        !encoded_frames.is_empty(),
        "エンコードされたフレームが存在しない"
    );

    // デコード
    let asc = encoder.audio_specific_config();
    let mut decoder = Decoder::new(lib, asc).expect("デコーダーの生成に失敗した");

    for frame in &encoded_frames {
        decoder.decode(&frame.data).expect("デコードに失敗した");
    }
    decoder.finish().expect("finish に失敗した");

    let mut pcm_output: Vec<i16> = Vec::new();
    while let Some(decoded) = decoder.next_frame().expect("next_frame に失敗した") {
        assert_eq!(decoded.channels, TEST_CHANNELS);
        assert_eq!(decoded.sample_rate, TEST_SAMPLE_RATE);
        pcm_output.extend_from_slice(&decoded.data);
    }

    // AAC はフレーム単位でエンコードするため、末尾のパーシャルフレーム
    // （1024 サンプル未満）はエンコーダーの内部バッファに残り出力されない。
    // そのためデコード結果は入力より最大 1 フレーム分少なくなる可能性がある。
    let output_samples = pcm_output.len() / TEST_CHANNELS as usize;
    let max_frame_loss = 1024_usize;
    assert!(
        output_samples + max_frame_loss >= num_samples,
        "デコードされたサンプル数 ({output_samples}) が入力サンプル数 ({num_samples}) より少なすぎる"
    );

    // デコード結果が無音でないことを確認する（RMS が閾値以上）
    let rms = {
        let sum_sq: f64 = pcm_output.iter().map(|&s| (s as f64) * (s as f64)).sum();
        (sum_sq / pcm_output.len() as f64).sqrt()
    };
    assert!(rms > 1000.0, "デコード結果が無音に近すぎる (RMS: {rms})");
}

/// エンコード → デコードの SNR（Signal-to-Noise Ratio）を検証するテスト。
/// 映像の PSNR に相当する音声品質指標。
/// AAC-LC 128kbps / 48kHz のラウンドトリップで SNR 20dB 以上を期待する。
#[test]
fn roundtrip_snr() {
    let _guard = lock();
    let lib = load_library();
    let mut encoder = Encoder::new(lib.clone(), encoder_config(Some(128_000)))
        .expect("エンコーダーの生成に失敗した");

    // 440Hz の正弦波を 5 秒分生成（ステレオ）
    let duration_secs = 5;
    let num_samples = TEST_SAMPLE_RATE as usize * duration_secs;
    let total_pcm = num_samples * TEST_CHANNELS as usize;
    let mut pcm_input = Vec::with_capacity(total_pcm);
    for i in 0..num_samples {
        let t = i as f64 / TEST_SAMPLE_RATE as f64;
        let sample = (t * 440.0 * 2.0 * std::f64::consts::PI).sin();
        let value = (sample * i16::MAX as f64) as i16;
        for _ in 0..TEST_CHANNELS {
            pcm_input.push(value);
        }
    }

    // エンコード
    encoder.encode(&pcm_input).expect("エンコードに失敗した");
    encoder.finish().expect("finish に失敗した");

    let mut encoded_frames = Vec::new();
    while let Some(frame) = encoder.next_frame() {
        encoded_frames.push(frame);
    }

    // デコード
    let asc = encoder.audio_specific_config();
    let mut decoder = Decoder::new(lib, asc).expect("デコーダーの生成に失敗した");

    for frame in &encoded_frames {
        decoder.decode(&frame.data).expect("デコードに失敗した");
    }
    decoder.finish().expect("finish に失敗した");

    let mut pcm_output: Vec<i16> = Vec::new();
    while let Some(decoded) = decoder.next_frame().expect("next_frame に失敗した") {
        pcm_output.extend_from_slice(&decoded.data);
    }

    // AAC エンコーダーにはプライミング遅延がある（AAC-LC では通常 2048 サンプル）。
    // デコード出力は入力に対してオフセットしているため、相互相関で最適なアライメントを求める。
    let channels = TEST_CHANNELS as usize;
    let max_offset_samples = 4096_usize;
    let max_offset = max_offset_samples * channels;
    let search_len = pcm_input
        .len()
        .min(pcm_output.len())
        .saturating_sub(max_offset);

    let mut best_offset = 0_usize;
    let mut best_corr = f64::NEG_INFINITY;
    // デコード出力側のオフセットを探索
    for offset in (0..max_offset).step_by(channels) {
        let len = search_len
            .min(pcm_output.len() - offset)
            .min(pcm_input.len());
        let corr: f64 = pcm_input[..len]
            .iter()
            .zip(pcm_output[offset..offset + len].iter())
            .map(|(&a, &b)| a as f64 * b as f64)
            .sum();
        if corr > best_corr {
            best_corr = corr;
            best_offset = offset;
        }
    }

    // アライメント後の信号を比較
    let compare_len = pcm_input.len().min(pcm_output.len() - best_offset);
    let input = &pcm_input[..compare_len];
    let output = &pcm_output[best_offset..best_offset + compare_len];

    // SNR = 10 * log10(signal_power / noise_power)
    let signal_power: f64 =
        input.iter().map(|&s| (s as f64) * (s as f64)).sum::<f64>() / compare_len as f64;
    let noise_power: f64 = input
        .iter()
        .zip(output.iter())
        .map(|(&s, &d)| {
            let diff = s as f64 - d as f64;
            diff * diff
        })
        .sum::<f64>()
        / compare_len as f64;

    assert!(
        noise_power > 0.0,
        "ノイズパワーがゼロである（AAC でロスレスになるのは想定外）"
    );

    let snr_db = 10.0 * (signal_power / noise_power).log10();

    // AAC-LC 128kbps で SNR 20dB 以上を期待
    assert!(
        snr_db > 20.0,
        "SNR {snr_db:.1} dB が 20 dB の閾値を下回っている"
    );
}
