import asyncio
import logging
import re
from codecs import encode
from dataclasses import dataclass
from enum import Enum
from socket import AF_INET
from socket import SOCK_STREAM
from socket import socket

import click
from pickledb import PickleDB
from pickledb import load
from websockets.client import connect
from websockets.exceptions import ConnectionClosed

from yomiage.auth import refresh_access_token
from yomiage.config import Settings
from yomiage.config import get_settings

logger = logging.getLogger("websockets")
logger.setLevel(logging.ERROR)
logger.addHandler(logging.StreamHandler())


MESSAGE_PATTERN = re.compile(r":(.+)!.+@.+\.tmi\.twitch\.tv PRIVMSG #(.+) :(.+)")
LOGIN_FAILED_PATTERN = re.compile(
    r":tmi\.twitch\.tv NOTICE \* :Login authentication failed\s*"
)

logger = logging.getLogger("tasks")
logger.setLevel(logging.INFO)
logger.addHandler(logging.StreamHandler())


class MessageType(Enum):
    CHAT_READ = 1
    LOGIN_FAILED = 2
    UNKNOWN = 999


@dataclass
class Message:
    message_type: MessageType
    user: str
    channel: str
    chat_message: str


def parse_message(message: str):
    match = re.findall(MESSAGE_PATTERN, message)
    if match and len(match[0]) == 3:
        return Message(
            message_type=MessageType.CHAT_READ,
            user=match[0][0],
            channel=match[0][1],
            chat_message=match[0][2],
        )
    match = re.findall(LOGIN_FAILED_PATTERN, message)
    if match:
        return Message(
            message_type=MessageType.LOGIN_FAILED,
            user="",
            channel="",
            chat_message="",
        )
    return Message(
        message_type=MessageType.UNKNOWN,
        user="",
        channel="",
        chat_message="",
    )


def send_message(chat_message: str, port: int):
    message = "t" + chat_message + "\n"
    logger.debug(message)
    output_bytes = encode(message, "utf-8", errors="replace")
    try:
        with socket(AF_INET, SOCK_STREAM) as sock:
            sock.connect(("localhost", port))
            sock.sendall(output_bytes)
    except ConnectionRefusedError:
        logger.warning("The speech process does not seem to working.")


async def read_chat(uri: str, username: str, channel: str, port: int, db: PickleDB):
    async for connection in connect(uri):
        try:
            await connection.send(f"PASS oauth:{db.get('access_token')}")
            await connection.send(f"NICK {username}")
            await connection.send(f"JOIN #{channel}")
            async for message in connection:
                parsed = parse_message(str(message))
                if parsed.message_type == MessageType.CHAT_READ:
                    logger.info(
                        "In %s, %s says %s",
                        parsed.channel,
                        parsed.user,
                        parsed.chat_message,
                    )
                    send_message(parsed.chat_message, port)
                elif parsed.message_type == MessageType.LOGIN_FAILED:
                    settings = get_settings()
                    refresh_access_token(db, settings)
                    logger.info("Refresh access token.")
                    break
                elif parsed.message_type == MessageType.UNKNOWN:
                    continue
        except ConnectionClosed:
            continue


def run_bot(db: PickleDB, settings: Settings):
    asyncio.run(
        read_chat(
            "wss://irc-ws.chat.twitch.tv:443",
            settings.username,
            settings.channel,
            settings.speech_port,
            db,
        )
    )


@click.command()
@click.option("-r", "--refresh", is_flag=True)
def main(refresh: bool):
    settings = get_settings()
    db = load(settings.db_file, auto_dump=False)
    if refresh:
        refresh_access_token(db, settings)
    run_bot(db, settings)