//! [FDK AAC] エンコーダー / デコーダー
//!
//! [FDK AAC] ライブラリを実行時に動的ロードして、PCM 音声データの AAC エンコード / デコードを行う。
//! ビルド時のライブラリリンクは不要で、実行時に共有ライブラリのパスを指定してロードする。
//!
//! [FDK AAC]: https://github.com/mstorsjo/fdk-aac
#![warn(missing_docs)]

// Linux 以外ではビルドを許可しない (cargo doc 時は除外)
#[cfg(all(not(target_os = "linux"), not(doc)))]
compile_error!("this crate only supports Linux");

use std::{
    collections::VecDeque,
    ffi::c_void,
    mem::MaybeUninit,
    path::{Path, PathBuf},
    sync::Arc,
};

mod dl;
mod sys;

// FDK AAC エンコーダー関数の型定義
type FnAacEncOpen =
    unsafe extern "C" fn(*mut sys::HANDLE_AACENCODER, sys::UINT, sys::UINT) -> sys::AACENC_ERROR;
type FnAacEncClose = unsafe extern "C" fn(*mut sys::HANDLE_AACENCODER) -> sys::AACENC_ERROR;
type FnAacEncoderSetParam =
    unsafe extern "C" fn(sys::HANDLE_AACENCODER, sys::AACENC_PARAM, sys::UINT) -> sys::AACENC_ERROR;
type FnAacEncEncode = unsafe extern "C" fn(
    sys::HANDLE_AACENCODER,
    *const sys::AACENC_BufDesc,
    *const sys::AACENC_BufDesc,
    *const sys::AACENC_InArgs,
    *mut sys::AACENC_OutArgs,
) -> sys::AACENC_ERROR;
type FnAacEncInfo =
    unsafe extern "C" fn(sys::HANDLE_AACENCODER, *mut sys::AACENC_InfoStruct) -> sys::AACENC_ERROR;

// FDK AAC デコーダー関数の型定義
type FnAacDecoderOpen =
    unsafe extern "C" fn(sys::TRANSPORT_TYPE, sys::UINT) -> sys::HANDLE_AACDECODER;
type FnAacDecoderClose = unsafe extern "C" fn(sys::HANDLE_AACDECODER);
type FnAacDecoderConfigRaw = unsafe extern "C" fn(
    sys::HANDLE_AACDECODER,
    *mut *mut u8,
    *const sys::UINT,
) -> sys::AAC_DECODER_ERROR;
type FnAacDecoderFill = unsafe extern "C" fn(
    sys::HANDLE_AACDECODER,
    *mut *mut u8,
    *const sys::UINT,
    *mut sys::UINT,
) -> sys::AAC_DECODER_ERROR;
type FnAacDecoderDecodeFrame = unsafe extern "C" fn(
    sys::HANDLE_AACDECODER,
    *mut i16,
    sys::INT,
    sys::UINT,
) -> sys::AAC_DECODER_ERROR;
type FnAacDecoderGetStreamInfo =
    unsafe extern "C" fn(sys::HANDLE_AACDECODER) -> *mut sys::CStreamInfo;

/// FDK AAC API のエラー
#[derive(Debug)]
pub enum Error {
    /// 共有ライブラリのロードまたはシンボル解決に失敗
    SharedLibraryError(String),

    /// FDK AAC エンコーダー / デコーダーのエラー
    FdkAacError {
        /// エラーコード
        code: std::os::raw::c_uint,
        /// エラーが発生した関数名
        function: &'static str,
    },
}

impl Error {
    fn check_encoder(code: sys::AACENC_ERROR, function: &'static str) -> Result<(), Self> {
        if code == sys::AACENC_ERROR_AACENC_OK {
            return Ok(());
        }
        Err(Self::FdkAacError { code, function })
    }

