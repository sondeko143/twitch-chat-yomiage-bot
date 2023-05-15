# Twitch 読み上げボット

<https://github.com/sondeko143/vstreamer-tool> に twitch chat の読み上げさせるために作ったもの  

## 使い方

`.env` に client id , client secret, 接続するチャンネル(`channel`), アカウント名(`username`), <https://github.com/sondeko143/vstreamer-tool> のポートを指定

```sh
cb_client_id = "foo" # bot の client id
cb_client_secret = "bar" # bot の client secret
cb_channel = "mychannel" # 読み上げるチャンネル名
cb_username = "myusername" # bot のユーザー名
cb_speech_port = 19829 # <https://github.com/sondeko143/vstreamer-tool> の待ち受けポート
cb_operations = "trasnlate,tts,playback"
cb_db_dir = "db"
cb_db_name = "data.json"
```

### Access Token を取る方法

```sh
cargo run -- open-auth

## ブラウザで http://localhost:8000/auth にアクセスして後は画面の指示通り
```

### 起動

```sh
cargo run -- read-chat
```
