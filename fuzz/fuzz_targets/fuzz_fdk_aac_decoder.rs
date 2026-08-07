//! デコーダーに対する fuzzing ターゲット
//!
//! 任意のバイト列を Audio Specific Config と圧縮パケットに解釈して、
//! デコードを行い、パニックが発生しないことを検証する。

#![no_main]

use libfuzzer_sys::fuzz_target;

use shiguredo_fdk_aac::{Decoder, FdkAacLibrary};

// FDK AAC 共有ライブラリをロードする
//
// 環境変数 `FDK_AAC_PATH` でパスを指定できる。
// 未指定の場合はシステムのライブラリ検索パスから `libfdk-aac.so.2` を探索する。
// ライブラリがロードできない場合は fuzzing を継続できないため panic する。
fn load_library() -> FdkAacLibrary {
    let path = std::env::var("FDK_AAC_PATH").unwrap_or_else(|_| "libfdk-aac.so.2".to_string());
    FdkAacLibrary::load(path).expect("FDK AAC 共有ライブラリのロードに失敗した")
}

// AAC-LC 48kHz ステレオの有効な Audio Specific Config
const VALID_ASC: [u8; 2] = [0x11, 0x90];

fuzz_target!(|data: &[u8]| {
    // 先頭バイトで Audio Specific Config の生成方法を決める
    let Some((&asc_mode, payload)) = data.split_first() else {
        return;
    };

    let lib = load_library();

    // 有効な ASC と、入力バイト列から作った不正な ASC の両方を検証する
    let asc: &[u8] = match asc_mode & 0b1 {
        0 => &VALID_ASC,
        _ => &payload[..payload.len().min(4)],
    };

    // ASC が不正な場合はエラーを返すため、その場合はスキップする
    let Ok(mut decoder) = Decoder::new(lib, asc) else {
        return;
    };

    // 残りのバイト列を 512 バイト単位のパケットとしてデコードする
    for packet in payload.chunks(512) {
        let Ok(()) = decoder.decode(packet) else {
            return;
        };

        // キューにあるパケットをすべてデコードして取り出す
        loop {
            let Ok(Some(_decoded)) = decoder.next_frame() else {
                break;
            };
        }
    }
});
