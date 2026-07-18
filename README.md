# fswiki-lsp

FreeStyleWiki (`.fsw`, `.fswiki`) 用の Rust 製 Language Server です。
[`go-fswiki`](https://github.com/entooone/go-fswiki) をベースに Rust で実装したものです。
いくつかの機能は独自実装です。

## 機能

- 文書全体のフォーマット
  - 見出し、リスト、段落、整形済みテキスト、テーブル、コメント、プラグイン
  - 日本語の表示幅を考慮したテーブル整形
  - セル内のカンマと `""` によるダブルクォートのエスケープ
  - 入力の LF/CRLF とテーブル末尾空白スタイルを維持
- 見出しから階層的な Document Symbol を生成
- 現在のセクションの見出しタイトル／階層パス／内容をコピーする Code Action
- 見出しセクションと複数行プラグインの Folding Range を生成
- 構文エラーと文書構造の警告をリアルタイムに診断
- 見出し、リスト、プラグイン、インライン記法のスニペット補完
- リスト内で改行したときに同じ種類と階層のマーカーを継続
- UTF-16 の LSP 位置と文書同期に対応

通信には標準入出力を使用します。

## エディタ対応状況

正式に対応しているエディタは下記です。

- Zed

## ビルドとインストール

ローカルでビルドをする際は下記の手順に従います。

```sh
cargo build --release
cargo install --path .
```

## リリース

`v<major>.<minor>.<patch>` 形式のタグを push すると、`.github/workflows/release.yml` がテストとリリースビルドを実行し、同じタグの GitHub Release を作成します。タグのバージョンは `Cargo.toml` の `package.version` と一致している必要があります。

```sh
TAG="v1.0.0"
git tag "${TAG}"
git push origin "${TAG}"
```

Release には Linux、macOS、Windows の x86_64／AArch64 向け archive と `SHA256SUMS` が登録されます。

## 開発

```sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```
