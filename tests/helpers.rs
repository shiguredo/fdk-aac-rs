//! テスト間で共有するヘルパー
//!
//! 実機の FDK AAC 共有ライブラリをロードしてテストするための共通処理をまとめる。
//! モックやスタブは使わず、必ず実ライブラリを対象にテストする。

use std::sync::Mutex;

use shiguredo_fdk_aac::{EncoderConfig, FdkAacLibrary};

/// テストで使用するサンプルレート
pub const TEST_SAMPLE_RATE: u32 = 48000;

/// テストで使用するチャンネル数
pub const TEST_CHANNELS: u8 = 2;

// FDK AAC の C ライブラリはグローバル状態を持つため、
// テストの並列実行で double free が発生する。
// Mutex で直列化して安全に実行する。
static TEST_LOCK: Mutex<()> = Mutex::new(());

/// テストの並列実行による FDK AAC のグローバル状態の競合を防ぐためのロックを取得する
///
/// 各テストはこの戻り値の `_guard` を関数スコープで保持すること。
/// Mutex が汚染されている場合（パニックが発生した場合）でも、
/// `into_inner()` でロックを回収してテストを継続できるようにする。
pub fn lock() -> std::sync::MutexGuard<'static, ()> {
    TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// テスト用の共有ライブラリをロードする
///
/// 環境変数 `FDK_AAC_PATH` でパスを指定できる。
/// 未指定の場合はシステムのライブラリ検索パスから `libfdk-aac.so.2` を探索する。
pub fn load_library() -> FdkAacLibrary {
    let path = std::env::var("FDK_AAC_PATH").unwrap_or_else(|_| "libfdk-aac.so.2".to_string());
    FdkAacLibrary::load(path).expect("ライブラリのロードに失敗した")
}

/// テスト用のエンコーダー設定を作成する
pub fn encoder_config(bitrate: Option<u32>) -> EncoderConfig {
    EncoderConfig {
        sample_rate: TEST_SAMPLE_RATE,
        channels: TEST_CHANNELS,
        bitrate,
    }
}
