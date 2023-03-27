import unittest

from yomiage.main import MessageType
from yomiage.main import parse_message


class TestMessage(unittest.TestCase):
    def test_chat_message_parse(self):
        message = ":foo!foo@foo.tmi.twitch.tv PRIVMSG #bar :bleedPurple"
        parsed = parse_message(message)
        self.assertEquals(parsed.message_type, MessageType.CHAT_READ)
        self.assertEquals(parsed.user, "foo")
        self.assertEquals(parsed.channel, "bar")
        self.assertEquals(parsed.chat_message, "bleedPurple")

    def test_login_failed_message_parse(self):
        message = ":tmi.twitch.tv NOTICE * :Login authentication failed\r\n"
        parsed = parse_message(message)
        self.assertEquals(parsed.message_type, MessageType.LOGIN_FAILED)

    def test_unknown_message_parse(self):
        message = "PING :tmi.twitch.tv\r\n"
        parsed = parse_message(message)
        self.assertEquals(parsed.message_type, MessageType.UNKNOWN)


if __name__ == "__main__":
    unittest.main()
