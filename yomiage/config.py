from functools import lru_cache
from typing import Any

from pydantic import BaseSettings


class Settings(BaseSettings):
    client_id: str
    client_secret: str
    channel: str
    username: str
    speech_port: int
    operations: list[str]
    db_file: str = "./pickle.db"

    class Config(BaseSettings.Config):
        env_file = ".env"

        @classmethod
        def parse_env_var(cls, field_name: str, raw_val: str) -> Any:
            if field_name == "operations":
                return [x.strip() for x in raw_val.split(",")]
            return cls.json_loads(raw_val)


@lru_cache()
def get_settings():
    return Settings()  # type: ignore
