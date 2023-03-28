import contextlib
import threading
import time
import webbrowser
from typing import Optional

import uvicorn
from pickledb import load

from yomiage.config import get_settings


class Server(uvicorn.Server):
    def install_signal_handlers(self):
        pass

    @contextlib.contextmanager
    def run_in_thread(self):
        thread = threading.Thread(target=self.run)
        thread.start()
        try:
            while not self.started:
                time.sleep(1e-3)
            yield
        finally:
            self.should_exit = True
            thread.join()


def open_auth(browser_name: Optional[str]):
    config = uvicorn.Config(
        "yomiage.web:app", host="localhost", port=8000, log_level="info"
    )
    server = Server(config=config)

    settings = get_settings()
    db = load(settings.db_file, auto_dump=False, sig=False)
    db.set("access_token", "")
    db.dump()
    with server.run_in_thread():
        browser = webbrowser.get(browser_name)
        browser.open("http://localhost:8000/auth")
        print("browser opened")
        while True:
            db = load(settings.db_file, auto_dump=False, sig=False)
            if db.get("access_token") != "":
                break
            time.sleep(1.0)
            print("sleeping")
