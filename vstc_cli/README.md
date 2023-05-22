# vstc cli

<https://github.com/sondeko143/vstreamer-tool> の cli クライアント

## 使い方

```sh
cargo run -- --help
Usage: vstc_cli.exe [OPTIONS] [OPERATIONS]...

Arguments:
  [OPERATIONS]...

Options:
  -t, --text <TEXT>            Text input
  -w, --wav <WAV>              Sound input
      --file-path <FILE_PATH>  Reload config file
      --filters <FILTERS>      Filters
  -H, --host <HOST>            Host name
  -p, --port <PORT>            Port
  -h, --help                   Print help
  -V, --version                Print version
```

```sh
# 例) 翻訳して読み上げ
./vstc_cli.exe transl tts play -p 19829 -t "hello, world"
```