    fn check_decoder(code: sys::AAC_DECODER_ERROR, function: &'static str) -> Result<(), Self> {
        if code == sys::AAC_DECODER_ERROR_AAC_DEC_OK {
            return Ok(());
        }
        Err(Self::FdkAacError { code, function })
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::SharedLibraryError(msg) => {
                write!(f, "[{}] {}", env!("CARGO_PKG_NAME"), msg)
            }
            Error::FdkAacError { code, function } => {
                write!(
                    f,
                    "[{}] {}() failed: code={}",
                    env!("CARGO_PKG_NAME"),
                    function,
                    code
                )
            }
        }
    }
}

impl std::error::Error for Error {}

// エンコード結果を格納するための一時バッファのサイズ（バイト数）
//
// AAC-LC の 1 フレーム最大出力は 6144 bits/channel * 2 channels / 8 = 1536 bytes 程度。
// 20480 bytes は十分なマージンを持たせた値。
const ENCODE_BUF_SIZE: usize = 20480;

// デコード時の出力バッファサイズ（サンプル数）
const DECODE_BUF_SIZE: usize = 4096;

/// FDK AAC 共有ライブラリを管理するための構造体
///
/// 実行時に `libfdk-aac.so` を動的ロードし、エンコーダー / デコーダーの生成に使用する。
#[derive(Debug, Clone)]
pub struct FdkAacLibrary {
    lib: Arc<dl::DynLib>,
    path: PathBuf,
}

impl FdkAacLibrary {
    /// 指定のパスにある共有ライブラリをロードする
    ///
    /// ライブラリ名のみ（例: `"libfdk-aac.so.2"`）を指定した場合、
    /// システムのライブラリ検索パスから自動的に探索される。
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self, Error> {
        let lib = dl::DynLib::open(path.as_ref()).map_err(Error::SharedLibraryError)?;
        Ok(Self {
            lib: Arc::new(lib),
            path: path.as_ref().to_path_buf(),
        })
    }

    /// 共有ライブラリのパスを取得する
    pub fn path(&self) -> &Path {
        &self.path
    }

    fn call<F, T, U>(&self, symbol: &str, f: F) -> Result<U, Error>
    where
        F: FnOnce(T) -> U,
    {
        let func: T = unsafe {
            self.lib
                .get(symbol.as_bytes())
                .map_err(Error::SharedLibraryError)?
        };
        Ok(f(func))
    }
}

/// エンコーダーの設定
///
/// FDK AAC エンコーダーに必要な全パラメーターを保持する。
/// `Option` のフィールドは未指定時にエンコーダーのデフォルト値が使用される。
#[derive(Debug, Clone)]
pub struct EncoderConfig {
    /// 入力 PCM のサンプルレート (Hz)
    ///
    /// 0 を指定するとエラーが返る。
    pub sample_rate: u32,

    /// 入力 PCM のチャンネル数
    ///
    /// 1（モノラル）または 2（ステレオ）を指定する。
    /// 0 や 3 以上を指定するとエラーが返る。
    pub channels: u8,

    /// ターゲットビットレート (bps)
    ///
    /// 未指定時はエンコーダーのデフォルト値が使用される。
    pub bitrate: Option<u32>,
}

/// AAC エンコーダー
///
/// FDK AAC ライブラリを使用して PCM 音声データを AAC にエンコードする。
///
/// # 使用フロー
///
/// 1. [`Encoder::new()`] でインスタンスを生成する
/// 2. [`Encoder::encode()`] で PCM データを入力する（複数回呼び出し可能）
/// 3. [`Encoder::next_frame()`] でエンコード済みフレームを取り出す
/// 4. 全データの入力が完了したら [`Encoder::finish()`] を呼び出す
/// 5. 残りのフレームを [`Encoder::next_frame()`] で取り出す
#[derive(Debug)]
pub struct Encoder {
    /// 共有ライブラリ（Drop で使用）
    lib: FdkAacLibrary,
    /// エンコーダーハンドル
    handle: sys::HANDLE_AACENCODER,
    /// エンコーダーに設定されたチャンネル数
    channels: usize,
    /// エンコード結果を格納するための一時バッファ
    encode_buf: Vec<u8>,
    /// まだエンコードされていない PCM サンプルのバッファ（インターリーブ形式）
    pcm_buf: Vec<i16>,
    /// エンコード済みフレームのキュー（next_frame() で取り出される）
    encoded_frames: VecDeque<EncodedFrame>,
    /// Audio Specific Config
    audio_specific_config: Vec<u8>,
    /// 1 フレームあたりのサンプル数（チャンネルあたり）
    frame_len: usize,
    /// finish() が呼ばれたかどうか
    eos: bool,
}

