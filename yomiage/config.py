from functools import lru_cache
from pathlib import Path
from typing import Any

from pydantic import BaseSettings


class Settings(BaseSettings):
    client_id: str
    client_secret: str
    channel: str
    username: str
    speech_port: int
    operations: list[str]
    db_dir: Path = Path("db")
    db_name: str = "data.json"

    class Config(BaseSettings.Config):
        env_file = ".env"
        env_prefix = "cb_"

        @classmethod
        def parse_env_var(cls, field_name: str, raw_val: str) -> Any:
            if field_name == "operations":
                return [x.strip() for x in raw_val.split(",")]
            return cls.json_loads(raw_val)

    @property
    def db_file(self) -> str:
        return str(self.db_dir.joinpath(self.db_name))


@lru_cache()
def get_settings():
    return Settings()  # type: ignore
