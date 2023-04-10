import logging
from time import sleep
from typing import List
from typing import cast

import httpx
from pickledb import PickleDB

from yomiage.config import Settings

logger = logging.getLogger("httpx")
logger.setLevel(logging.DEBUG)
logger.addHandler(logging.StreamHandler())


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


def get_user_id(username: str, access_token: str, client_id: str):
    headers = [
        (b"Authorization", b"Bearer " + bytes(access_token, "utf-8")),
        (b"content-type", b"application/x-www-form-urlencoded"),
        (b"Client-Id", bytes(client_id, "utf-8")),
    ]
    response = httpx.get(
        "https://api.twitch.tv/helix/users",
        params={"login": username},
        headers=headers,
    )
    body = response.json()
    print(body)
    if not body["data"]:
        raise TypeError("no data")
    return body["data"][0]["id"]


def get_bot_list_from_twitch_insights() -> List[str]:
    response = httpx.get("https://api.twitchinsights.net/v1/bots/online")
    body = response.json()
    return [bot[0] for bot in body["bots"]]


def get_bot_list() -> List[str]:
    response = httpx.get(
        "https://raw.githubusercontent.com/arrowgent/Twitchtv-Bots-List/main/list.txt"
    )
    return [name.strip() for name in response.text.splitlines()]


def ban_bot_list():
    bot_names = get_bot_list()
    response = httpx.get("https://mreliasen.github.io/twitch-bot-list/whitelist.json")
    body = response.json()
    whitelist_bot_names = body
    return [bot_name for bot_name in bot_names if bot_name not in whitelist_bot_names]


def ban_user(db: PickleDB, settings: Settings):
    ban_list = ban_bot_list()
    print(ban_list)
    for banned_username in ban_list:
        access_token = cast(str, db.get("access_token"))
        user_id = db.get("user_id")
        if not user_id:
            user_id = get_user_id(
                settings.username,
                access_token=access_token,
                client_id=settings.client_id,
            )
            db.set("user_id", user_id)
            db.dump()
        try:
            banned_user_id = get_user_id(
                username=banned_username,
                access_token=access_token,
                client_id=settings.client_id,
            )
        except TypeError:
            continue
        headers = [
            (b"Authorization", b"Bearer " + bytes(access_token, "utf-8")),
            (b"content-type", b"application/json"),
            (b"Client-Id", bytes(settings.client_id, "utf-8")),
        ]
        response = httpx.post(
            "https://api.twitch.tv/helix/moderation/bans",
            params={"broadcaster_id": user_id, "moderator_id": user_id},
            headers=headers,
            json={"data": {"user_id": banned_user_id, "reason": "bot"}},
        )
        body = response.json()
        print(body)
        sleep(0.1)
