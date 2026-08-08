//! ブラウザーsmoke試験用のHTMLシェルを標準出力へ書き出す。

fn main() {
    print!("{}", marginalis_web::http::browser_smoke_shell());
}