impl Encoder {
    /// エンコーダーインスタンスを生成する
    ///
    /// AAC-LC エンコーダーを作成し、各種パラメーターを設定する。
    /// Afterburner（品質向上機能）はデフォルトで有効。
    pub fn new(lib: FdkAacLibrary, config: EncoderConfig) -> Result<Self, Error> {
        if config.sample_rate == 0 {
            return Err(Error::FdkAacError {
                code: sys::AACENC_ERROR_AACENC_INVALID_CONFIG,
                function: "Encoder::new(sample_rate)",
            });
        }
        if config.channels == 0 {
            return Err(Error::FdkAacError {
                code: sys::AACENC_ERROR_AACENC_INVALID_CONFIG,
                function: "Encoder::new(channels)",
            });
        }

        let channels = config.channels as usize;

        // チャンネルモード: ステレオの場合は MODE_2、モノラルの場合は MODE_1
        let channel_mode = match config.channels {
            1 => sys::CHANNEL_MODE_MODE_1,
            2 => sys::CHANNEL_MODE_MODE_2,
            _ => {
                return Err(Error::FdkAacError {
                    code: sys::AACENC_ERROR_AACENC_INVALID_CONFIG,
                    function: "Encoder::new(channels)",
                });
            }
        };

        let mut handle = std::ptr::null_mut();

        // aacEncOpen でハンドルを取得する
        let code = lib.call("aacEncOpen", |f: FnAacEncOpen| unsafe {
            f(&mut handle, 0, channels as sys::UINT)
        })?;
        Error::check_encoder(code, "aacEncOpen")?;

        // ここから先でエラーが発生した場合、Encoder の Drop で aacEncClose が呼ばれる
        let mut encoder = Self {
            lib,
            handle,
            channels,
            encode_buf: vec![0; ENCODE_BUF_SIZE],
            pcm_buf: Vec::new(),
            encoded_frames: VecDeque::new(),
            audio_specific_config: Vec::new(),
            frame_len: 0,
            eos: false,
        };

        unsafe {
            let h = encoder.handle;

            // AAC-LC (Low Complexity) を指定する
            let code = encoder
                .lib
                .call("aacEncoder_SetParam", |f: FnAacEncoderSetParam| {
                    f(
                        h,
                        sys::AACENC_PARAM_AACENC_AOT,
                        sys::AUDIO_OBJECT_TYPE_AOT_AAC_LC as sys::UINT,
                    )
                })?;
            Error::check_encoder(code, "aacEncoder_SetParam(AOT)")?;

            let code = encoder
                .lib
                .call("aacEncoder_SetParam", |f: FnAacEncoderSetParam| {
                    f(
                        h,
                        sys::AACENC_PARAM_AACENC_SAMPLERATE,
                        config.sample_rate as sys::UINT,
                    )
                })?;
            Error::check_encoder(code, "aacEncoder_SetParam(SAMPLERATE)")?;

            let code = encoder
                .lib
                .call("aacEncoder_SetParam", |f: FnAacEncoderSetParam| {
                    f(
                        h,
                        sys::AACENC_PARAM_AACENC_CHANNELMODE,
                        channel_mode as sys::UINT,
                    )
                })?;
            Error::check_encoder(code, "aacEncoder_SetParam(CHANNELMODE)")?;

            let code = encoder
                .lib
                .call("aacEncoder_SetParam", |f: FnAacEncoderSetParam| {
                    f(h, sys::AACENC_PARAM_AACENC_CHANNELORDER, 1)
                })?;
            Error::check_encoder(code, "aacEncoder_SetParam(CHANNELORDER)")?;

            // ビットレート設定（未指定時はエンコーダーのデフォルト値）
            if let Some(bitrate) = config.bitrate {
                let code = encoder
                    .lib
                    .call("aacEncoder_SetParam", |f: FnAacEncoderSetParam| {
                        f(h, sys::AACENC_PARAM_AACENC_BITRATE, bitrate as sys::UINT)
                    })?;
                Error::check_encoder(code, "aacEncoder_SetParam(BITRATE)")?;
            }

            let code = encoder
                .lib
                .call("aacEncoder_SetParam", |f: FnAacEncoderSetParam| {
                    f(
                        h,
                        sys::AACENC_PARAM_AACENC_TRANSMUX,
                        sys::TRANSPORT_TYPE_TT_MP4_RAW as sys::UINT,
                    )
                })?;
            Error::check_encoder(code, "aacEncoder_SetParam(TRANSMUX)")?;

            let code = encoder
                .lib
                .call("aacEncoder_SetParam", |f: FnAacEncoderSetParam| {
                    f(h, sys::AACENC_PARAM_AACENC_AFTERBURNER, 1)
                })?;
            Error::check_encoder(code, "aacEncoder_SetParam(AFTERBURNER)")?;

            // エンコーダーを初期化する
            let code = encoder.lib.call("aacEncEncode", |f: FnAacEncEncode| {
                f(
                    h,
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null_mut(),
                )
            })?;
            Error::check_encoder(code, "aacEncEncode")?;

            let mut info = MaybeUninit::<sys::AACENC_InfoStruct>::zeroed();
            let code = encoder
                .lib
                .call("aacEncInfo", |f: FnAacEncInfo| f(h, info.as_mut_ptr()))?;
            Error::check_encoder(code, "aacEncInfo")?;

            let info = info.assume_init();
            encoder.audio_specific_config = info.confBuf[..info.confSize as usize].to_vec();
            encoder.frame_len = info.frameLength as usize;
        }

        Ok(encoder)
    }

