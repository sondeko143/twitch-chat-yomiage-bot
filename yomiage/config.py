from functools import lru_cache

from pydantic import BaseSettings


class Settings(BaseSettings):
    client_id: str
    client_secret: str
    channel: str
    username: str
    speech_port: int
    db_file: str = "./pickle.db"

    class Config:  # type: ignore
        env_file = ".env"


@lru_cache()
def get_settings():
    return Settings()  # type: ignore
