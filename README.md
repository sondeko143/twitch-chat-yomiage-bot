<https://github.com/sondeko143/vstreamer-tool> これに twitch chat の読み上げさせるために作ったもの  
いい加減なつくりだ

## 使い方

### 設定

`.env` に client id , client secret, 接続するチャンネル(`channel`), アカウント名(`username`), <https://github.com/sondeko143/vstreamer-tool> のポートを指定

### Access Token を取る方法

```sh
python -m pipenv run python -m uvicorn yomiage.web:app --reload

## ブラウザで http://localhost:8000/auth にアクセスして後は画面の指示通り
```

### 起動

```sh
python -m pipenv run python -m yomiage
```