    /// MP4 のサンプルエントリーに設定するデコーダー向けの情報
    pub fn audio_specific_config(&self) -> &[u8] {
        &self.audio_specific_config
    }

    /// PCM 音声データをエンコードする
    ///
    /// インターリーブ形式の i16 PCM データを受け取り、内部バッファに蓄積する。
    /// 十分なサンプルが蓄積されるとエンコードを実行する。
    /// エンコード結果は [`Encoder::next_frame()`] で取得できる。
    pub fn encode(&mut self, pcm: &[i16]) -> Result<(), Error> {
        self.pcm_buf.extend_from_slice(pcm);
        while self.pcm_buf.len() >= self.frame_len * self.channels {
            match self.encode_impl()? {
                Some(frame) => self.encoded_frames.push_back(frame),
                None => break,
            }
        }
        Ok(())
    }

    /// エンコーダーに、これ以上データが来ないことを伝える
    ///
    /// 内部バッファに残っている PCM データをすべてエンコードする。
    /// 残りのエンコード結果は [`Encoder::next_frame()`] で取得できる。
    pub fn finish(&mut self) -> Result<(), Error> {
        self.eos = true;
        // pcm_buf が空になるまでエンコードを繰り返す。
        // encode_impl() が None を返しても pcm_buf にデータが残っている場合は
        // エンコーダーが内部バッファリング中なので再度呼び出す。
        while !self.pcm_buf.is_empty() {
            if let Some(frame) = self.encode_impl()? {
                self.encoded_frames.push_back(frame);
            }
        }
        Ok(())
    }

    /// エンコード済みのフレームを取り出す
    ///
    /// エンコード結果がない場合は `None` を返す。
    pub fn next_frame(&mut self) -> Option<EncodedFrame> {
        self.encoded_frames.pop_front()
    }

