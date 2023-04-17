# Twitch 読み上げボット

<https://github.com/sondeko143/vstreamer-tool> に twitch chat の読み上げさせるために作ったもの  

## 使い方

poetry で依存パッケージをインストール

```sh
poetry install
```

### 設定

`.env` に client id , client secret, 接続するチャンネル(`channel`), アカウント名(`username`), <https://github.com/sondeko143/vstreamer-tool> のポートを指定

```sh
client_id = "foo"
client_secret = "bar"
channel = "mychannel"
username = "myusername"
speech_port = 19829 # <https://github.com/sondeko143/vstreamer-tool> の待ち受けポート
```

### Access Token を取る方法

```sh
python -m poetry run yomiage -a

## ブラウザで http://localhost:8000/auth にアクセスして後は画面の指示通り
```

### 起動

```sh
python -m pipenv run python -m yomiage
```
