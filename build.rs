use std::path::PathBuf;

// 環境変数で fdk-aac のインクルードパスを指定する場合のキー名
// デフォルト: /usr/include/fdk-aac/
//
// 基本的には未指定（デフォルト）で問題ないが、
// システムにインストールされている fdk-aac のパスが通常とは異なる場合には
// この環境変数を指定する必要がある
const ENV_FDK_AAC_INCLUDE_DIR: &str = "FDK_AAC_INCLUDE_DIR";

fn main() {
    println!("cargo::rerun-if-changed=Cargo.toml");
    println!("cargo::rerun-if-changed=build.rs");

    let out_dir = PathBuf::from(std::env::var_os("OUT_DIR").expect("infallible"));
    let output_bindings_path = out_dir.join("bindings.rs");

    if std::env::var("DOCS_RS").is_ok() {
        // docs.rs 向けのビルドではシステムの fdk-aac が参照できないので、
        // ドキュメント生成時に最低限必要な定義だけをダミーで出力する
        //
        // See also: https://docs.rs/about/builds
        std::fs::write(
            output_bindings_path,
            r#"
// 基本型
pub type UINT = ::std::os::raw::c_uint;
pub type INT = ::std::os::raw::c_int;

// エラー型
pub type AACENC_ERROR = UINT;
pub const AACENC_ERROR_AACENC_OK: AACENC_ERROR = 0;
pub const AACENC_ERROR_AACENC_INVALID_CONFIG: AACENC_ERROR = 0x22;
pub type AAC_DECODER_ERROR = UINT;
pub const AAC_DECODER_ERROR_AAC_DEC_OK: AAC_DECODER_ERROR = 0;
pub const AAC_DECODER_ERROR_AAC_DEC_NOT_ENOUGH_BITS: AAC_DECODER_ERROR = 0x2002;
pub const AAC_DECODER_ERROR_AAC_DEC_UNKNOWN: AAC_DECODER_ERROR = 0x5005;

// ハンドル型
#[repr(C)] pub struct AACENCODER { _unused: [u8; 0] }
pub type HANDLE_AACENCODER = *mut AACENCODER;
#[repr(C)] pub struct AAC_DECODER_INSTANCE { _unused: [u8; 0] }
pub type HANDLE_AACDECODER = *mut AAC_DECODER_INSTANCE;

// パラメータ型
pub type AACENC_PARAM = UINT;
pub const AACENC_PARAM_AACENC_AOT: AACENC_PARAM = 0x0100;
pub const AACENC_PARAM_AACENC_BITRATE: AACENC_PARAM = 0x0101;
pub const AACENC_PARAM_AACENC_SAMPLERATE: AACENC_PARAM = 0x0103;
pub const AACENC_PARAM_AACENC_CHANNELMODE: AACENC_PARAM = 0x0105;
pub const AACENC_PARAM_AACENC_CHANNELORDER: AACENC_PARAM = 0x0106;
pub const AACENC_PARAM_AACENC_AFTERBURNER: AACENC_PARAM = 0x0200;
pub const AACENC_PARAM_AACENC_TRANSMUX: AACENC_PARAM = 0x0302;

// トランスポート型
pub type TRANSPORT_TYPE = UINT;
pub const TRANSPORT_TYPE_TT_MP4_RAW: TRANSPORT_TYPE = 0;

// チャンネルモード
pub type CHANNEL_MODE = UINT;
pub const CHANNEL_MODE_MODE_1: CHANNEL_MODE = 1;
pub const CHANNEL_MODE_MODE_2: CHANNEL_MODE = 2;

// オーディオオブジェクト型
pub const AUDIO_OBJECT_TYPE_AOT_AAC_LC: UINT = 2;

// バッファ識別子
pub const AACENC_BufferIdentifier_IN_AUDIO_DATA: INT = 0;
pub const AACENC_BufferIdentifier_OUT_BITSTREAM_DATA: INT = 3;

// エンコーダー構造体
#[repr(C)] #[derive(Debug, Copy, Clone)] pub struct AACENC_BufDesc {
    pub numBufs: INT,
    pub bufs: *mut *mut ::std::os::raw::c_void,
    pub bufferIdentifiers: *mut INT,
    pub bufSizes: *mut INT,
    pub bufElSizes: *mut INT,
}
#[repr(C)] #[derive(Debug, Copy, Clone)] pub struct AACENC_InArgs {
    pub numInSamples: INT,
    pub numAncBytes: INT,
}
#[repr(C)] #[derive(Debug, Copy, Clone)] pub struct AACENC_OutArgs {
    pub numOutBytes: INT,
    pub numInSamples: INT,
    pub numAncBytes: INT,
}
#[repr(C)] #[derive(Debug, Copy, Clone)] pub struct AACENC_InfoStruct {
    pub maxOutBufBytes: UINT,
    pub maxAncBytes: UINT,
    pub inBufFillLevel: UINT,
    pub inputChannels: UINT,
    pub frameLength: UINT,
    pub nDelay: UINT,
    pub nDelayCore: UINT,
    pub confBuf: [u8; 64],
    pub confSize: UINT,
}

// デコーダー構造体
#[repr(C)] #[derive(Debug, Copy, Clone)] pub struct CStreamInfo {
    pub sampleRate: INT,
    pub frameSize: INT,
    pub numChannels: INT,
}
"#,
        )
        .expect("write file error");
        return;
    }

    // システムにインストールされた fdk-aac のヘッダから型定義とバインディングを生成する
    //
    // 関数宣言はブロックリストに登録し、生成しない。
    // 関数は実行時に共有ライブラリから動的ロードするため、
    // コンパイル時のリンクは不要。
    let include_dir = PathBuf::from(
        std::env::var(ENV_FDK_AAC_INCLUDE_DIR)
            .ok()
            .unwrap_or_else(|| "/usr/include/fdk-aac/".to_owned()),
    );
    bindgen::Builder::default()
        .header(include_dir.join("aacenc_lib.h").display().to_string())
        .header(include_dir.join("aacdecoder_lib.h").display().to_string())
        .blocklist_function(".*")
        .generate()
        .expect("failed to generate bindings")
        .write_to_file(output_bindings_path)
        .expect("failed to write bindings");
}