    /// aacEncEncode を呼び出して 1 パケット分のエンコードを試みる
    fn encode_impl(&mut self) -> Result<Option<EncodedFrame>, Error> {
        if self.pcm_buf.is_empty() {
            return Ok(None);
        }

        let in_buf = MaybeUninit::<sys::AACENC_BufDesc>::zeroed();
        let out_buf = MaybeUninit::<sys::AACENC_BufDesc>::zeroed();
        let in_elem_size = 2;
        let out_elem_size = 1;
        let in_args = MaybeUninit::<sys::AACENC_InArgs>::zeroed();
        let mut out_args = MaybeUninit::<sys::AACENC_OutArgs>::zeroed();
        unsafe {
            let mut in_args = in_args.assume_init();
            in_args.numInSamples = self.pcm_buf.len() as sys::INT;

            let mut in_buf = in_buf.assume_init();

            // 一時配列を直接フィールドに代入してしまうと、
            // リリースビルド時のコンパイラの最適化によってポインタが無効になることがあるので、
            // 一度変数を経由する
            let mut in_buf_bufs = [self.pcm_buf.as_ptr() as *mut c_void];
            let mut in_buf_buffer_identifiers = [sys::AACENC_BufferIdentifier_IN_AUDIO_DATA as i32];
            let mut in_buf_buf_sizes = [self.pcm_buf.len() as sys::INT * in_elem_size];
            let mut in_buf_buf_el_sizes = [in_elem_size];

            in_buf.numBufs = 1;
            in_buf.bufs = in_buf_bufs.as_mut_ptr();
            in_buf.bufferIdentifiers = in_buf_buffer_identifiers.as_mut_ptr();
            in_buf.bufSizes = in_buf_buf_sizes.as_mut_ptr();
            in_buf.bufElSizes = in_buf_buf_el_sizes.as_mut_ptr();

            let mut out_buf = out_buf.assume_init();

            // in_buf_* と同様にこちらも変数を経由してポインタを取得する
            let mut out_buf_bufs = [self.encode_buf.as_mut_ptr() as *mut c_void];
            let mut out_buf_buffer_identifiers =
                [sys::AACENC_BufferIdentifier_OUT_BITSTREAM_DATA as i32];
            let mut out_buf_buf_sizes = [self.encode_buf.len() as sys::INT];
            let mut out_buf_buf_el_sizes = [out_elem_size];

            out_buf.numBufs = 1;
            out_buf.bufs = out_buf_bufs.as_mut_ptr();
            out_buf.bufferIdentifiers = out_buf_buffer_identifiers.as_mut_ptr();
            out_buf.bufSizes = out_buf_buf_sizes.as_mut_ptr();
            out_buf.bufElSizes = out_buf_buf_el_sizes.as_mut_ptr();

            let h = self.handle;
            let channels = self.channels;
            let out_args_ptr = out_args.as_mut_ptr();
            let code = self.lib.call("aacEncEncode", |f: FnAacEncEncode| {
                f(h, &in_buf, &out_buf, &in_args, out_args_ptr)
            })?;
            Error::check_encoder(code, "aacEncEncode")?;

            let out_args = out_args.assume_init();
            let consumed = out_args.numInSamples as usize;
            self.pcm_buf.drain(..consumed);

            // consumed == 0 かつ出力もない場合はこれ以上進まない
            if consumed == 0 && out_args.numOutBytes == 0 {
                return Ok(None);
            }

            let data = self.encode_buf[..out_args.numOutBytes as usize].to_vec();
            Ok(Some(EncodedFrame {
                data,
                samples: consumed / channels,
            }))
        }
    }
}

impl Drop for Encoder {
    fn drop(&mut self) {
        let _ = self.lib.call("aacEncClose", |f: FnAacEncClose| unsafe {
            f(&mut self.handle)
        });
    }
}

// HANDLE_AACENCODER 自体はスレッドセーフではないが、
// Encoder は &mut self を要求するため、同時アクセスは Rust の型システムで防がれる。
// Sync は実装しない: HANDLE_AACENCODER が内部的にスレッドセーフでないため。
unsafe impl Send for Encoder {}

/// エンコードされた AAC フレーム
///
/// 1 回のエンコードで生成される圧縮データとメタデータを保持する。
#[derive(Debug)]
pub struct EncodedFrame {
    /// 圧縮データ
    pub data: Vec<u8>,

    /// このフレームに含まれている PCM サンプル数（チャンネルあたり）
    pub samples: usize,
}

