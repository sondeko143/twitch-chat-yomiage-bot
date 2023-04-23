import asyncio
import logging
import re
from dataclasses import dataclass
from enum import Enum

import click
import grpc
from pickledb import PickleDB
from pickledb import load
from vstreamer_protos.commander.commander_pb2 import TRANSLATE
from vstreamer_protos.commander.commander_pb2 import Command
from vstreamer_protos.commander.commander_pb2_grpc import CommanderStub
from websockets.client import connect
from websockets.exceptions import ConnectionClosed

from yomiage.api import ban_user
from yomiage.api import refresh_access_token
from yomiage.config import Settings
from yomiage.config import get_settings
from yomiage.server import open_auth

logger = logging.getLogger("websockets")
logger.setLevel(logging.DEBUG)
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


async def send_message(chat_message: str, port: int):
    try:
        async with grpc.aio.insecure_channel(f"localhost:{port}") as channel:
            stub = CommanderStub(channel)
            await stub.process_command(
                Command(operations=[TRANSLATE], text=chat_message)
            )
    except grpc.aio.AioRpcError as e:
        logger.warning(e)


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
                    await send_message(parsed.chat_message, port)
                elif parsed.message_type == MessageType.LOGIN_FAILED:
                    settings = get_settings()
                    refresh_access_token(db, settings)
                    logger.info("Refresh access token.")
                    break
                elif parsed.message_type == MessageType.UNKNOWN:
                    continue
        except ConnectionClosed:
            continue


def run_yomiage_bot(db: PickleDB, settings: Settings):
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
@click.option("-a", "--get-auth", is_flag=True)
@click.option("-b", "--ban-bots", is_flag=True)
def main(refresh: bool, get_auth: bool, ban_bots: bool):
    settings = get_settings()
    db = load(settings.db_file, auto_dump=False)
    if get_auth:
        open_auth(None)
    elif refresh:
        refresh_access_token(db, settings)
    elif ban_bots:
        ban_user(db, settings)
    else:
        run_yomiage_bot(db, settings)
