//! エンコード → デコードのラウンドトリップに対する fuzzing ターゲット
//!
//! 任意のバイト列をエンコーダー設定と PCM データに解釈して、
//! エンコード → デコードを行い、パニックが発生しないことを検証する。

#![no_main]

use libfuzzer_sys::fuzz_target;

use shiguredo_fdk_aac::{Decoder, Encoder, EncoderConfig, FdkAacLibrary};

// FDK AAC 共有ライブラリをロードする
//
// 環境変数 `FDK_AAC_PATH` でパスを指定できる。
// 未指定の場合はシステムのライブラリ検索パスから `libfdk-aac.so.2` を探索する。
// ライブラリがロードできない場合は fuzzing を継続できないため panic する。
fn load_library() -> FdkAacLibrary {
    let path = std::env::var("FDK_AAC_PATH").unwrap_or_else(|_| "libfdk-aac.so.2".to_string());
    FdkAacLibrary::load(path).expect("FDK AAC 共有ライブラリのロードに失敗した")
}

fuzz_target!(|data: &[u8]| {
    // 先頭バイトからエンコーダー設定を決める
    let Some((&config_byte, pcm_bytes)) = data.split_first() else {
        return;
    };

    // サンプルレート: 有効な値と無効な値の両方を混ぜてエラーパスも検証する
    let sample_rates = [8000u32, 16000, 44100, 48000, 96000, 0, 12345, 192000];
    let sample_rate = sample_rates[(config_byte & 0b111) as usize];

    // チャンネル数: 1 または 2
    let channels = if config_byte & 0b1000 == 0 { 1 } else { 2 };

    // ビットレート: 任意の値を指定してエラーパスも検証する
    let bitrate = Some(((config_byte >> 4) as u32) * 1000);

    let lib = load_library();
    let config = EncoderConfig {
        sample_rate,
        channels,
        bitrate,
    };

    // 設定が不正な場合はエラーを返すため、その場合はスキップする
    let Ok(mut encoder) = Encoder::new(lib.clone(), config) else {
        return;
    };

    // 残りのバイト列を i16 の PCM データとしてエンコードする
    let pcm: Vec<i16> = pcm_bytes
        .chunks_exact(2)
        .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]))
        .collect();

    let Ok(()) = encoder.encode(&pcm) else {
        return;
    };
    let Ok(()) = encoder.finish() else {
        return;
    };

    let mut encoded_frames = Vec::new();
    while let Some(frame) = encoder.next_frame() {
        encoded_frames.push(frame);
    }

    // デコーダーでデコードする
    let asc = encoder.audio_specific_config();
    let Ok(mut decoder) = Decoder::new(lib, asc) else {
        return;
    };

    for frame in &encoded_frames {
        let Ok(()) = decoder.decode(&frame.data) else {
            return;
        };
    }
    let Ok(()) = decoder.finish() else {
        return;
    };

    // デコード結果をすべて取り出す
    loop {
        let Ok(Some(_decoded)) = decoder.next_frame() else {
            break;
        };
    }
});