/// AAC デコーダー
///
/// FDK AAC ライブラリを使用して AAC 圧縮データを PCM にデコードする。
///
/// # 使用フロー
///
/// 1. [`Decoder::new()`] でインスタンスを生成する
/// 2. [`Decoder::decode()`] で圧縮データを入力する（1 パケットずつ）
/// 3. [`Decoder::next_frame()`] でデコード済みフレームを取り出す
/// 4. 全データの入力が完了したら [`Decoder::finish()`] を呼び出す
/// 5. 残りのフレームを [`Decoder::next_frame()`] で取り出す
#[derive(Debug)]
pub struct Decoder {
    /// 共有ライブラリ（Drop で使用）
    lib: FdkAacLibrary,
    /// デコーダーハンドル
    handle: sys::HANDLE_AACDECODER,
    /// デコード待ちの圧縮パケットのキュー
    encoded_packets: VecDeque<Vec<u8>>,
    /// finish() が呼ばれたかどうか
    eos: bool,
}

impl Decoder {
    /// デコーダーインスタンスを生成する
    ///
    /// Audio Specific Config バッファを指定して、AAC デコーダーを初期化する。
    pub fn new(lib: FdkAacLibrary, audio_specific_config: &[u8]) -> Result<Self, Error> {
        if audio_specific_config.is_empty() {
            // 空が指定されると SIGSEGV となることがあるのでここで弾く
            return Err(Error::FdkAacError {
                code: sys::AAC_DECODER_ERROR_AAC_DEC_UNKNOWN,
                function: "Decoder::new(audio_specific_config is empty)",
            });
        }

        let handle = lib.call("aacDecoder_Open", |f: FnAacDecoderOpen| unsafe {
            f(sys::TRANSPORT_TYPE_TT_MP4_RAW, 1)
        })?;
        if handle.is_null() {
            return Err(Error::FdkAacError {
                code: sys::AAC_DECODER_ERROR_AAC_DEC_UNKNOWN,
                function: "aacDecoder_Open",
            });
        }

        // ここから先でエラーが発生した場合、Decoder の Drop で aacDecoder_Close が呼ばれる
        let decoder = Self {
            lib,
            handle,
            encoded_packets: VecDeque::new(),
            eos: false,
        };

        unsafe {
            let h = decoder.handle;
            let mut conf = [audio_specific_config.as_ptr() as *mut u8];
            let length = [audio_specific_config.len() as sys::UINT];

            let code = decoder
                .lib
                .call("aacDecoder_ConfigRaw", |f: FnAacDecoderConfigRaw| {
                    f(h, conf.as_mut_ptr(), length.as_ptr())
                })?;
            Error::check_decoder(code, "aacDecoder_ConfigRaw")?;
        }

        Ok(decoder)
    }

    /// AAC 圧縮データをデコーダーに入力する
    ///
    /// 1 回の呼び出しで 1 パケット分のデータを渡す。
    /// デコード結果は [`Decoder::next_frame()`] で取得できる。
    pub fn decode(&mut self, encoded: &[u8]) -> Result<(), Error> {
        self.encoded_packets.push_back(encoded.to_vec());
        Ok(())
    }

    /// デコーダーに、これ以上データが来ないことを伝える
    pub fn finish(&mut self) -> Result<(), Error> {
        self.eos = true;
        Ok(())
    }

    /// デコード済みのフレームを取り出す
    ///
    /// キューにあるパケットを 1 つデコードして返す。
    /// パケットがない場合は `None` を返す。
    pub fn next_frame(&mut self) -> Result<Option<DecodedFrame>, Error> {
        let Some(packet) = self.encoded_packets.pop_front() else {
            return Ok(None);
        };
        self.decode_packet(&packet)
    }

