import asyncio
import logging
import re
from codecs import encode
from socket import AF_INET
from socket import SOCK_STREAM
from socket import socket

from pickledb import PickleDB
from pickledb import load
from websockets.client import connect
from websockets.exceptions import ConnectionClosed

from yomiage.config import Settings
from yomiage.config import get_settings

logger = logging.getLogger("websockets")
logger.setLevel(logging.DEBUG)
logger.addHandler(logging.StreamHandler())


PATTERN = re.compile(r":(.+)!.+@.+\.tmi\.twitch\.tv PRIVMSG #(.+) :(.+)")

logger = logging.getLogger("tasks")
logger.setLevel(logging.DEBUG)
logger.addHandler(logging.StreamHandler())


def parse_message(message: str):
    match = re.findall(PATTERN, message)
    if not match or len(match[0]) != 3:
        logger.debug("does not match '%s", message)
        return "", "", ""
    return match[0]


def send_message(chat_message: str, port: int):
    message = "t" + chat_message + "\n"
    logger.info(message)
    output_bytes = encode(message, "utf-8", errors="replace")
    with socket(AF_INET, SOCK_STREAM) as sock:
        sock.connect(("localhost", port))
        sock.sendall(output_bytes)


async def read_chat(
    uri: str, access_token: str, username: str, channel: str, port: int
):
    async for connection in connect(uri):
        try:
            await connection.send(f"PASS oauth:{access_token}")
            await connection.send(f"NICK {username}")
            await connection.send(f"JOIN #{channel}")
            async for message in connection:
                chat_user, chat_channel, chat_message = parse_message(str(message))
                if not chat_message:
                    continue
                logger.debug("In %s, %s says %s", chat_channel, chat_user, chat_message)
                send_message(chat_message, port)
        except ConnectionClosed:
            continue


def run_bot(db: PickleDB, settings: Settings):
    asyncio.run(
        read_chat(
            "wss://irc-ws.chat.twitch.tv:443",
            str(db.get("access_token")),
            settings.username,
            settings.channel,
            settings.speech_port,
        )
    )


def main():
    settings = get_settings()
    db = load(settings.db_file, auto_dump=False)
    run_bot(db, settings)
