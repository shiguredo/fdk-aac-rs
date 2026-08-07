//! FDK AAC の FFI バインディング
//!
//! bindgen が生成したコードを include するためのモジュール。
//! 生成コードはリポジトリ管理外のため、lint の抑制には
//! `#[expect(...)]` を使い、警告が発生しなくなったら
//! 抑制自体を削除する必要があることに気づけるようにする。

#![expect(non_upper_case_globals)]
#![expect(non_camel_case_types)]
#![expect(non_snake_case)]
#![expect(dead_code)]
#![expect(clippy::all)]

include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