    /// 1 パケット分のデータをデコードする
    fn decode_packet(&mut self, encoded: &[u8]) -> Result<Option<DecodedFrame>, Error> {
        unsafe {
            let mut buf = [encoded.as_ptr() as *mut u8];
            let buf_size = [encoded.len() as sys::UINT];
            let mut bytes_valid = encoded.len() as sys::UINT;

            let h = self.handle;

            // デコーダーの入力バッファにデータを充填する
            let code = self.lib.call("aacDecoder_Fill", |f: FnAacDecoderFill| {
                f(h, buf.as_mut_ptr(), buf_size.as_ptr(), &mut bytes_valid)
            })?;
            Error::check_decoder(code, "aacDecoder_Fill")?;

            // デコード用バッファを準備
            // aacDecoder_DecodeFrame の timeDataSize は PCM サンプル数を期待する
            let mut decode_buf = vec![0i16; DECODE_BUF_SIZE];
            let decode_buf_ptr = decode_buf.as_mut_ptr();
            let decode_buf_size = decode_buf.len() as sys::INT;

            // フレームをデコードする
            let code = self
                .lib
                .call("aacDecoder_DecodeFrame", |f: FnAacDecoderDecodeFrame| {
                    f(h, decode_buf_ptr, decode_buf_size, 0)
                })?;

            // AAC_DEC_NOT_ENOUGH_BITS は入力データ不足を示す
            if code == sys::AAC_DECODER_ERROR_AAC_DEC_NOT_ENOUGH_BITS {
                return Ok(None);
            }
            if code != sys::AAC_DECODER_ERROR_AAC_DEC_OK {
                return Err(Error::FdkAacError {
                    code,
                    function: "aacDecoder_DecodeFrame",
                });
            }

            // ストリーム情報を取得
            let stream_info = self.lib.call(
                "aacDecoder_GetStreamInfo",
                |f: FnAacDecoderGetStreamInfo| f(h),
            )?;
            if stream_info.is_null() {
                return Ok(None);
            }

            let stream_info = &*stream_info;
            let frame_size = stream_info.frameSize as usize;
            let num_channels = stream_info.numChannels as u8;
            let sample_rate = stream_info.sampleRate as u32;
            let total_samples = frame_size * num_channels as usize;

            // バッファを実際のサンプル数に縮小
            decode_buf.truncate(total_samples);

            Ok(Some(DecodedFrame {
                data: decode_buf,
                samples: frame_size,
                channels: num_channels,
                sample_rate,
            }))
        }
    }
}

impl Drop for Decoder {
    fn drop(&mut self) {
        let _ = self
            .lib
            .call("aacDecoder_Close", |f: FnAacDecoderClose| unsafe {
                f(self.handle)
            });
    }
}

// HANDLE_AACDECODER 自体はスレッドセーフではないが、
// Decoder は &mut self を要求するため、同時アクセスは Rust の型システムで防がれる。
// Sync は実装しない: HANDLE_AACDECODER が内部的にスレッドセーフでないため。
unsafe impl Send for Decoder {}

/// デコードされた AAC フレーム
///
/// 1 回のデコードで生成される PCM データとメタデータを保持する。
#[derive(Debug)]
pub struct DecodedFrame {
    /// PCM データ（インターリーブ形式）
    pub data: Vec<i16>,

    /// フレーム内のサンプル数（チャンネル数は含まない）
    pub samples: usize,

    /// チャンネル数
    pub channels: u8,

    /// サンプルレート (Hz)
    pub sample_rate: u32,
}

