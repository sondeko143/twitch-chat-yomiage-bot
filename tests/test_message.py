import unittest

from yomiage.main import parse_message


class TestMessage(unittest.TestCase):
    def test_message_parse(self):
        message = ":foo!foo@foo.tmi.twitch.tv PRIVMSG #bar :bleedPurple"
        chat_user, chat_channel, chat_message = parse_message(message)
        self.assertEquals(chat_user, "foo")
        self.assertEquals(chat_channel, "bar")
        self.assertEquals(chat_message, "bleedPurple")


if __name__ == "__main__":
    unittest.main()
