import logging
import uuid
from typing import Annotated

from fastapi import BackgroundTasks
from fastapi import Depends
from fastapi import FastAPI
from fastapi.responses import RedirectResponse
from pickledb import PickleDB
from pickledb import load

from yomiage.auth import get_access_token
from yomiage.auth import refresh_access_token
from yomiage.config import Settings
from yomiage.config import get_settings

logger = logging.getLogger("websockets")
logger.setLevel(logging.DEBUG)
logger.addHandler(logging.StreamHandler())


app = FastAPI()


def get_db(settings: Annotated[Settings, Depends(get_settings)]):
    db = load(settings.db_file, auto_dump=False, sig=False)
    try:
        yield db
    finally:
        db.dump()


@app.get("/callback")
async def root(
    code: str,
    background_tasks: BackgroundTasks,
    settings: Annotated[Settings, Depends(get_settings)],
    db: PickleDB = Depends(get_db),
):
    background_tasks.add_task(get_access_token, code, db, settings)
    return {"result": "success"}


@app.get("/auth")
async def auth(settings: Annotated[Settings, Depends(get_settings)]):
    params = {
        "client_id": settings.client_id,
        "redirect_uri": "http://localhost:8000/callback",
        "response_type": "code",
        "scope": "chat:read",
        "force_verify": "true",
        "state": str(uuid.uuid4()),
    }
    queries = "&".join([key + "=" + value for key, value in params.items()])

    return RedirectResponse(f"https://id.twitch.tv/oauth2/authorize?{queries}")


@app.get("/refresh")
async def refresh(
    background_tasks: BackgroundTasks,
    settings: Annotated[Settings, Depends(get_settings)],
    db: PickleDB = Depends(get_db),
):
    background_tasks.add_task(refresh_access_token, db, settings)
    return {"result": "success"}


@app.get("/dump")
async def dump(db: PickleDB = Depends(get_db)):
    keys = db.getall()
    return {key: db.get(key) for key in keys}