impl DecodedFrame {
    /// フレーム内の総サンプル数 (チャンネル数 * samples)
    pub fn total_samples(&self) -> usize {
        self.samples * self.channels as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    const TEST_SAMPLE_RATE: u32 = 48000;
    const TEST_CHANNELS: u8 = 2;

    // fdk-aac の C ライブラリはグローバル状態を持つため、
    // テストの並列実行で double free が発生する。
    // Mutex で直列化して安全に実行する。
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn load_library() -> FdkAacLibrary {
        let path = std::env::var("FDK_AAC_PATH").unwrap_or_else(|_| "libfdk-aac.so.2".to_string());
        FdkAacLibrary::load(path).expect("load library error")
    }

    fn encoder_config(bitrate: Option<u32>) -> EncoderConfig {
        EncoderConfig {
            sample_rate: TEST_SAMPLE_RATE,
            channels: TEST_CHANNELS,
            bitrate,
        }
    }

    #[test]
    fn test_load_library() {
        let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        load_library();
    }

    #[test]
    fn init_encoder() {
        let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
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

    #[test]
    fn encode_silent() {
        let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let lib = load_library();
        let mut encoder =
            Encoder::new(lib, encoder_config(Some(100_000))).expect("create encoder error");
        let mut sample_count = 0;

        for _ in 0..100 {
            encoder
                .encode(&[0; 100 * TEST_CHANNELS as usize])
                .expect("encode error");
            while let Some(encoded) = encoder.next_frame() {
                sample_count += encoded.samples;
            }
        }
        encoder.finish().expect("finish error");
        while let Some(encoded) = encoder.next_frame() {
            sample_count += encoded.samples;
        }

        assert_eq!(sample_count, 100 * 100);
    }

    #[test]
    fn init_decoder() {
        let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let lib = load_library();
        let encoder =
            Encoder::new(lib.clone(), encoder_config(Some(100_000))).expect("create encoder error");
        let asc = encoder.audio_specific_config();

        // Audio Specific Config が正しい
        assert!(Decoder::new(lib.clone(), asc).is_ok());

        // Audio Specific Config が空の場合はエラーになる
        assert!(Decoder::new(lib, &[]).is_err());
    }

    #[test]
    fn decode_silent() {
        let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let lib = load_library();
        let mut encoder =
            Encoder::new(lib.clone(), encoder_config(Some(100_000))).expect("create encoder error");

        // 無音のオーディオをエンコード
        let pcm_data = vec![0i16; 1024 * TEST_CHANNELS as usize];
        encoder.encode(&pcm_data).expect("encode error");
        encoder.finish().expect("finish error");

        let mut encoded_frames = Vec::new();
        while let Some(frame) = encoder.next_frame() {
            encoded_frames.push(frame);
        }

        // デコーダーを初期化
        let asc = encoder.audio_specific_config();
        let mut decoder = Decoder::new(lib, asc).expect("create decoder error");

        // エンコードされたフレームをデコード
        for frame in &encoded_frames {
            decoder.decode(&frame.data).expect("decode error");
        }
        decoder.finish().expect("finish error");

        let mut total_decoded = 0;
        while let Some(decoded) = decoder.next_frame().expect("next_frame error") {
            assert_eq!(decoded.channels, TEST_CHANNELS);
            assert_eq!(decoded.sample_rate, TEST_SAMPLE_RATE);
            total_decoded += decoded.samples;
        }

        // デコードされたサンプル数が入力サンプル数と一致することを確認
        assert!(total_decoded > 0, "expected to decode some samples");
    }

    /// 正弦波を生成してエンコード → デコードし、
    /// デコード結果が無音でないことを確認するラウンドトリップテスト
    #[test]
    fn roundtrip_sine_wave() {
        let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let lib = load_library();
        let mut encoder =
            Encoder::new(lib.clone(), encoder_config(Some(128_000))).expect("create encoder error");

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
        encoder.encode(&pcm_input).expect("encode error");
        encoder.finish().expect("finish error");

        let mut encoded_frames = Vec::new();
        while let Some(frame) = encoder.next_frame() {
            encoded_frames.push(frame);
        }
        assert!(
            !encoded_frames.is_empty(),
            "expected at least one encoded frame"
        );

        // デコード
        let asc = encoder.audio_specific_config();
        let mut decoder = Decoder::new(lib, asc).expect("create decoder error");

        for frame in &encoded_frames {
            decoder.decode(&frame.data).expect("decode error");
        }
        decoder.finish().expect("finish error");

        let mut pcm_output: Vec<i16> = Vec::new();
        while let Some(decoded) = decoder.next_frame().expect("next_frame error") {
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
            "decoded samples ({output_samples}) is too few compared to input samples ({num_samples})"
        );

        // デコード結果が無音でないことを確認する（RMS が閾値以上）
        let rms = {
            let sum_sq: f64 = pcm_output.iter().map(|&s| (s as f64) * (s as f64)).sum();
            (sum_sq / pcm_output.len() as f64).sqrt()
        };
        assert!(
            rms > 1000.0,
            "decoded output is too quiet (RMS: {rms}), expected audible signal"
        );
    }
}
