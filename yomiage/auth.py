import httpx
from pickledb import PickleDB

from yomiage.config import Settings


def get_access_token(code: str, db: PickleDB, settings: Settings):
    headers = [(b"content-type", b"application/x-www-form-urlencoded")]
    response = httpx.post(
        "https://id.twitch.tv/oauth2/token",
        headers=headers,
        data={
            "client_id": settings.client_id,
            "client_secret": settings.client_secret,
            "code": code,
            "grant_type": "authorization_code",
            "redirect_uri": "http://localhost:8000/callback",
        },
    )
    body = response.json()
    db.set("access_token", body["access_token"])
    db.set("refresh_token", body["refresh_token"])
    db.dump()


def refresh_access_token(db: PickleDB, settings: Settings):
    headers = [(b"content-type", b"application/x-www-form-urlencoded")]
    response = httpx.post(
        "https://id.twitch.tv/oauth2/token",
        headers=headers,
        data={
            "refresh_token": db.get("refresh_token"),
            "client_id": settings.client_id,
            "grant_type": "refresh_token",
            "client_secret": settings.client_secret,
        },
    )
    body = response.json()
    db.set("access_token", body["access_token"])
    db.set("refresh_token", body["refresh_token"])
    db.dump()
